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
        .route("/v1/skills/catalog", get(skills_catalog))
        .route("/v1/skills/sync", axum::routing::post(skills_sync))
        .route("/v1/skills/discover", axum::routing::post(skills_discover))
        .route("/v1/skills/{id}/{version}/files", get(skill_files))
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
pub(crate) async fn project_profile(
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

/// What a run waits for, from its persisted state: an approval decision, an external AI
/// client, a blocked document write, a failure to recover, or nothing.
fn pending_action(w: &WorkflowInstance) -> Value {
    use eaeh_workflow::InstanceStatus as S;
    let (kind, text) = match w.status {
        _ if w.data.blocked.is_some() => (
            "blocked",
            format!(
                "document write blocked at {}: resolve the conflict, then resume",
                w.state
            ),
        ),
        S::WaitingApproval => ("approval", format!("human decision at {}", w.state)),
        S::WaitingExternal => (
            "external_ai",
            format!("an AI client or worker must execute {}", w.state),
        ),
        S::Running => ("running", format!("executing {}", w.state)),
        S::Failed => (
            "failed",
            format!("failed at {}: retry the stage (restart preview)", w.state),
        ),
        S::Succeeded => ("none", "completed".to_string()),
        S::Cancelled => ("none", "cancelled".to_string()),
        _ => ("none", String::new()),
    };
    json!({"kind": kind, "text": text})
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
        _ if subject_type == "skill" => "Skill".into(),
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
                active.push(json!({"project_id": pid, "project_name": name, "workflow": instance_view(&inst), "tasks": its, "pending_action": pending_action(&inst)}));
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
    /// Workflow queue only: definition id, current stage, run status and last update since.
    definition: Option<String>,
    stage: Option<String>,
    run_status: Option<String>,
    since: Option<String>,
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
        let tasks = st.harness.engine.active_tasks(&p, scope, 100).await?;
        for i in list {
            let keep = q
                .definition
                .as_deref()
                .is_none_or(|d| d.is_empty() || i.definition_id.as_str() == d)
                && q.stage.as_deref().is_none_or(|d| d.is_empty() || i.state == d)
                && q.run_status
                    .as_deref()
                    .is_none_or(|d| d.is_empty() || i.status.as_str() == d)
                && q.since
                    .as_deref()
                    .is_none_or(|d| d.is_empty() || i.updated_at.as_str() >= d);
            if !keep {
                continue;
            }
            let current = tasks.iter().find(|t| t.workflow_instance_id == i.id.to_string());
            items.push(json!({
                "project_id": pid,
                "project_name": name,
                "workflow": instance_view(&i),
                "current_task": current,
                "pending_action": pending_action(&i),
                "operations": {"resume": !i.status.is_terminal(), "cancel": !i.status.is_terminal(), "restart": true},
            }));
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
    let lineage = st.harness.engine.lineage(&p, id).await?;
    // What the engine supports for this run in its current status; the backend still
    // authorizes each call (`workflow.start`).
    let terminal = inst.status.is_terminal();
    let operations = json!({
        "resume": !terminal,
        "cancel": !terminal,
        "restart": true,
        "may_operate": allowed(&st, &p, Action::WorkflowStart, inst.scope),
    });
    Ok(Json(json!({
        "project": {"id": inst.scope.project, "name": name},
        "workflow": instance_view(&inst),
        "lineage": lineage,
        "operations": operations,
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
    // Skill approvals: the full definition under review (MARKDOWN-SKILLS-PLAN, K4).
    let skill = if rec.subject_type == "skill" {
        skill_review(&st, &p, &rec).await?
    } else {
        Value::Null
    };
    Ok(Json(json!({
        "approval": view,
        "document": document,
        "skill": skill,
        "workflow": workflow,
        "decision_endpoint": format!("/v1/approvals/{id}/decision"),
    })))
}

/// Stage layers (`workflow/stage`) per task kind, from the latest workflow definitions.
async fn layers_by_task_kind(st: &AppState) -> BTreeMap<String, Vec<String>> {
    let mut ids: Vec<String> = st
        .harness
        .engine
        .definitions()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.id.to_string())
        .collect();
    ids.sort();
    ids.dedup();
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (w, stage, kind) in eaeh_runtime::project::workflow_layers(&st.harness, &ids)
        .await
        .unwrap_or_default()
    {
        let e = out.entry(kind).or_default();
        let l = format!("{w}/{stage}");
        if !e.contains(&l) {
            e.push(l);
        }
    }
    out
}

fn skill_view(d: &eaeh_skills::SkillDefinition, layers: &BTreeMap<String, Vec<String>>) -> Value {
    let applies: Vec<&String> = d
        .task_kinds
        .iter()
        .flat_map(|k| layers.get(k).into_iter().flatten())
        .collect();
    json!({
        "id": d.id, "version": d.version.to_string(), "reference": d.reference(), "description": d.description,
        "task_kinds": d.task_kinds, "capabilities": d.capabilities, "layers": applies,
        "required_tools": d.required_tools, "required_knowledge": d.required_knowledge, "max_risk": d.max_risk,
        "dependencies": d.dependencies, "evaluation_criteria": d.evaluation_criteria,
        "output_schema": d.output_schema, "instructions": d.instructions,
        // Community SKILL.md: the complete YAML front matter (license, metadata, any other
        // properties) and how the layers were determined.
        "frontmatter": d.frontmatter,
        "layers_from": d.layers_from,
        "package": d.package.as_ref().map(|p| json!({
            "dir": p.dir, "source_version": p.source_version.to_string(), "revision": p.revision,
            "tree_hash": p.tree_hash, "file_count": p.files.len(),
            "size": p.files.iter().map(|f| f.size).sum::<u64>(),
        })),
    })
}

/// Highest approved version of a skill id (the one agents use).
fn in_force(records: &[eaeh_skills::SkillRecord], id: &eaeh_core::SkillId) -> Option<semver::Version> {
    records
        .iter()
        .filter(|r| &r.definition.id == id && r.status == eaeh_skills::SkillStatus::Approved)
        .map(|r| r.definition.version.clone())
        .max()
}

/// The skill an approval request is for, with its full definition and digest.
async fn skill_review(st: &AppState, p: &Principal, rec: &ApprovalRecord) -> ApiResult<Value> {
    authorize(st, p, Action::SkillRead, Target::Tenant(p.tenant)).await?;
    let layers = layers_by_task_kind(st).await;
    let id = rec.id.to_string();
    let found = st
        .harness
        .skills
        .list(p)
        .await?
        .into_iter()
        .find(|r| r.approval_id.as_deref() == Some(id.as_str()));
    let files = eaeh_runtime::project::repository_skills(&st.harness).unwrap_or_default();
    Ok(match found {
        Some(r) => {
            let file = files
                .iter()
                .find(|(_, d)| d.id == r.definition.id && d.version == r.definition.version)
                .map(|(f, _)| f.display().to_string());
            let mut v = skill_view(&r.definition, &layers);
            v["digest"] = json!(r.digest);
            v["status"] = json!(r.status);
            v["registered_by"] = json!(r.registered_by);
            v["file"] = json!(file);
            v["files_endpoint"] = json!(format!(
                "/v1/skills/{}/{}/files",
                r.definition.id, r.definition.version
            ));
            v
        }
        None => Value::Null,
    })
}

/// Skill catalog (`skill.read`): every registered version (status, approval request,
/// whether agents use it) and every skill file or directory in `definitions.skills_dir`
/// that is not registered yet or whose content changed.
async fn skills_catalog(State(st): St, Extension(p): Extension<Principal>) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::SkillRead, Target::Tenant(p.tenant)).await?;
    let layers = layers_by_task_kind(&st).await;
    let registered = st.harness.skills.list(&p).await?;
    let (files, file_error) = match eaeh_runtime::project::repository_skills(&st.harness) {
        Ok(f) => (f, None),
        Err(e) => (vec![], Some(e.message)),
    };
    let dirs = eaeh_runtime::project::skill_directories(&st.harness).unwrap_or_default();
    let mut items: BTreeMap<String, Value> = BTreeMap::new();
    for r in &registered {
        let mut v = skill_view(&r.definition, &layers);
        v["registration"] = json!({"status": r.status, "approval_id": r.approval_id, "digest": r.digest, "registered_by": r.registered_by});
        v["in_force"] =
            json!(in_force(&registered, &r.definition.id).as_ref() == Some(&r.definition.version));
        v["file"] = Value::Null;
        v["file_changed"] = json!(false);
        items.insert(r.definition.reference(), v);
    }
    for (f, d) in &files {
        let digest = d.digest().unwrap_or_default();
        let reg = registered
            .iter()
            .find(|r| r.definition.id == d.id && r.definition.version == d.version);
        let v = items.entry(d.reference()).or_insert_with(|| {
            let mut v = skill_view(d, &layers);
            v["registration"] = Value::Null;
            v["in_force"] = json!(false);
            v
        });
        v["file"] = json!(f.display().to_string());
        v["format"] = json!(if f.extension().is_some_and(|x| x == "md") {
            "markdown"
        } else {
            "json"
        });
        v["file_digest"] = json!(digest);
        v["file_changed"] = json!(reg.is_some_and(|r| r.digest != digest));
    }
    let mut directories = vec![];
    for (path, def) in &dirs {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        match def {
            Ok(d) => {
                let tree = d.tree_hash().unwrap_or_default();
                let reg = registered
                    .iter()
                    .find(|r| r.definition.id == d.id && r.definition.tree_hash() == Some(tree));
                match reg {
                    Some(r) => {
                        if let Some(v) = items.get_mut(&r.definition.reference()) {
                            v["file"] = json!(path.display().to_string());
                            v["format"] = json!("directory");
                        }
                    }
                    None => {
                        let mut v = skill_view(d, &layers);
                        v["reference"] = json!(format!("{} (not registered yet)", d.reference()));
                        v["registration"] = Value::Null;
                        v["in_force"] = json!(false);
                        v["file"] = json!(path.display().to_string());
                        v["format"] = json!("directory");
                        v["file_changed"] = json!(registered.iter().any(|r| r.definition.id == d.id));
                        items.insert(format!("{}#disk", d.reference()), v);
                    }
                }
                directories.push(json!({"dir": name, "skill": d.id, "manifest_version": d.version.to_string(), "tree_hash": tree, "registered": reg.map(|r| r.definition.version.to_string())}));
            }
            Err(e) => directories.push(json!({"dir": name, "error": e.message})),
        }
    }
    Ok(Json(json!({
        "skills_dir": st.harness.config.resolve(&st.harness.config.definitions.skills_dir),
        "file_error": file_error,
        "items": items.into_values().collect::<Vec<_>>(),
        "directories": directories,
        "layers": layers,
    })))
}

/// Complete content of a registered skill version (`skill.read`): every file with its kind,
/// hash and text, and – per file – how it differs from the version agents use now.
async fn skill_files(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath((id, version)): ApiPath<(String, String)>,
) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::SkillRead, Target::Tenant(p.tenant)).await?;
    let records = st.harness.skills.list(&p).await?;
    let rec = records
        .iter()
        .find(|r| r.definition.id.as_str() == id && r.definition.version.to_string() == version)
        .ok_or_else(|| ApiError::from(HarnessError::not_found("skill version not found")))?;
    let layers = layers_by_task_kind(&st).await;
    let current = in_force(&records, &rec.definition.id);
    let base = current
        .as_ref()
        .filter(|v| **v != rec.definition.version)
        .and_then(|v| {
            records
                .iter()
                .find(|r| r.definition.id == rec.definition.id && &r.definition.version == v)
        });
    let base_files: BTreeMap<String, String> = base
        .and_then(|b| b.definition.package.as_ref())
        .map(|p| {
            p.files
                .iter()
                .map(|f| (f.path.clone(), f.sha256.clone()))
                .collect()
        })
        .unwrap_or_default();
    // Single-file skills are shown as one manifest file.
    let files: Vec<eaeh_skills::SkillFile> = match &rec.definition.package {
        Some(pk) => pk.files.clone(),
        None => {
            let text = serde_json::to_string_pretty(&rec.definition).unwrap_or_default();
            vec![eaeh_skills::SkillFile {
                path: format!("{}.json", rec.definition.id),
                sha256: eaeh_core::hash::sha256_hex(text.as_bytes()),
                size: text.len() as u64,
                kind: "json".into(),
                content: Some(text),
            }]
        }
    };
    let mut out = vec![];
    for f in &files {
        let change = match (base, base_files.get(&f.path)) {
            (None, _) => Value::Null,
            (Some(_), None) => json!("added"),
            (Some(_), Some(h)) if h == &f.sha256 => json!("unchanged"),
            (Some(_), Some(_)) => json!("modified"),
        };
        out.push(json!({"path": f.path, "sha256": f.sha256, "size": f.size, "kind": f.kind, "content": f.content, "change": change}));
    }
    let removed: Vec<&String> = base_files
        .keys()
        .filter(|k| !files.iter().any(|f| &f.path == *k))
        .collect();
    let mut v = skill_view(&rec.definition, &layers);
    v["digest"] = json!(rec.digest);
    v["status"] = json!(rec.status);
    v["approval_id"] = json!(rec.approval_id);
    v["registered_by"] = json!(rec.registered_by);
    v["in_force_version"] = json!(current.map(|c| c.to_string()));
    v["compared_with"] = json!(base.map(|b| b.definition.version.to_string()));
    v["files"] = json!(out);
    v["removed_files"] = json!(removed);
    // Approval history of every version of this skill (approval service records).
    let mut history = vec![];
    for r in records.iter().filter(|r| r.definition.id == rec.definition.id) {
        if let Some(aid) = r
            .approval_id
            .as_deref()
            .and_then(|a| a.parse::<ApprovalId>().ok())
            && let Ok(a) = st.harness.approvals.get(aid).await
        {
            history.push(json!({"version": r.definition.version.to_string(), "approval_id": a.id, "status": a.status, "created_at": a.created_at, "decided_at": a.decided_at, "decision": a.decision}));
        }
    }
    history.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    v["history"] = json!(history);
    Ok(Json(v))
}

#[derive(Deserialize)]
struct DiscoverSkills {
    #[serde(default)]
    project_id: Option<ProjectId>,
}

/// "Scan now": automatic discovery on demand (`skill.read`). New or changed skill
/// directories are registered by `system:skill-discovery` as pending approval; nothing is
/// approved here.
async fn skills_discover(
    State(st): St,
    Extension(p): Extension<Principal>,
    Json(b): Json<DiscoverSkills>,
) -> ApiResult<Json<Value>> {
    authorize(&st, &p, Action::SkillRead, Target::Tenant(p.tenant)).await?;
    if let Some(pid) = b.project_id {
        visible_projects(&st, &p, Some(pid)).await?;
    }
    let out = eaeh_runtime::project::discover_skills(&st.harness, b.project_id).await?;
    Ok(Json(json!({"results": out})))
}

#[derive(Deserialize)]
struct SyncSkills {
    /// Governance project of the approval requests (the caller's project).
    project_id: ProjectId,
}

/// Register every skill file of `definitions.skills_dir` that is not registered yet, as
/// pending human approval – the same service and validation as `eaeh skill sync`
/// (`skill.manage` on the project). Nothing is approved here.
async fn skills_sync(
    State(st): St,
    Extension(p): Extension<Principal>,
    Json(b): Json<SyncSkills>,
) -> ApiResult<Json<Value>> {
    visible_projects(&st, &p, Some(b.project_id)).await?;
    authorize(
        &st,
        &p,
        Action::SkillManage,
        Target::Scope(Scope::new(p.tenant, b.project_id)),
    )
    .await?;
    let refs: Vec<String> = eaeh_runtime::project::repository_skills(&st.harness)?
        .iter()
        .map(|(_, d)| format!("{}@={}", d.id, d.version))
        .collect();
    let out = eaeh_runtime::project::sync_skills(&st.harness, &p, b.project_id, &refs).await?;
    Ok(Json(json!({"results": out})))
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
