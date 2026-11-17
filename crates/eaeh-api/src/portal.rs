//! Governance Portal server core (ADR-0026), used by `eaeh portal`.
//!
//! * Serves the built web UI and mounts the unchanged `/v1` router under `/api`, so every
//!   portal request passes the same authentication, authorization, rate limiting, approval
//!   manager and audit as any API client.
//! * No login: requests carry the bearer token of an identity the developer stored locally
//!   (resolved by the CLI). Tokens stay in the server process and never reach the browser.
//! * Reads as the viewer; approval decisions only as the explicitly configured approver
//!   (read-only without one).
//! * Only an allowlist of read endpoints plus `POST /v1/approvals/{id}/decision`.
//! * Local-only guard: `Host` check (DNS rebinding), `Origin` check on writes (CSRF) and a
//!   per-launch secret exchanged for an `HttpOnly; SameSite=Strict` cookie, so another
//!   local process cannot use the stored approver identity through the portal.
//!
//! A trusted local-development tool, not a multi-user or production server.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Extension, Json, Router};
use eaeh_core::Principal;
use eaeh_runtime::Harness;
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const COOKIE: &str = "eaeh_portal";

/// An identity the portal acts as. The token never leaves this process.
#[derive(Clone)]
pub struct Acting {
    pub label: String,
    token: String,
    pub principal: Principal,
}

impl std::fmt::Debug for Acting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Acting")
            .field("label", &self.label)
            .field("principal", &self.principal.id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct PortalState {
    pub viewer: Acting,
    pub approver: Option<Acting>,
    /// Identity used only to register skill files (`--steward`, `skill.manage`).
    pub steward: Option<Acting>,
    /// Identity for workflow operations: start, cancel, resume, restart, retry (`--operator`).
    pub operator: Option<Acting>,
    /// Identity for registering existing projects (`--admin`, `principal.manage`).
    pub admin: Option<Acting>,
    /// Canonical folders under which *Register existing project* may browse (`--workspace-root`).
    pub workspace_roots: Vec<PathBuf>,
    /// The CLI-side services the portal process calls (project registration, workers).
    pub host: Option<Arc<dyn PortalHost>>,
    secret: String,
    allowed_hosts: Vec<String>,
    allowed_origins: Vec<String>,
    static_dir: Option<PathBuf>,
    execution_mode: String,
}

impl PortalState {
    /// Same state with a steward identity for skill registration (`--steward`).
    pub fn with_steward(self: Arc<Self>, steward: Option<Acting>) -> Arc<Self> {
        let mut s = Arc::try_unwrap(self).unwrap_or_else(|a| (*a).clone());
        s.steward = steward;
        Arc::new(s)
    }

    /// Same state with the operation identities, workspace roots and host services
    /// (PORTAL-OPERATIONS-PLAN W2–W4).
    pub fn with_operations(
        self: Arc<Self>,
        operator: Option<Acting>,
        admin: Option<Acting>,
        workspace_roots: Vec<PathBuf>,
        host: Option<Arc<dyn PortalHost>>,
    ) -> Arc<Self> {
        let mut s = Arc::try_unwrap(self).unwrap_or_else(|a| (*a).clone());
        s.operator = operator;
        s.admin = admin;
        s.workspace_roots = eaeh_runtime::onboarding::canonical_roots(&workspace_roots);
        s.host = host;
        Arc::new(s)
    }

    pub fn new(
        viewer: Acting,
        approver: Option<Acting>,
        secret: String,
        addr: SocketAddr,
        dev_origins: Vec<String>,
        static_dir: Option<PathBuf>,
        execution_mode: String,
    ) -> Arc<Self> {
        let port = addr.port();
        let hosts = vec![
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("[::1]:{port}"),
        ];
        let mut origins: Vec<String> = hosts.iter().map(|h| format!("http://{h}")).collect();
        origins.extend(
            dev_origins
                .into_iter()
                .map(|o| o.trim_end_matches('/').to_string()),
        );
        Arc::new(Self {
            viewer,
            approver,
            steward: None,
            operator: None,
            admin: None,
            workspace_roots: vec![],
            host: None,
            secret,
            allowed_hosts: hosts,
            allowed_origins: origins,
            static_dir,
            execution_mode,
        })
    }
}

impl Acting {
    pub fn new(label: impl Into<String>, token: impl Into<String>, principal: Principal) -> Self {
        Self {
            label: label.into(),
            token: token.into(),
            principal,
        }
    }
}

/// A boxed future (the host trait is object-safe).
pub type HostFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = eaeh_core::Result<T>> + Send + 'a>>;

/// Services of the `eaeh` CLI that the portal process calls, so the portal and the CLI run
/// the same code (W4): registering a project in place (`eaeh project init`) and starting the
/// project's worker for external AI execution (`eaeh workflow run`).
pub trait PortalHost: Send + Sync {
    /// Register the confined, canonical `root` as a project, acting as `admin`.
    fn register<'a>(
        &'a self,
        h: &'a Harness,
        admin: &'a Principal,
        root: PathBuf,
        req: RegisterRequest,
    ) -> HostFuture<'a, Value>;
    /// Start the project's worker when external AI work waits and none runs.
    fn ensure_worker<'a>(
        &'a self,
        h: &'a Harness,
        operator: &'a Principal,
        project: eaeh_core::ProjectId,
    ) -> HostFuture<'a, Option<String>>;
}

/// What the user confirmed in *Register existing project*. Document paths are relative to
/// the project folder and are confined to it.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct RegisterRequest {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub framework: String,
    #[serde(default)]
    pub build_tool: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub brd: Option<String>,
    #[serde(default)]
    pub knowledge: Vec<String>,
    #[serde(default)]
    pub standards: Vec<String>,
    pub developer: String,
    pub steward: String,
    pub approvers: Vec<String>,
    /// Re-register a folder that already carries this installation's manifest.
    #[serde(default)]
    pub update: bool,
}

/// The portal application (also used by tests).
/// Which EAEH installation the portal shows (several can exist on one computer, e.g. a
/// demo under `target/` and the real one), so the UI can say so explicitly.
#[derive(Clone)]
struct Installation(Value);

fn installation(h: &Harness) -> Installation {
    let canon = |p: &Path| {
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .display()
            .to_string()
    };
    // Optional label set by the launcher (e.g. `DEMO` from scripts/demo-portal.sh).
    let label = std::env::var("EAEH_INSTALLATION_LABEL")
        .ok()
        .map(|l| l.trim().chars().take(40).collect::<String>())
        .filter(|l| !l.is_empty());
    Installation(json!({
        "data_dir": canon(&h.config.data_dir()),
        "config_dir": canon(&h.config.base_dir),
        "label": label,
    }))
}

pub fn app(h: Arc<Harness>, state: Arc<PortalState>) -> Router {
    let inst = installation(&h);
    let h_for_portal = h.clone();
    let api = crate::router(crate::AppState::new(h.clone())).layer(middleware::from_fn_with_state(
        (state.clone(), h),
        inject_identity,
    ));
    Router::new()
        .route("/portal/session", post(open_session).get(session_info))
        .route("/portal/fs", axum::routing::get(browse))
        .route("/portal/projects/inspect", post(inspect))
        .route("/portal/projects/register", post(register))
        .route("/portal/projects/{id}/worker", post(ensure_worker))
        .with_state((state.clone(), h_for_portal))
        .nest("/api", api)
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(state.clone(), local_guard))
        .layer(Extension(state))
        .layer(Extension(inst))
}

fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"error": {"code": code, "message": message}}))).into_response()
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

fn has_session(st: &PortalState, headers: &HeaderMap) -> bool {
    session_cookie(headers).is_some_and(|c| ct_eq(&c, &st.secret))
}

/// Host (DNS rebinding), Origin (cross-site writes) and session checks, plus security headers.
async fn local_guard(State(st): State<Arc<PortalState>>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !st.allowed_hosts.iter().any(|h| h == host) {
        return err(
            StatusCode::FORBIDDEN,
            "host_not_allowed",
            "the portal only answers on its loopback address",
        );
    }
    let unsafe_method = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if unsafe_method {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if !st.allowed_origins.iter().any(|o| o == origin) {
            return err(
                StatusCode::FORBIDDEN,
                "origin_not_allowed",
                "cross-origin requests are refused",
            );
        }
    }
    let path = req.uri().path().to_string();
    let needs_session = path.starts_with("/api/")
        || (path.starts_with("/portal/") && path != "/portal/session")
        || (path == "/portal/session" && !unsafe_method);
    if needs_session && !has_session(&st, req.headers()) {
        return err(
            StatusCode::UNAUTHORIZED,
            "portal_session_required",
            "open the portal with the link printed by `eaeh portal` (it carries the launch secret)",
        );
    }
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    );
    if path.starts_with("/api/") || path.starts_with("/portal/") {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

#[derive(Deserialize)]
struct OpenSession {
    launch: String,
}

async fn open_session(
    State((st, _)): State<(Arc<PortalState>, Arc<Harness>)>,
    Json(b): Json<OpenSession>,
) -> Response {
    if !ct_eq(b.launch.trim(), &st.secret) {
        return err(
            StatusCode::UNAUTHORIZED,
            "invalid_launch_secret",
            "the launch secret is not valid for this portal process",
        );
    }
    let mut res = Json(json!({"ok": true})).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!(
        "{COOKIE}={}; HttpOnly; SameSite=Strict; Path=/",
        st.secret
    )) {
        res.headers_mut().insert(header::SET_COOKIE, v);
    }
    res
}

fn acting_json(a: &Acting) -> Value {
    json!({"profile": a.label, "principal": a.principal.id, "kind": a.principal.kind_str(), "roles": a.principal.roles(), "projects": a.principal.projects})
}

async fn session_info(
    State((st, _)): State<(Arc<PortalState>, Arc<Harness>)>,
    Extension(inst): Extension<Installation>,
) -> Json<Value> {
    Json(json!({
        "installation": inst.0,
        "viewer": acting_json(&st.viewer),
        "approver": st.approver.as_ref().map(acting_json),
        "steward": st.steward.as_ref().map(acting_json),
        "can_decide": st.approver.is_some(),
        "can_register_skills": st.steward.is_some(),
        "operator": st.operator.as_ref().map(acting_json),
        "admin": st.admin.as_ref().map(acting_json),
        "can_operate": st.operator.is_some(),
        "can_register_projects": st.admin.is_some() && st.host.is_some() && !st.workspace_roots.is_empty(),
        "workspace_roots": st.workspace_roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>(),
        "execution_mode": st.execution_mode,
        "api_version": "v1",
        "local_only": true,
        "authentication": "none (trusted local development only)",
    }))
}

/// Which identity a portal request is sent as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Reads, as the viewer.
    Read,
    /// Reads that depend on the operator's permissions (execution plan and readiness):
    /// the operator when configured, else the viewer.
    OperatorRead,
    /// Approval decisions (`--approver`).
    Decision,
    /// Skill file registration (`--steward`).
    SkillRegister,
    /// Skill directory scan (registers as `system:skill-discovery`): the viewer.
    SkillScan,
    /// Workflow and task operations (`--operator`).
    Operate,
}

/// What the portal may call, and as whom. Everything else (tokens, principals, plugins,
/// knowledge ingestion and deletion, standards decisions) stays with the CLI and the
/// authenticated API.
fn classify(method: &Method, path: &str) -> Option<Class> {
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.clone(), segs.as_slice()) {
        (Method::POST, ["v1", "approvals", _, "decision"]) => Some(Class::Decision),
        (Method::POST, ["v1", "skills", "sync"]) => Some(Class::SkillRegister),
        (Method::POST, ["v1", "skills", "discover"]) => Some(Class::SkillScan),
        (
            Method::POST,
            ["v1", "workflows"]
            | ["v1", "workflows", _, "cancel" | "resume" | "restart"]
            | ["v1", "workflows", _, "restart", "preview"]
            | ["v1", "tasks", _, "retry"],
        ) => Some(Class::Operate),
        (Method::GET | Method::HEAD, ["v1", "projects", _, "plan"]) => Some(Class::OperatorRead),
        (Method::GET | Method::HEAD, s) => matches!(
            s,
            ["v1", "health"]
                | ["v1", "health", "details"]
                | ["v1", "me"]
                | ["v1", "dashboard"]
                | ["v1", "activity"]
                | ["v1", "approval-queue"]
                | ["v1", "workflow-queue"]
                | ["v1", "task-queue"]
                | ["v1", "workflows"]
                | ["v1", "workflows", _]
                | ["v1", "workflows", _, "detail"]
                | ["v1", "workflows", _, "lineage"]
                | ["v1", "workflow-definitions"]
                | ["v1", "tasks", _]
                | ["v1", "executions"]
                | ["v1", "integrations"]
                | ["v1", "approvals"]
                | ["v1", "approvals", _]
                | ["v1", "approvals", _, "document"]
                | ["v1", "artifacts", _]
                | ["v1", "projects"]
                | ["v1", "projects", _, "overview"]
                | ["v1", "audit"]
                | ["v1", "audit", "verify"]
                | ["v1", "traceability"]
                | ["v1", "agents"]
                | ["v1", "tools"]
                | ["v1", "knowledge", "sources"]
                | ["v1", "knowledge", "documents"]
                | ["v1", "knowledge", "documents", _]
                | ["v1", "knowledge", "conflicts"]
                | ["v1", "skills"]
                | ["v1", "skills", "catalog"]
                | ["v1", "skills", _, _, "files"]
        )
        .then_some(Class::Read),
        _ => None,
    }
}

fn missing(code: &str, message: &str) -> Response {
    err(StatusCode::FORBIDDEN, code, message)
}

async fn inject_identity(
    State((st, h)): State<(Arc<PortalState>, Arc<Harness>)>,
    mut req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let path = path.strip_prefix("/api").unwrap_or(&path).to_string();
    let Some(class) = classify(req.method(), &path) else {
        return err(
            StatusCode::FORBIDDEN,
            "not_available_in_portal",
            "this operation is not available through the portal; use the eaeh CLI (see docs/implementation/EAEH-PORTAL-CLI-PARITY.md)",
        );
    };
    let acting = match class {
        Class::Read | Class::SkillScan => &st.viewer,
        Class::OperatorRead => st.operator.as_ref().unwrap_or(&st.viewer),
        // Skill registration is the steward's (skill.manage), never the approver's: the
        // registrant of a skill can never approve it (separation of duties).
        Class::SkillRegister => match &st.steward {
            Some(s) => s,
            None => {
                return missing(
                    "steward_not_configured",
                    "registering skills needs a steward identity: restart the portal with `--steward <profile>`",
                );
            }
        },
        Class::Decision => match &st.approver {
            Some(a) => a,
            None => {
                return missing(
                    "approver_not_configured",
                    "this portal is read-only: restart it with `--approver <profile>` to record decisions",
                );
            }
        },
        Class::Operate => match &st.operator {
            Some(o) => o,
            None => {
                return missing(
                    "operator_not_configured",
                    "workflow operations need an operator identity: restart the portal with `--operator <profile>` (a developer/operator profile)",
                );
            }
        },
    };
    // Portal decisions: workflow document gates and skills (ADR-0028, amending ADR-0026 §4;
    // the review page shows a skill's full definition and digest). Mandatory standards are
    // still reviewed with `eaeh approval review`.
    if class == Class::Decision {
        let id = path
            .split('/')
            .nth(3)
            .and_then(|s| s.parse::<eaeh_core::ApprovalId>().ok());
        if let Some(id) = id
            && let Ok(rec) = h.approvals.get(id).await
            && rec.workflow_instance_id.is_none()
            && rec.subject_type != "skill"
        {
            return err(
                StatusCode::FORBIDDEN,
                "not_available_in_portal",
                "the portal decides workflow documents and skills; review mandatory standards with `eaeh approval review`",
            );
        }
    }
    // Never forward a client-supplied credential.
    req.headers_mut().remove(header::AUTHORIZATION);
    match HeaderValue::from_str(&format!("Bearer {}", acting.token)) {
        Ok(v) => {
            req.headers_mut().insert(header::AUTHORIZATION, v);
        }
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "invalid stored credential",
            );
        }
    }
    match class {
        Class::Decision => {
            tracing::info!(path = %path, approver = %acting.principal.id, "portal approval decision submitted")
        }
        Class::SkillRegister => {
            tracing::info!(steward = %acting.principal.id, "portal skill registration submitted")
        }
        Class::Operate => {
            tracing::info!(path = %path, operator = %acting.principal.id, "portal workflow operation submitted")
        }
        _ => {}
    }
    next.run(req).await
}

// ------------------------------------------------- register an existing project (W3, W4)

type PState = State<(Arc<PortalState>, Arc<Harness>)>;

fn harness_err(e: eaeh_core::HarnessError) -> Response {
    crate::ApiError::from(e).into_response()
}

#[derive(Deserialize)]
struct BrowseQuery {
    path: Option<String>,
}

/// Sub-folders of a folder under the allowed workspace roots (no files, no hidden entries).
async fn browse(
    State((st, _)): PState,
    axum::extract::Query(q): axum::extract::Query<BrowseQuery>,
) -> Response {
    match eaeh_runtime::onboarding::list_dir(&st.workspace_roots, q.path.as_deref()) {
        Ok(l) => Json(l).into_response(),
        Err(e) => harness_err(e),
    }
}

#[derive(Deserialize)]
struct InspectBody {
    path: String,
}

/// Read-only detection of a folder, and whether it is already registered here.
async fn inspect(State((st, h)): PState, Json(b): Json<InspectBody>) -> Response {
    let root = match eaeh_runtime::onboarding::confine(&st.workspace_roots, &b.path) {
        Ok(r) => r,
        Err(e) => return harness_err(e),
    };
    let d = eaeh_runtime::onboarding::detect(&root);
    let reg = registration_state(&st, &h, &d).await;
    Json(json!({"detection": d, "registration": reg})).into_response()
}

/// `registered` (the folder's manifest names a project of this installation), `foreign`
/// (a manifest of another installation), `name_taken` (another project has this name), or
/// `new`.
async fn registration_state(st: &PortalState, h: &Harness, d: &eaeh_runtime::onboarding::Detection) -> Value {
    let tenant = st.viewer.principal.tenant;
    let projects = h.identity.projects(tenant).await.unwrap_or_default();
    if let Some(m) = &d.manifest {
        if let Some((id, name)) = projects.iter().find(|(id, _)| id.to_string() == m.project_id) {
            return json!({"state": "registered", "project_id": id, "name": name});
        }
        return json!({"state": "foreign", "project_id": m.project_id, "name": m.name});
    }
    if let Some((id, _)) = projects.iter().find(|(_, n)| *n == d.suggested_name) {
        return json!({"state": "name_taken", "project_id": id, "name": d.suggested_name});
    }
    json!({"state": "new"})
}

/// A document path relative to the project folder that stays inside it.
fn confine_doc(root: &Path, f: &str) -> eaeh_core::Result<String> {
    let rel = Path::new(f.trim());
    if f.trim().is_empty()
        || rel.is_absolute()
        || rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(eaeh_core::HarnessError::invalid_input(format!(
            "'{f}': document paths are relative to the project folder and stay inside it"
        )));
    }
    let c = std::fs::canonicalize(root.join(rel)).map_err(|_| {
        eaeh_core::HarnessError::invalid_input(format!("document not found in the project folder: {f}"))
    })?;
    if !c.starts_with(root) || !c.is_file() {
        return Err(eaeh_core::HarnessError::invalid_input(format!(
            "'{f}' is not a file inside the project folder"
        )));
    }
    Ok(rel.display().to_string())
}

/// Register the confirmed folder through the shared `project init` service, as `--admin`.
async fn register(State((st, h)): PState, Json(mut b): Json<RegisterRequest>) -> Response {
    let Some(admin) = &st.admin else {
        return missing(
            "admin_not_configured",
            "registering projects needs an administrator identity: restart the portal with `--admin <profile>`",
        );
    };
    let Some(host) = &st.host else {
        return missing(
            "not_available_in_portal",
            "project registration is not available in this portal process",
        );
    };
    let root = match eaeh_runtime::onboarding::confine(&st.workspace_roots, &b.path) {
        Ok(r) => r,
        Err(e) => return harness_err(e),
    };
    let d = eaeh_runtime::onboarding::detect(&root);
    if !d.recognised {
        return harness_err(eaeh_core::HarnessError::invalid_input(format!(
            "{} is not a project folder: {}",
            root.display(),
            d.warnings.join("; ")
        )));
    }
    if let Some(i) = &d.manifest_issue {
        return harness_err(eaeh_core::HarnessError::invalid_input(i.clone()));
    }
    let name = b.name.trim().to_string();
    if name.is_empty() || name.len() > 100 {
        return harness_err(eaeh_core::HarnessError::invalid_input(
            "a project name (1–100 characters) is required",
        ));
    }
    b.name = name;
    let reg = registration_state(
        &st,
        &h,
        &eaeh_runtime::onboarding::Detection {
            suggested_name: b.name.clone(),
            ..d.clone()
        },
    )
    .await;
    match reg["state"].as_str() {
        Some("registered") if !b.update => {
            return harness_err(eaeh_core::HarnessError::conflict(format!(
                "this folder is already registered as project '{}' ({}); open it, or confirm an update of its registration",
                reg["name"].as_str().unwrap_or_default(),
                reg["project_id"].as_str().unwrap_or_default()
            )));
        }
        Some("foreign") => {
            return harness_err(eaeh_core::HarnessError::conflict(format!(
                "this folder carries the manifest of project {} from another EAEH installation; register it there, or remove .eaeh/project.toml first",
                reg["project_id"].as_str().unwrap_or_default()
            )));
        }
        Some("name_taken") => {
            return harness_err(eaeh_core::HarnessError::conflict(format!(
                "a project named '{}' already exists in this installation; choose another name",
                b.name
            )));
        }
        _ => {}
    }
    for f in b
        .brd
        .iter_mut()
        .chain(b.knowledge.iter_mut())
        .chain(b.standards.iter_mut())
    {
        match confine_doc(&root, f) {
            Ok(r) => *f = r,
            Err(e) => return harness_err(e),
        }
    }
    tracing::info!(admin = %admin.principal.id, root = %root.display(), "portal project registration submitted");
    match host.register(&h, &admin.principal, root, b).await {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => harness_err(e),
    }
}

/// Start the project's worker when external AI work waits (as `eaeh workflow run` does).
async fn ensure_worker(
    State((st, h)): PState,
    axum::extract::Path(id): axum::extract::Path<eaeh_core::ProjectId>,
) -> Response {
    let Some(op) = &st.operator else {
        return missing(
            "operator_not_configured",
            "workflow operations need an operator identity: restart the portal with `--operator <profile>`",
        );
    };
    let Some(host) = &st.host else {
        return missing(
            "not_available_in_portal",
            "workers cannot be started from this portal process",
        );
    };
    let scope = eaeh_core::Scope::new(op.principal.tenant, id);
    if !op.principal.can_access(&scope)
        || h.authz
            .authorize(
                &op.principal,
                eaeh_governance::authz::Action::WorkflowStart,
                eaeh_governance::authz::Target::Scope(scope),
            )
            .is_err()
    {
        return harness_err(eaeh_core::HarnessError::not_found("project not found"));
    }
    match host.ensure_worker(&h, &op.principal, id).await {
        Ok(msg) => Json(json!({"project": id, "worker": msg})).into_response(),
        Err(e) => harness_err(e),
    }
}

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Built UI files; unknown paths fall back to `index.html` (client-side routing).
async fn static_file(Extension(st): Extension<Arc<PortalState>>, req: Request) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return err(StatusCode::NOT_FOUND, "not_found", "no such route");
    }
    let Some(dir) = &st.static_dir else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "The portal web UI is not built. Run `npm --prefix portal ci && npm --prefix portal run build`, then restart `eaeh portal`.",
        )
            .into_response();
    };
    let rel = req.uri().path().trim_start_matches('/');
    let safe = Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    let mut file = dir.join(if rel.is_empty() || !safe {
        "index.html"
    } else {
        rel
    });
    if !file.is_file() {
        file = dir.join("index.html");
    }
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let cache = if rel.starts_with("assets/") && file.ends_with(rel) {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                [
                    (header::CONTENT_TYPE, content_type(&file)),
                    (header::CACHE_CONTROL, cache),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => err(StatusCode::NOT_FOUND, "not_found", "file not found"),
    }
}
