//! Read models for the Governance Portal (ADR-0026).
//!
//! These endpoints only *compose* existing governed services: every project is read through
//! the same authorization checks as the per-project endpoints (`workflow.read`,
//! `approval.read`, `artifact.read`), projects the caller may not read are reported as
//! `restricted` instead of failing the whole view, and nothing here writes state. Approval
//! decisions stay on `POST /v1/approvals/{id}/decision`.

use crate::error::ApiError;
use crate::routes::{ApiPath, ApiQuery, AppState, authorize};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Extension, Json, Router};
use eaeh_core::{ApprovalId, ArtifactId, HarnessError, Principal, ProjectId, Scope, WorkflowInstanceId};
use eaeh_governance::authz::{Action, Target};
use eaeh_governance::{ApprovalRecord, ApprovalStatus};
use eaeh_workflow::{StateKind, WorkflowDefinition, WorkflowInstance};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

type St = State<Arc<AppState>>;
type ApiResult<T> = Result<T, ApiError>;

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/dashboard", get(dashboard))
        .route("/v1/activity", get(activity))
        .route("/v1/approval-queue", get(approval_queue))
        .route("/v1/workflow-queue", get(workflow_queue))
        .route("/v1/workflows/{id}/detail", get(workflow_detail))
        .route("/v1/approvals/{id}/document", get(approval_document))
        .route("/v1/projects/{id}/overview", get(project_overview))
}

// ---------------------------------------------------------------- helpers

/// Projects of the caller's tenant that the caller's project access covers.
async fn visible_projects(
    st: &AppState,
    p: &Principal,
    only: Option<ProjectId>,
) -> ApiResult<Vec<(ProjectId, String)>> {
    let all = st.harness.identity.projects(p.tenant).await?;
    let out: Vec<_> = all
        .into_iter()
        .filter(|(id, _)| p.can_access(&Scope::new(p.tenant, *id)))
        .filter(|(id, _)| only.is_none_or(|o| o == *id))
        .collect();
    if only.is_some() && out.is_empty() {
        // Same answer for "missing" and "not yours": no existence oracle.
        return Err(ApiError::from(HarnessError::not_found("project not found")));
    }
    Ok(out)
}

/// Workspace and committed manifest of a project. Existence and access come from the
/// project registry; the workspace from its registration (ADR-0027); description, stack and
/// workflows are read from that repository's `.eaeh/project.toml` only when it declares this
/// project (no second registry, no copy).
async fn project_profile(
    st: &AppState,
    scope: Scope,
) -> (
    Option<eaeh_runtime::WorkspaceRegistration>,
    Option<eaeh_runtime::project::ProjectManifest>,
    Option<String>,
) {
    let ws = match st.harness.engine.materializer().workspace(scope).await {
        Ok(w) => w,
        Err(e) => return (None, None, Some(e.message)),
    };
    let Some(reg) = ws else {
        return (
            None,
            None,
            Some("no workspace registered yet: run `eaeh workflow run …` or `eaeh artifact sync` in the project repository".into()),
        );
    };
    match eaeh_runtime::project::ProjectManifest::load(std::path::Path::new(&reg.root)) {
        Ok(m) if m.project.id == scope.project => (Some(reg), Some(m), None),
        Ok(m) => {
            let issue = format!(
                "the manifest in {} belongs to project {} – re-run `eaeh project init` there",
                reg.root, m.project.id
            );
            (Some(reg), None, Some(issue))
        }
        Err(e) => (
            Some(reg),
            None,
            Some(format!("manifest not readable: {}", e.message)),
        ),
    }
}

fn manifest_view(m: &eaeh_runtime::project::ProjectManifest) -> Value {
    json!({
        "description": m.project.description,
        "stack": {"language": m.stack.language, "framework": m.stack.framework, "build_tool": m.stack.build_tool, "constraints": m.stack.constraints},
        "workflows": m.workflows,
        "brd": m.brd.as_ref().map(|b| json!({"path": b.path, "citation": b.citation})),
        "knowledge": m.knowledge.iter().map(|k| json!({"path": k.path, "citation": k.citation})).collect::<Vec<_>>(),
        "execution_mode": m.execution.as_ref().map(|e| e.mode.clone()),
        "roles": {"developer": m.roles.developer, "steward": m.roles.steward, "approvers": m.roles.approvers},
    })
}

/// Lifecycle status of a project from its real records: `ready` (registered, nothing
/// running), `in_progress`, `waiting_approval`, `blocked`, `completed`, `failed`, `cancelled`.
fn project_status(workflows: &[WorkflowInstance]) -> (&'static str, Option<&WorkflowInstance>) {
    // `list` is newest first.
    let active: Vec<&WorkflowInstance> = workflows.iter().filter(|w| !w.status.is_terminal()).collect();
    if let Some(w) = active.iter().find(|w| w.data.blocked.is_some()) {
        return ("blocked", Some(w));
    }
    if let Some(w) = active
        .iter()
        .find(|w| w.status == eaeh_workflow::InstanceStatus::WaitingApproval)
    {
        return ("waiting_approval", Some(w));
    }
    if let Some(w) = active.first() {
        return ("in_progress", Some(w));
    }
    match workflows.first() {
        None => ("ready", None),
        Some(w) => (
            match w.status {
                eaeh_workflow::InstanceStatus::Succeeded => "completed",
                eaeh_workflow::InstanceStatus::Cancelled => "cancelled",
                _ => "failed",
            },
            Some(w),
        ),
    }
}

fn current_workflow_view(w: &WorkflowInstance) -> Value {
    json!({"id": w.id, "definition_id": w.definition_id, "status": w.status, "state": w.state,
           "blocked": w.data.blocked, "updated_at": w.updated_at, "created_at": w.created_at})
}

fn allowed(st: &AppState, p: &Principal, action: Action, scope: Scope) -> bool {
    st.harness
        .authz
        .authorize(p, action, Target::Scope(scope))
        .is_ok()
}

/// Requirements / HLD / LLD (Phase 3 engineering documents) or a generic label.
fn document_type(artifact_name: Option<&str>, subject_type: &str) -> String {
    let n = artifact_name.unwrap_or_default();
    let leaf = n.rsplit('/').next().unwrap_or(n);
    match leaf {
        "requirements" | "requirements.json" => "Requirements".into(),
        "hld" | "hld-model.json" => "HLD".into(),
        "lld" | "lld-model.json" => "LLD".into(),
        _ if !n.is_empty() => n.to_string(),
        _ => subject_type.to_string(),
    }
}

/// Instance without bulky agent outputs (they are artifacts and readable separately).
fn instance_view(i: &WorkflowInstance) -> Value {
    let d = &i.data;
    json!({
        "id": i.id,
        "definition_id": i.definition_id,
        "definition_version": i.definition_version,
        "project_id": i.scope.project,
        "state": i.state,
        "status": i.status,
        "version": i.version,
        "started_by": i.started_by,
        "correlation_id": i.correlation_id,
        "cancel_requested": i.cancel_requested,
        "created_at": i.created_at,
        "updated_at": i.updated_at,
        "artifacts": d.artifacts,
        "approved_gates": d.approved_gates,
        "current_approval": d.current_approval,
        "revisions": d.revisions,
        "visits": d.visits,
        "quality": d.quality,
        "feedback": d.feedback,
        "failure": d.failure,
        "awaiting_external": d.awaiting_external,
        "blocked": d.blocked,
    })
}

async fn approval_view(st: &AppState, rec: &ApprovalRecord, project_name: &str) -> Value {
    let meta = match rec.artifact_id.as_deref().map(str::parse::<ArtifactId>) {
        Some(Ok(id)) => st.harness.artifacts.meta(rec.scope, id).await.ok(),
        _ => None,
    };
    let workflow = match rec.workflow_instance_id.as_deref().map(str::parse::<WorkflowInstanceId>) {
        Some(Ok(id)) => st.harness.engine.get(id).await.ok().map(|i| {
            json!({"id": i.id, "definition_id": i.definition_id, "definition_version": i.definition_version, "state": i.state, "status": i.status})
        }),
        _ => None,
    };
    json!({
        "id": rec.id,
        "project_id": rec.scope.project,
        "project_name": project_name,
        "subject_type": rec.subject_type,
        "subject_ref": rec.subject_ref,
        "document_type": document_type(meta.as_ref().map(|m| m.name.as_str()), &rec.subject_type),
        "gate": rec.gate,
        "status": rec.status,
        "required_role": rec.required_role,
        "authors": rec.authors,
        "requested_by": rec.requested_by,
        "created_at": rec.created_at,
        "decided_at": rec.decided_at,
        "decision": rec.decision,
        "artifact_id": rec.artifact_id,
        "artifact_hash": rec.artifact_hash,
        "artifact": meta.map(|m| json!({"name": m.name, "version": m.version, "hash": m.content_hash, "media_type": m.media_type, "size_bytes": m.size_bytes, "created_at": m.created_at})),
        "workflow": workflow,
    })
}

fn status_bucket(s: &str) -> &'static str {
    match s {
        "running" | "waiting_approval" | "waiting_external" => "in_progress",
        "succeeded" => "completed",
        "failed" | "escalated" => "failed_or_blocked",
        "cancelled" => "cancelled",
        _ => "other",
    }
}

// ---------------------------------------------------------------- dashboard

async fn dashboard(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    let projects = visible_projects(&st, &p, None).await?;
    let mut summary: BTreeMap<&str, i64> = [
        ("projects", projects.len() as i64),
        ("in_progress", 0),
        ("executing_tasks", 0),
        ("waiting_external", 0),
        ("waiting_approval", 0),
        ("pending_approvals", 0),
        ("completed", 0),
        ("failed_or_blocked", 0),
    ]
    .into_iter()
    .collect();
    let mut project_rows = vec![];
    let mut active = vec![];
    let mut pending = vec![];
    let mut activity = vec![];
    for (pid, name) in &projects {
        let scope = Scope::new(p.tenant, *pid);
        let can_wf = allowed(&st, &p, Action::WorkflowRead, scope);
        let can_ap = allowed(&st, &p, Action::ApprovalRead, scope);
        let mut row = json!({"id": pid, "name": name, "restricted": !(can_wf || can_ap)});
        if can_wf || can_ap {
            let (ws, m, issue) = project_profile(&st, scope).await;
            row["workspace"] = json!(ws);
            row["description"] = json!(m.as_ref().map(|m| m.project.description.clone()));
            row["stack"] = json!(m.as_ref().map(|m| json!({"language": m.stack.language, "framework": m.stack.framework, "build_tool": m.stack.build_tool})));
            row["registered_workflows"] = json!(m.as_ref().map(|m| m.workflows.clone()));
            row["manifest_issue"] = json!(issue);
        }
        if can_wf {
            let all = st.harness.engine.list(&p, scope, None, 50).await?;
            let (status, current) = project_status(&all);
            row["status"] = json!(status);
            row["current_workflow"] = json!(current.map(current_workflow_view));
            row["last_activity"] = json!(all.iter().map(|w| w.updated_at.clone()).max());
            let counts = st.harness.engine.status_counts(&p, scope).await?;
            for (s, n) in &counts {
                let b = status_bucket(s);
                if let Some(v) = summary.get_mut(b) {
                    *v += n;
                }
                if let Some(v) = summary.get_mut(s.as_str()) {
                    *v += n;
                }
            }
            let tasks = st.harness.engine.active_tasks(&p, scope, 100).await?;
            *summary.entry("executing_tasks").or_default() += tasks
                .iter()
                .filter(|t| t.status == "running" || t.status == "awaiting_external")
                .count() as i64;
            for inst in st.harness.engine.list_active(&p, scope, 50).await? {
                let its: Vec<_> = tasks
                    .iter()
                    .filter(|t| t.workflow_instance_id == inst.id.to_string())
                    .collect();
                active.push(json!({"project_id": pid, "project_name": name, "workflow": instance_view(&inst), "tasks": its}));
            }
            for a in st.harness.engine.recent_activity(&p, scope, 25).await? {
                let mut v = serde_json::to_value(a).map_err(HarnessError::from)?;
                v["project_id"] = json!(pid);
                v["project_name"] = json!(name);
                activity.push(v);
            }
            row["workflow_counts"] = json!(counts);
            row["active_tasks"] = json!(tasks.len());
        }
        if can_ap {
            let recs = st
                .harness
                .approvals
                .list(scope, Some(ApprovalStatus::Pending), None, 100)
                .await?;
            *summary.entry("pending_approvals").or_default() += recs.len() as i64;
            row["pending_approvals"] = json!(recs.len());
            for r in &recs {
                pending.push(approval_view(&st, r, name).await);
            }
        }
        project_rows.push(row);
    }
    active.sort_by(|a, b| {
        b["workflow"]["updated_at"]
            .as_str()
            .cmp(&a["workflow"]["updated_at"].as_str())
    });
    pending.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    activity.sort_by(|a, b| b["occurred_at"].as_str().cmp(&a["occurred_at"].as_str()));
    activity.truncate(25);
    Ok(Json(json!({
        "generated_at": st.harness.db.now().to_rfc3339(),
        "execution_mode": st.harness.execution_mode,
        "summary": summary,
        "projects": project_rows,
        "active": active,
        "pending_approvals": pending,
        "activity": activity,
    })))
}

// ---------------------------------------------------------------- cross-project lists

#[derive(Deserialize)]
struct ListQuery {
    project_id: Option<ProjectId>,
    status: Option<String>,
    limit: Option<u32>,
}

async fn activity(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let mut items = vec![];
    let mut restricted = vec![];
    for (pid, name) in visible_projects(&st, &p, q.project_id).await? {
        let scope = Scope::new(p.tenant, pid);
        if !allowed(&st, &p, Action::WorkflowRead, scope) {
            restricted.push(pid);
            continue;
        }
        for a in st.harness.engine.recent_activity(&p, scope, limit).await? {
            let mut v = serde_json::to_value(a).map_err(HarnessError::from)?;
            v["project_id"] = json!(pid);
            v["project_name"] = json!(name);
            items.push(v);
        }
    }
    items.sort_by(|a, b| b["occurred_at"].as_str().cmp(&a["occurred_at"].as_str()));
    items.truncate(limit as usize);
    Ok(Json(json!({"items": items, "restricted_projects": restricted})))
}

async fn approval_queue(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Value>> {
    let status = match q.status.as_deref() {
        None | Some("pending") => Some(ApprovalStatus::Pending),
        Some("all") => None,
        Some(s) => Some(
            serde_json::from_value::<ApprovalStatus>(json!(s))
                .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid_input", "invalid status"))?,
        ),
    };
    let limit = q.limit.unwrap_or(100).clamp(1, 200);
    let mut items = vec![];
    let mut restricted = vec![];
    for (pid, name) in visible_projects(&st, &p, q.project_id).await? {
        let scope = Scope::new(p.tenant, pid);
        if !allowed(&st, &p, Action::ApprovalRead, scope) {
            restricted.push(pid);
            continue;
        }
        for r in st.harness.approvals.list(scope, status, None, limit).await? {
            items.push(approval_view(&st, &r, &name).await);
        }
    }
    items.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    items.truncate(limit as usize);
    Ok(Json(json!({"items": items, "restricted_projects": restricted})))
}

async fn workflow_queue(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Value>> {
    let active_only = match q.status.as_deref() {
        None | Some("all") => false,
        Some("active") => true,
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "status must be active or all",
            ));
        }
    };
    let limit = q.limit.unwrap_or(100).clamp(1, 100);
    let mut items = vec![];
    let mut restricted = vec![];
    for (pid, name) in visible_projects(&st, &p, q.project_id).await? {
        let scope = Scope::new(p.tenant, pid);
        if !allowed(&st, &p, Action::WorkflowRead, scope) {
            restricted.push(pid);
            continue;
        }
        let list = if active_only {
            st.harness.engine.list_active(&p, scope, limit).await?
        } else {
            st.harness.engine.list(&p, scope, None, limit).await?
        };
        for i in list {
            items.push(json!({"project_id": pid, "project_name": name, "workflow": instance_view(&i)}));
        }
    }
    items.sort_by(|a, b| {
        b["workflow"]["updated_at"]
            .as_str()
            .cmp(&a["workflow"]["updated_at"].as_str())
    });
    items.truncate(limit as usize);
    Ok(Json(json!({"items": items, "restricted_projects": restricted})))
}

// ---------------------------------------------------------------- details

async fn project_name(st: &AppState, p: &Principal, id: ProjectId) -> String {
    st.harness
        .identity
        .projects(p.tenant)
        .await
        .ok()
        .and_then(|v| v.into_iter().find(|(i, _)| *i == id).map(|(_, n)| n))
        .unwrap_or_default()
}

async fn workflow_detail(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<WorkflowInstanceId>,
) -> ApiResult<Json<Value>> {
    let inst = st.harness.engine.get_for(&p, id).await?;
    let history = st.harness.engine.history(id).await?;
    let tasks = st.harness.engine.tasks_for(&p, id).await?;
    let definition = st
        .harness
        .engine
        .definition(&inst.definition_id, &inst.definition_version)
        .await
        .ok();
    let name = project_name(&st, &p, inst.scope.project).await;
    let approvals = if allowed(&st, &p, Action::ApprovalRead, inst.scope) {
        let mut v = vec![];
        for r in st.harness.approvals.list(inst.scope, None, None, 500).await? {
            if r.workflow_instance_id.as_deref() == Some(id.to_string().as_str()) {
                v.push(approval_view(&st, &r, &name).await);
            }
        }
        Some(v)
    } else {
        None
    };
    let governance = governance(&st, &p, &inst).await?;
    // Approved documents in the project workspace; files are re-hashed for this response.
    let documents = st.harness.engine.documents(&p, id).await?;
    let workspace = st.harness.engine.materializer().workspace(inst.scope).await?;
    let mut provenance = serde_json::Map::new();
    for (state, a) in &inst.data.artifacts {
        if let Ok(m) = st.harness.artifacts.meta(inst.scope, a.id).await {
            provenance.insert(state.clone(), generation(&m));
        }
    }
    Ok(Json(json!({
        "project": {"id": inst.scope.project, "name": name},
        "workflow": instance_view(&inst),
        "definition": definition,
        "history": history,
        "tasks": tasks,
        "approvals": approvals,
        "execution_mode": st.harness.execution_mode,
        "governance": governance,
        "artifact_provenance": provenance,
        "documents": documents,
        "workspace": workspace,
    })))
}

/// Who or what produced an artifact, from its recorded provenance: a connected AI client
/// (`external:<client>`), the deterministic replay fixture (`replay/…`, NOT AI), a direct
/// model provider, or a platform step (no model).
fn generation(m: &eaeh_runtime::ArtifactMeta) -> Value {
    let models = &m.provenance.models;
    let source = if models.iter().any(|x| x.starts_with("external:")) {
        "external_ai"
    } else if models.iter().any(|x| x.starts_with("replay/")) {
        "deterministic_replay"
    } else if !models.is_empty() {
        "direct_provider"
    } else {
        "platform"
    };
    json!({
        "source": source,
        "models": models,
        "agent_definition": m.provenance.agent_definition,
        "produced_by": m.provenance.produced_by,
        "created_at": m.created_at,
        "artifact": m.name,
        "version": m.version,
    })
}

/// Gate verification (the same rules as the engine's completion guard) and the recorded
/// `workflow.completion_check` evidence for a terminal instance.
async fn governance(st: &AppState, p: &Principal, inst: &WorkflowInstance) -> ApiResult<Value> {
    let gates = st.harness.engine.gate_checks(p, inst.id).await?;
    let check = st
        .harness
        .db
        .audit_by_resource(&inst.id.to_string(), "workflow.completion_check")
        .await?;
    let completion = check.last().map(|r| {
        json!({
            "outcome": r.outcome,
            "occurred_at": r.occurred_at,
            "details": serde_json::from_str::<Value>(&r.details).unwrap_or(Value::Null),
        })
    });
    let granted = gates.iter().filter(|g| g.approval_id.is_some()).count();
    let invalid = gates.iter().filter(|g| !g.issues.is_empty()).count();
    let verified = match inst.status {
        eaeh_workflow::InstanceStatus::Succeeded => Some(
            invalid == 0
                && granted == gates.len()
                && completion.as_ref().is_some_and(|c| c["outcome"] == "succeeded"),
        ),
        _ => None,
    };
    Ok(json!({
        "gates": gates,
        "gates_total": gates.len(),
        "gates_granted": granted,
        "gates_with_issues": invalid,
        "completion_check": completion,
        "completion_verified": verified,
    }))
}

async fn approval_document(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<ApprovalId>,
) -> ApiResult<Json<Value>> {
    let rec = st.harness.approvals.get(id).await?;
    st.harness
        .authz
        .authorize(&p, Action::ApprovalRead, Target::Scope(rec.scope))
        .map_err(|_| ApiError::from(HarnessError::not_found("approval not found")))?;
    authorize(&st, &p, Action::ArtifactRead, Target::Scope(rec.scope)).await?;
    let name = project_name(&st, &p, rec.scope.project).await;
    let view = approval_view(&st, &rec, &name).await;

    let mut document = Value::Null;
    if let Some(aid) = rec.artifact_id.as_deref() {
        let aid: ArtifactId = aid.parse()?;
        let (meta, bytes) = st.harness.artifacts.read(rec.scope, aid).await?;
        let manifest: Option<Value> = serde_json::from_slice(&bytes).ok();
        let files = manifest
            .as_ref()
            .and_then(|m| m.get("files"))
            .and_then(Value::as_object);
        document = match files {
            Some(files) => {
                let mut out = vec![];
                for (path, f) in files {
                    let fid = f.get("artifact_id").and_then(Value::as_str).unwrap_or_default();
                    let expected = f.get("hash").and_then(Value::as_str);
                    let (fm, content) = st.harness.artifacts.read(rec.scope, fid.parse()?).await?;
                    if Some(fm.content_hash.as_str()) != expected {
                        return Err(HarnessError::integrity(format!(
                            "{path}: hash differs from the bundle manifest"
                        ))
                        .into());
                    }
                    out.push(json!({
                        "path": path,
                        "artifact_id": fm.id,
                        "name": fm.name,
                        "version": fm.version,
                        "hash": fm.content_hash,
                        "media_type": fm.media_type,
                        "content": String::from_utf8(content).ok(),
                    }));
                }
                json!({"artifact": meta, "bundle": true, "files": out})
            }
            None => json!({
                "artifact": meta,
                "bundle": false,
                "files": [{
                    "path": meta.name.rsplit('/').next().unwrap_or("document"),
                    "artifact_id": meta.id,
                    "name": meta.name,
                    "version": meta.version,
                    "hash": meta.content_hash,
                    "media_type": meta.media_type,
                    "content": String::from_utf8(bytes).ok(),
                }],
            }),
        };
    }

    // Workflow context: related artifacts, quality results, revision history, trace links.
    let mut workflow = Value::Null;
    if let Some(Ok(wid)) = rec
        .workflow_instance_id
        .as_deref()
        .map(str::parse::<WorkflowInstanceId>)
        && let Ok(inst) = st.harness.engine.get_for(&p, wid).await
    {
        let mut history = vec![];
        for r in st.harness.approvals.list(rec.scope, None, None, 500).await? {
            if r.workflow_instance_id == rec.workflow_instance_id && r.gate == rec.gate && r.id != rec.id {
                history.push(approval_view(&st, &r, &name).await);
            }
        }
        let trace: Vec<_> = st
            .harness
            .engine
            .trace(&p, rec.scope, None, 3)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|l| l.workflow_instance_id == wid.to_string())
            .collect();
        let mut provenance = serde_json::Map::new();
        for (state, a) in &inst.data.artifacts {
            if let Ok(m) = st.harness.artifacts.meta(inst.scope, a.id).await {
                provenance.insert(state.clone(), generation(&m));
            }
        }
        let workspace_document = st
            .harness
            .engine
            .documents(&p, wid)
            .await?
            .into_iter()
            .find(|d| Some(d.gate.as_str()) == rec.gate.as_deref());
        let workspace = st.harness.engine.materializer().workspace(inst.scope).await?;
        workflow = json!({
            "workspace_document": workspace_document,
            "workspace": workspace,
            "instance": instance_view(&inst),
            "previous_requests": history,
            "trace_links": trace,
            "artifact_provenance": provenance,
            "execution_mode": st.harness.execution_mode,
        });
    }
    Ok(Json(json!({
        "approval": view,
        "document": document,
        "workflow": workflow,
        "decision_endpoint": format!("/v1/approvals/{id}/decision"),
    })))
}

async fn project_overview(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<ProjectId>,
) -> ApiResult<Json<Value>> {
    let (pid, name) = visible_projects(&st, &p, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::from(HarnessError::not_found("project not found")))?;
    let scope = Scope::new(p.tenant, pid);
    let can_wf = allowed(&st, &p, Action::WorkflowRead, scope);
    let can_ap = allowed(&st, &p, Action::ApprovalRead, scope);
    if !(can_wf || can_ap) {
        return Err(
            HarnessError::forbidden("workflow.read or approval.read is required for this project").into(),
        );
    }
    let domain = st
        .harness
        .identity
        .project_domain(p.tenant, pid)
        .await
        .ok()
        .flatten();
    let mut out = json!({"project": {"id": pid, "name": name, "domain": domain}});
    let (ws, manifest, issue) = project_profile(&st, scope).await;
    out["workspace"] = json!(ws);
    out["manifest"] = json!(manifest.as_ref().map(manifest_view));
    out["manifest_issue"] = json!(issue);
    // Read-only readiness (the same checks as `eaeh project readiness`).
    if let (Some(m), Some(w)) = (&manifest, &ws) {
        out["readiness"] =
            match eaeh_runtime::project::readiness(&st.harness, &p, m, std::path::Path::new(&w.root), None)
                .await
            {
                Ok(r) => json!(r),
                Err(e) => json!({"error": e.message}),
            };
    }
    if allowed(&st, &p, Action::AuditRead, scope) {
        out["audit"] = json!(
            st.harness
                .db
                .audit_recent(Some(p.tenant), None, 400)
                .await?
                .into_iter()
                .filter(|r| r.project_id.as_deref() == Some(pid.to_string().as_str()))
                .take(50)
                .collect::<Vec<_>>()
        );
    }
    if can_wf {
        let counts = st.harness.engine.status_counts(&p, scope).await?;
        let workflows = st.harness.engine.list(&p, scope, None, 100).await?;
        // Engineering artifacts produced by this project's workflows (latest version per name).
        let mut artifacts: BTreeMap<String, Value> = BTreeMap::new();
        let mut defs: HashMap<(String, String), Option<WorkflowDefinition>> = HashMap::new();
        for w in &workflows {
            let key = (w.definition_id.clone(), w.definition_version.clone());
            if !defs.contains_key(&key) {
                let d = st.harness.engine.definition(&key.0, &key.1).await.ok();
                defs.insert(key.clone(), d);
            }
            let def = defs.get(&key).and_then(|d| d.as_ref());
            for (state, a) in &w.data.artifacts {
                // The gate that reviews this state's artifact, if any, and whether it passed.
                let gate = def.and_then(|d| {
                    d.states.iter().find(|s| {
                        matches!(&s.kind, StateKind::ApprovalGate { artifact_from, .. } if artifact_from == state)
                    })
                });
                let gate_name = gate.map(|g| g.name.clone());
                let approved = gate_name.as_ref().map(|g| w.data.approved_gates.contains_key(g));
                let entry = json!({
                    "id": a.id, "name": a.name, "version": a.version, "hash": a.hash, "state": state,
                    "document_type": document_type(Some(&a.name), "artifact"),
                    "workflow_instance_id": w.id, "workflow_status": w.status,
                    "gate": gate_name, "gate_approved": approved,
                });
                match artifacts.get(&a.name) {
                    Some(e) if e["version"].as_i64() >= Some(a.version) => {}
                    _ => {
                        artifacts.insert(a.name.clone(), entry);
                    }
                }
            }
        }
        let (status, current) = project_status(&workflows);
        out["status"] = json!(status);
        out["current_workflow"] = json!(current.map(current_workflow_view));
        // Requirements / HLD / LLD of the latest design run: approval and workspace state.
        if let Some(w) = workflows
            .iter()
            .find(|w| w.definition_id == "engineering.design")
            .or(workflows.first())
        {
            out["documents"] = json!({
                "workflow_instance_id": w.id,
                "items": st.harness.engine.documents(&p, w.id).await?,
            });
        }
        out["workflow_counts"] = json!(counts);
        out["workflows"] = json!(workflows.iter().map(instance_view).collect::<Vec<_>>());
        out["artifacts"] = json!(artifacts.into_values().collect::<Vec<_>>());
        out["activity"] = json!(st.harness.engine.recent_activity(&p, scope, 25).await?);
    }
    if can_ap {
        let mut v = vec![];
        for r in st.harness.approvals.list(scope, None, None, 100).await? {
            v.push(approval_view(&st, &r, &name).await);
        }
        out["approvals"] = json!(v);
    }
    Ok(Json(out))
}
