use crate::error::ApiError;
use axum::extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use eaeh_core::{
    ApprovalId, ArtifactId, HarnessError, Principal, PrincipalId, PrincipalKind, ProjectAccess, ProjectId,
    Role, Scope, TokenId, WorkflowDefinitionId, WorkflowInstanceId,
};
use eaeh_governance::authz::{Action, Target};
use eaeh_governance::{ApprovalDecision, ApprovalStatus, NewPrincipal};
use eaeh_runtime::Harness;
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct AppState {
    pub harness: Arc<Harness>,
    limiter: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl AppState {
    pub fn new(harness: Arc<Harness>) -> Arc<Self> {
        Arc::new(Self {
            harness,
            limiter: Mutex::new(HashMap::new()),
        })
    }
}

type St = State<Arc<AppState>>;
type ApiResult<T> = Result<T, ApiError>;

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
struct ApiJson<T>(T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
struct ApiQuery<T>(T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
struct ApiPath<T>(T);

pub fn router(state: Arc<AppState>) -> Router {
    let max_body = state.harness.config.api.max_body_bytes;
    let authed = Router::new()
        .route("/v1/me", get(me))
        .route("/v1/metrics", get(metrics))
        .route("/v1/health/details", get(health_details))
        .route("/v1/workflows", post(start_workflow).get(list_workflows))
        .route("/v1/workflows/{id}", get(get_workflow))
        .route("/v1/workflows/{id}/cancel", post(cancel_workflow))
        .route("/v1/workflow-definitions", get(list_definitions))
        .route("/v1/approvals", get(list_approvals))
        .route("/v1/approvals/{id}", get(get_approval))
        .route("/v1/approvals/{id}/decision", post(decide_approval))
        .route("/v1/artifacts/{id}", get(get_artifact))
        .route("/v1/agents", get(list_agents))
        .route("/v1/tools", get(list_tools))
        .route("/v1/audit", get(list_audit))
        .route("/v1/audit/verify", get(verify_audit))
        .route("/v1/tokens", post(issue_token).get(list_tokens))
        .route("/v1/tokens/{id}/revoke", post(revoke_token))
        .route("/v1/tokens/{id}/rotate", post(rotate_token))
        .route("/v1/principals", post(create_principal))
        .route("/v1/projects", post(create_project).get(list_projects))
        .route("/v1/plugins", get(list_plugins))
        .route("/v1/plugins/{id}/{version}/{action}", post(plugin_action))
        // Phase 2: knowledge & skills
        .route("/v1/knowledge/sources", post(create_source).get(list_sources))
        .route(
            "/v1/knowledge/documents",
            post(ingest_document).get(list_documents),
        )
        .route(
            "/v1/knowledge/documents/{id}",
            get(get_document).delete(delete_document),
        )
        .route("/v1/knowledge/documents/{id}/retire", post(retire_document))
        .route("/v1/knowledge/search", post(search_knowledge))
        .route("/v1/knowledge/conflicts", get(list_conflicts))
        // Phase 3: traceability recorded from approved artifacts
        .route("/v1/traceability", get(traceability))
        .route("/v1/skills", post(register_skill).get(list_skills))
        .route("/v1/skills/{id}/{version}/{action}", post(skill_action))
        .route("/v1/projects/{id}/domain", post(set_project_domain))
        .route("/v1/principals/{id}/teams", post(set_principal_teams))
        .layer(middleware::from_fn_with_state(state.clone(), authenticate));
    Router::new()
        .route("/v1/health", get(health))
        .merge(authed)
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "not_found", "no such route") })
        .layer(middleware::from_fn(request_id))
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(state)
}

async fn request_id(req: Request, next: Next) -> Response {
    let id = uuid::Uuid::now_v7().to_string();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let start = Instant::now();
    let mut res = next.run(req).await;
    if let Ok(v) = HeaderValue::from_str(&id) {
        res.headers_mut().insert("x-request-id", v);
    }
    tracing::info!(request_id = %id, %method, %path, status = res.status().as_u16(), ms = start.elapsed().as_millis() as u64, "api request");
    res
}

async fn authenticate(State(st): St, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .map(String::from);
    let Some(token) = token else {
        return ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "bearer token required",
        )
        .into_response();
    };
    let principal = match st.harness.identity.authenticate(&token).await {
        Ok(p) => p,
        Err(e) => {
            tracing::info!(path = %req.uri().path(), "authentication failed");
            return ApiError::from(e).into_response();
        }
    };
    if let Err(e) = rate_limit(&st, &principal) {
        return e.into_response();
    }
    req.extensions_mut().insert(principal);
    next.run(req).await
}

fn rate_limit(st: &AppState, p: &Principal) -> ApiResult<()> {
    let per_min = st.harness.config.api.requests_per_minute as usize;
    let mut m = st
        .limiter
        .lock()
        .map_err(|_| ApiError::from(HarnessError::internal("limiter poisoned")))?;
    let q = m.entry(p.id.to_string()).or_default();
    let now = Instant::now();
    while q
        .front()
        .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
    {
        q.pop_front();
    }
    if q.len() >= per_min {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "request rate limit exceeded",
        ));
    }
    q.push_back(now);
    Ok(())
}

/// Authorize a read/listing at the API boundary and audit denials.
async fn authorize(st: &AppState, p: &Principal, action: Action, target: Target) -> ApiResult<()> {
    if let Err(e) = st.harness.authz.authorize(p, action, target) {
        let _ = st
            .harness
            .db
            .audit(
                AuditEntry::new(p, action.as_str(), "api", "authorization", AuditOutcome::Denied)
                    .details(json!({"reason": e.message})),
            )
            .await;
        return Err(e.into());
    }
    Ok(())
}

#[derive(Deserialize)]
struct Page {
    limit: Option<u32>,
    cursor: Option<String>,
    project_id: Option<ProjectId>,
    status: Option<String>,
}

impl Page {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(50).clamp(1, 100)
    }
    fn scope(&self, p: &Principal) -> ApiResult<Scope> {
        let project = self.project_id.ok_or_else(|| {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_input", "project_id is required")
        })?;
        Ok(Scope::new(p.tenant, project))
    }
}

fn page<T: serde::Serialize>(items: Vec<T>, limit: u32, cursor: impl Fn(&T) -> String) -> Value {
    let next = if items.len() as u32 == limit {
        items.last().map(&cursor)
    } else {
        None
    };
    json!({"items": items, "next_cursor": next})
}

// ---------------------------------------------------------------- handlers

async fn health(State(st): St) -> impl IntoResponse {
    let h = st.harness.health().await;
    let code = if h.status == "ok" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(json!({"status": h.status, "api_version": "v1"})))
}

async fn health_details(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::MetricsRead, Target::Tenant(p.tenant)).await?;
    Ok(Json(
        serde_json::to_value(st.harness.health().await).map_err(HarnessError::from)?,
    ))
}

async fn metrics(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Response> {
    authorize(&st, &p, Action::MetricsRead, Target::Tenant(p.tenant)).await?;
    let h = st.harness.health().await;
    let body = st
        .harness
        .metrics
        .render_prometheus(&[("eaeh_outbox_pending", h.pending_events)]);
    Ok(([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body).into_response())
}

async fn me(Extension(p): Extension<Principal>) -> Json<Value> {
    Json(
        json!({"id": p.id, "kind": p.kind_str(), "tenant": p.tenant, "roles": p.roles(), "projects": p.projects}),
    )
}

#[derive(Deserialize)]
struct StartWorkflow {
    definition: WorkflowDefinitionId,
    project_id: ProjectId,
    #[serde(default)]
    input: Value,
}

async fn start_workflow(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiJson(b): ApiJson<StartWorkflow>,
) -> ApiResult<Response> {
    let scope = Scope::new(p.tenant, b.project_id);
    let id = st
        .harness
        .start_workflow(&p, &b.definition, scope, b.input)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"id": id, "status": "running"}))).into_response())
}

async fn list_workflows(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<Page>,
) -> ApiResult<Json<Value>> {
    let scope = q.scope(&p)?;
    let before = q
        .cursor
        .as_deref()
        .map(str::parse::<WorkflowInstanceId>)
        .transpose()?;
    let items = st.harness.engine.list(&p, scope, before, q.limit()).await?;
    Ok(Json(page(items, q.limit(), |i| i.id.to_string())))
}

async fn get_workflow(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<WorkflowInstanceId>,
) -> ApiResult<Json<Value>> {
    let inst = st.harness.engine.get_for(&p, id).await?;
    let history = st.harness.engine.history(id).await?;
    Ok(Json(json!({"instance": inst, "history": history})))
}

async fn cancel_workflow(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<WorkflowInstanceId>,
) -> ApiResult<Json<Value>> {
    st.harness.engine.get_for(&p, id).await?;
    let status = st.harness.engine.cancel(&p, id).await?;
    Ok(Json(json!({"id": id, "status": status})))
}

async fn list_definitions(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::WorkflowRead, Target::Tenant(p.tenant)).await?;
    Ok(Json(json!({"items": st.harness.engine.definitions().await?})))
}

async fn list_approvals(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<Page>,
) -> ApiResult<Json<Value>> {
    let scope = q.scope(&p)?;
    authorize(&st, &p, Action::ApprovalRead, Target::Scope(scope)).await?;
    let status = match q.status.as_deref() {
        None => None,
        Some(s) => Some(
            serde_json::from_value::<ApprovalStatus>(json!(s))
                .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid_input", "invalid status"))?,
        ),
    };
    let before = q.cursor.as_deref().map(str::parse::<ApprovalId>).transpose()?;
    let items = st
        .harness
        .approvals
        .list(scope, status, before, q.limit())
        .await?;
    Ok(Json(page(items, q.limit(), |a| a.id.to_string())))
}

async fn get_approval(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<ApprovalId>,
) -> ApiResult<Json<Value>> {
    let rec = st.harness.approvals.get(id).await?;
    st.harness
        .authz
        .authorize(&p, Action::ApprovalRead, Target::Scope(rec.scope))
        .map_err(|_| ApiError::from(HarnessError::not_found("approval not found")))?;
    Ok(Json(serde_json::to_value(rec).map_err(HarnessError::from)?))
}

#[derive(Deserialize)]
struct Decide {
    decision: ApprovalDecision,
    #[serde(default)]
    comments: String,
    artifact_hash: Option<String>,
}

async fn decide_approval(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<ApprovalId>,
    ApiJson(b): ApiJson<Decide>,
) -> ApiResult<Json<Value>> {
    let rec = st
        .harness
        .decide_approval(&p, id, b.decision, &b.comments, b.artifact_hash.as_deref())
        .await?;
    Ok(Json(serde_json::to_value(rec).map_err(HarnessError::from)?))
}

async fn get_artifact(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<ArtifactId>,
    ApiQuery(q): ApiQuery<Page>,
) -> ApiResult<Json<Value>> {
    let scope = q.scope(&p)?;
    authorize(&st, &p, Action::ArtifactRead, Target::Scope(scope)).await?;
    let (meta, bytes) = st.harness.artifacts.read(scope, id).await?;
    let content = String::from_utf8(bytes).ok();
    Ok(Json(json!({"artifact": meta, "content": content})))
}

async fn list_agents(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::AgentRead, Target::Tenant(p.tenant)).await?;
    let items: Vec<Value> = st
        .harness
        .agent_registry
        .list()
        .await?
        .into_iter()
        .map(|(d, s)| json!({"definition": d, "status": s}))
        .collect();
    Ok(Json(json!({"items": items})))
}

async fn list_tools(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::ToolRead, Target::Tenant(p.tenant)).await?;
    Ok(Json(json!({"items": st.harness.tools.list()})))
}

#[derive(Deserialize)]
struct AuditQuery {
    after_seq: Option<i64>,
    limit: Option<u32>,
}

async fn list_audit(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<AuditQuery>,
) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::AuditRead, Target::Tenant(p.tenant)).await?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let items = st
        .harness
        .db
        .audit_records(Some(p.tenant), q.after_seq.unwrap_or(0), limit)
        .await?;
    let next = if items.len() as u32 == limit {
        items.last().map(|r| r.seq.to_string())
    } else {
        None
    };
    Ok(Json(json!({"items": items, "next_cursor": next})))
}

async fn verify_audit(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::AuditRead, Target::Tenant(p.tenant)).await?;
    Ok(Json(
        serde_json::to_value(st.harness.db.verify_audit_chain().await?).map_err(HarnessError::from)?,
    ))
}

#[derive(Deserialize)]
struct IssueToken {
    principal_id: Option<PrincipalId>,
    label: String,
    ttl_days: Option<i64>,
}

async fn issue_token(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiJson(b): ApiJson<IssueToken>,
) -> ApiResult<Response> {
    let target = b.principal_id.unwrap_or_else(|| p.id.clone());
    let t = st
        .harness
        .identity
        .issue_token(&p, &target, &b.label, b.ttl_days.unwrap_or(30))
        .await?;
    let mut res = (
        StatusCode::CREATED,
        Json(json!({"id": t.id, "token": t.token.expose(), "expires_at": t.expires_at, "note": "store this token now; it cannot be retrieved again"})),
    )
        .into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

#[derive(Deserialize)]
struct TokenQuery {
    principal_id: Option<PrincipalId>,
}

async fn list_tokens(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<TokenQuery>,
) -> ApiResult<Json<Value>> {
    let target = q.principal_id.unwrap_or_else(|| p.id.clone());
    Ok(Json(
        json!({"items": st.harness.identity.list_tokens(&p, &target).await?}),
    ))
}

async fn revoke_token(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<TokenId>,
) -> ApiResult<StatusCode> {
    st.harness.identity.revoke_token(&p, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Rotate {
    ttl_days: Option<i64>,
}

async fn rotate_token(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<TokenId>,
    ApiQuery(q): ApiQuery<Rotate>,
) -> ApiResult<Response> {
    let ttl = q.ttl_days.unwrap_or(30);
    let t = st.harness.identity.rotate_token(&p, id, ttl).await?;
    let mut res = (
        StatusCode::CREATED,
        Json(json!({"id": t.id, "token": t.token.expose(), "expires_at": t.expires_at, "replaces": id})),
    )
        .into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

#[derive(Deserialize)]
struct CreatePrincipal {
    id: PrincipalId,
    display_name: String,
    #[serde(default = "human")]
    kind: String,
    roles: Vec<Role>,
    /// Project ids, or omitted for all projects in the tenant.
    projects: Option<Vec<ProjectId>>,
}

fn human() -> String {
    "human".into()
}

async fn create_principal(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiJson(b): ApiJson<CreatePrincipal>,
) -> ApiResult<Response> {
    let kind = match b.kind.as_str() {
        "human" => PrincipalKind::Human,
        "service" => PrincipalKind::Service,
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "kind must be human or service",
            ));
        }
    };
    let created = st
        .harness
        .identity
        .create_principal(
            &p,
            NewPrincipal {
                id: b.id,
                display_name: b.display_name,
                kind,
                roles: b.roles.into_iter().collect(),
                projects: match b.projects {
                    Some(ps) => ProjectAccess::Only(ps.into_iter().collect()),
                    None => ProjectAccess::All,
                },
            },
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": created.id, "roles": created.roles()})),
    )
        .into_response())
}

#[derive(Deserialize)]
struct CreateProject {
    name: String,
}

async fn create_project(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiJson(b): ApiJson<CreateProject>,
) -> ApiResult<Response> {
    let id = st.harness.identity.create_project(&p, p.tenant, &b.name).await?;
    Ok((StatusCode::CREATED, Json(json!({"id": id, "name": b.name}))).into_response())
}

async fn list_projects(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let all = st.harness.identity.projects(p.tenant).await?;
    let visible: Vec<Value> = all
        .into_iter()
        .filter(|(id, _)| p.can_access(&Scope::new(p.tenant, *id)))
        .map(|(id, name)| json!({"id": id, "name": name}))
        .collect();
    Ok(Json(json!({"items": visible})))
}

async fn list_plugins(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::PluginManage, Target::Platform).await?;
    Ok(Json(json!({"items": st.harness.plugins.list().await?})))
}

async fn plugin_action(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath((id, version, action)): ApiPath<(eaeh_core::PluginId, String, String)>,
) -> ApiResult<Json<Value>> {
    let v = semver::Version::parse(&version)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_input", e.to_string()))?;
    let rec = match action.as_str() {
        "enable" => st.harness.plugins.enable(&p, &id, &v).await?,
        "disable" => st.harness.plugins.disable(&p, &id, &v).await?,
        "retire" => st.harness.plugins.retire(&p, &id, &v).await?,
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "action must be enable, disable or retire",
            ));
        }
    };
    Ok(Json(
        json!({"plugin": rec, "note": "takes effect on next harness start"}),
    ))
}

// ---------------------------------------------------------------- Phase 2: knowledge & skills

#[derive(Deserialize)]
struct CreateSource {
    name: String,
    kind: eaeh_knowledge::SourceKind,
    description: String,
}

async fn create_source(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiJson(b): ApiJson<CreateSource>,
) -> ApiResult<Response> {
    let src = st
        .harness
        .knowledge
        .register_source(&p, &b.name, b.kind, &b.description)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(src).map_err(HarnessError::from)?),
    )
        .into_response())
}

async fn list_sources(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({"items": st.harness.knowledge.sources(&p).await?})))
}

async fn ingest_document(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiJson(b): ApiJson<eaeh_knowledge::IngestRequest>,
) -> ApiResult<Response> {
    let doc = st
        .harness
        .knowledge
        .ingest(&p, b, eaeh_core::TraceContext::new_root())
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(doc).map_err(HarnessError::from)?),
    )
        .into_response())
}

async fn list_documents(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<Page>,
) -> ApiResult<Json<Value>> {
    let status = q
        .status
        .as_deref()
        .map(eaeh_knowledge::DocStatus::parse)
        .transpose()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid_input", "invalid status"))?;
    Ok(Json(
        json!({"items": st.harness.knowledge.list(&p, status, q.limit()).await?}),
    ))
}

async fn get_document(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(st.harness.knowledge.get(&p, &id).await?).map_err(HarnessError::from)?,
    ))
}

async fn retire_document(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(st.harness.knowledge.retire(&p, &id).await?).map_err(HarnessError::from)?,
    ))
}

async fn delete_document(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(st.harness.knowledge.delete(&p, &id).await?).map_err(HarnessError::from)?,
    ))
}

#[derive(Deserialize)]
struct SearchBody {
    project_id: ProjectId,
    #[serde(flatten)]
    query: eaeh_knowledge::SearchQuery,
}

async fn search_knowledge(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiJson(b): ApiJson<SearchBody>,
) -> ApiResult<Json<Value>> {
    let ctx = st.harness.knowledge.context_for_human(&p, b.project_id).await?;
    let hits = st
        .harness
        .knowledge
        .search(&ctx, &b.query, eaeh_core::TraceContext::new_root())
        .await?;
    Ok(Json(json!({"items": hits})))
}

#[derive(Deserialize)]
struct TraceQuery {
    project_id: ProjectId,
    id: Option<String>,
    depth: Option<u32>,
}

async fn traceability(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<TraceQuery>,
) -> ApiResult<Json<Value>> {
    let scope = Scope::new(p.tenant, q.project_id);
    let items = st
        .harness
        .engine
        .trace(&p, scope, q.id.as_deref(), q.depth.unwrap_or(3))
        .await?;
    Ok(Json(json!({"items": items})))
}

async fn list_conflicts(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({"items": st.harness.knowledge.conflicts(&p).await?})))
}

#[derive(Deserialize)]
struct RegisterSkill {
    project_id: ProjectId,
    definition: eaeh_skills::SkillDefinition,
}

async fn register_skill(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiJson(b): ApiJson<RegisterSkill>,
) -> ApiResult<Response> {
    let rec = st
        .harness
        .skills
        .register(
            &p,
            &b.definition,
            b.project_id,
            eaeh_core::TraceContext::new_root(),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(rec).map_err(HarnessError::from)?),
    )
        .into_response())
}

async fn list_skills(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({"items": st.harness.skills.list(&p).await?})))
}

async fn skill_action(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath((id, version, action)): ApiPath<(eaeh_core::SkillId, String, String)>,
) -> ApiResult<Json<Value>> {
    let v = semver::Version::parse(&version)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_input", e.to_string()))?;
    let to = match action.as_str() {
        "deprecate" => eaeh_skills::SkillStatus::Deprecated,
        "retire" => eaeh_skills::SkillStatus::Retired,
        "reinstate" => eaeh_skills::SkillStatus::Approved,
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "action must be deprecate, retire or reinstate",
            ));
        }
    };
    Ok(Json(
        serde_json::to_value(st.harness.skills.set_status(&p, &id, &v, to).await?)
            .map_err(HarnessError::from)?,
    ))
}

#[derive(Deserialize)]
struct DomainBody {
    domain: Option<String>,
}

async fn set_project_domain(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<ProjectId>,
    ApiJson(b): ApiJson<DomainBody>,
) -> ApiResult<StatusCode> {
    st.harness
        .identity
        .set_project_domain(&p, id, b.domain.as_deref())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TeamsBody {
    teams: Vec<String>,
}

async fn set_principal_teams(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<PrincipalId>,
    ApiJson(b): ApiJson<TeamsBody>,
) -> ApiResult<StatusCode> {
    st.harness.identity.set_principal_teams(&p, &id, &b.teams).await?;
    Ok(StatusCode::NO_CONTENT)
}
