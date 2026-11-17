//! Workflow and task operations for the portal and any API client
//! (PORTAL-OPERATIONS-PLAN, W1–W2).
//!
//! Every handler calls the same governed service the CLI calls: the workflow engine
//! (resume, restart preview, restart, lineage, task detail), the external-AI coordinator
//! (task retry, executions, workers) and the project planner (execution plan and readiness).
//! Authorization, idempotency and audit happen in those services; nothing here writes to the
//! database directly.

use crate::error::ApiError;
use crate::routes::{ApiPath, ApiQuery, AppState};
use axum::extract::{FromRequest, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use eaeh_core::{HarnessError, Principal, ProjectId, Scope, TaskId, WorkflowInstanceId};
use eaeh_runtime::coordinator as co;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

type St = State<Arc<AppState>>;
type ApiResult<T> = Result<T, ApiError>;

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
struct ApiJson<T>(T);

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/workflows/{id}/resume", post(resume))
        .route("/v1/workflows/{id}/restart/preview", post(restart_preview))
        .route("/v1/workflows/{id}/restart", post(restart))
        .route("/v1/workflows/{id}/lineage", get(lineage))
        .route("/v1/task-queue", get(task_queue))
        .route("/v1/tasks/{id}", get(task))
        .route("/v1/tasks/{id}/retry", post(retry_task))
        .route("/v1/projects/{id}/plan", get(project_plan))
        .route("/v1/executions", get(executions))
        .route("/v1/integrations", get(integrations))
}

async fn resume(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<WorkflowInstanceId>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    st.harness.resume_workflow(&p, id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"id": id, "status": "resuming"})),
    ))
}

#[derive(Deserialize, Default)]
struct PreviewBody {
    stage: Option<String>,
    input: Option<Value>,
}

async fn restart_preview(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<WorkflowInstanceId>,
    ApiJson(b): ApiJson<PreviewBody>,
) -> ApiResult<Json<Value>> {
    let plan = st
        .harness
        .engine
        .restart_plan(&p, id, b.stage.as_deref(), b.input.as_ref())
        .await?;
    Ok(Json(serde_json::to_value(plan).map_err(HarnessError::from)?))
}

#[derive(Deserialize)]
struct RestartBody {
    stage: String,
    input: Option<Value>,
    /// The fingerprint of the preview the user confirmed; a stale one is refused.
    fingerprint: String,
    reason: String,
}

async fn restart(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<WorkflowInstanceId>,
    ApiJson(b): ApiJson<RestartBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let reason = b.reason.trim();
    if reason.is_empty() || reason.len() > 2000 {
        return Err(
            HarnessError::invalid_input("a reason (1–2000 characters) is required for a restart").into(),
        );
    }
    let (new, plan) = st
        .harness
        .restart_workflow(&p, id, &b.stage, b.input, &b.fingerprint, reason)
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(
            json!({"id": new, "source": id, "stage": plan.stage, "source_cancelled": plan.source_will_be_cancelled}),
        ),
    ))
}

async fn lineage(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<WorkflowInstanceId>,
) -> ApiResult<Json<Value>> {
    Ok(Json(st.harness.engine.lineage(&p, id).await?))
}

/// A task with its input, output, stage definition, dependencies, agent and skill versions,
/// produced artifact, quality result, the external-AI hand-off (no package) and the
/// recorded execution attempts.
async fn task(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Value>> {
    let d = st.harness.engine.task_detail(&p, &id).await?;
    let inst = st
        .harness
        .engine
        .get_for(&p, d.task.workflow_instance_id.parse()?)
        .await?;
    let scope = inst.scope;
    let agent = match d.task.agent.as_deref().and_then(|a| a.split_once('@')) {
        Some((aid, ver)) => match (eaeh_core::AgentDefinitionId::new(aid), semver::Version::parse(ver)) {
            (Ok(aid), Ok(ver)) => st
                .harness
                .agents
                .registry()
                .get(&aid, &ver)
                .await
                .ok()
                .map(|(a, status)| {
                    json!({"reference": format!("{}@{}", a.id, a.version), "status": status, "skills": a.skills, "purpose": a.purpose, "capabilities": a.capabilities, "allowed_tools": a.allowed_tools, "autonomy": a.autonomy})
                }),
            _ => None,
        },
        None => None,
    };
    let handoff = st
        .harness
        .agents
        .external_tasks(&p, scope, false)
        .await
        .ok()
        .and_then(|v| v.into_iter().find(|t| t.task_id == id));
    let attempts = co::executions(&st.harness, &p, scope, Some(&id), 50)
        .await
        .unwrap_or_default();
    // Only failed or stopped hand-offs that wait for a result are retried (coordinator rule).
    let retryable = handoff.as_ref().is_some_and(|h| {
        h.status == "awaiting" && matches!(h.dispatch_state.as_str(), "blocked" | "exhausted" | "retry_wait")
    });
    let mut v = serde_json::to_value(&d).map_err(HarnessError::from)?;
    if let Value::Object(m) = &mut v {
        m.insert("workflow".into(), json!({"id": inst.id, "definition": inst.definition_id, "status": inst.status, "state": inst.state}));
        m.insert("project".into(), json!(scope.project));
        m.insert("agent_definition".into(), agent.unwrap_or(Value::Null));
        m.insert(
            "handoff".into(),
            serde_json::to_value(&handoff).map_err(HarnessError::from)?,
        );
        m.insert(
            "executions".into(),
            serde_json::to_value(&attempts).map_err(HarnessError::from)?,
        );
        m.insert(
            "operations".into(),
            json!({
                "retry_handoff": retryable,
                // Re-executing a stage is a restart from that stage (W1), previewed first.
                "reexecute_stage": d.task.state,
            }),
        );
    }
    Ok(Json(v))
}

async fn retry_task(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(id): ApiPath<TaskId>,
) -> ApiResult<Json<Value>> {
    let scope = co::retry_task(&st.harness, &p, id).await?;
    Ok(Json(
        json!({"task": id, "project": scope.project, "dispatch_state": "pending"}),
    ))
}

#[derive(Deserialize)]
struct PlanQuery {
    workflow: String,
}

/// The execution plan and readiness for starting `workflow` in a project, from the project's
/// registered workspace and manifest (the same planner as `eaeh workflow run --plan`).
async fn project_plan(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiPath(project): ApiPath<ProjectId>,
    ApiQuery(q): ApiQuery<PlanQuery>,
) -> ApiResult<Json<Value>> {
    let scope = Scope::new(p.tenant, project);
    crate::routes::authorize(
        &st,
        &p,
        eaeh_governance::authz::Action::WorkflowRead,
        eaeh_governance::authz::Target::Scope(scope),
    )
    .await?;
    let (reg, manifest, issue) = crate::views::project_profile(&st, scope).await;
    let (Some(reg), Some(m)) = (reg, manifest) else {
        return Err(HarnessError::conflict(
            issue.unwrap_or_else(|| "the project has no registered workspace".into()),
        )
        .into());
    };
    let root = std::path::PathBuf::from(&reg.root);
    let plan = eaeh_runtime::project::plan_activity(&st.harness, &p, &m, &root, &q.workflow).await?;
    Ok(Json(
        json!({"project": project, "workspace": reg.root, "plan": plan, "configured_workflows": m.workflows}),
    ))
}

#[derive(Deserialize)]
struct ExecQuery {
    project_id: ProjectId,
    task: Option<String>,
    limit: Option<i64>,
}

/// Execution attempts and workers of a project (newest first).
async fn executions(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<ExecQuery>,
) -> ApiResult<Json<Value>> {
    let scope = Scope::new(p.tenant, q.project_id);
    if !p.can_access(&scope) {
        return Err(HarnessError::not_found("project not found").into());
    }
    let items = co::executions(
        &st.harness,
        &p,
        scope,
        q.task.as_deref(),
        q.limit.unwrap_or(50).clamp(1, 200),
    )
    .await?;
    let workers = co::workers(&st.harness, &p, scope).await?;
    let handoffs = st.harness.agents.external_tasks(&p, scope, false).await?;
    Ok(Json(
        json!({"items": items, "workers": workers, "handoffs": handoffs}),
    ))
}

#[derive(Deserialize)]
struct IntegrationsQuery {
    project_id: ProjectId,
}

/// Execution mode and the AI-client integration of a project: configured clients, the
/// generated client files in its workspace, and the worker status. No secrets.
async fn integrations(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<IntegrationsQuery>,
) -> ApiResult<Json<Value>> {
    let scope = Scope::new(p.tenant, q.project_id);
    crate::routes::authorize(
        &st,
        &p,
        eaeh_governance::authz::Action::WorkflowRead,
        eaeh_governance::authz::Target::Scope(scope),
    )
    .await?;
    let (reg, manifest, issue) = crate::views::project_profile(&st, scope).await;
    let files = reg.as_ref().map(|r| {
        let root = std::path::Path::new(&r.root);
        [
            ".mcp.json",
            "CLAUDE.md",
            ".vscode/mcp.json",
            ".github/copilot-instructions.md",
            ".eaeh/project.toml",
        ]
        .iter()
        .map(|f| json!({"file": f, "present": root.join(f).is_file()}))
        .collect::<Vec<_>>()
    });
    let workers = co::workers(&st.harness, &p, scope).await?;
    Ok(Json(json!({
        "installation_execution_mode": st.harness.execution_mode,
        "billing": st.harness.execution_mode.billing(),
        "project_execution": manifest.as_ref().and_then(|m| m.execution.as_ref()).map(|e| json!({"mode": e.mode, "clients": e.clients})),
        "workspace": reg.as_ref().map(|r| r.root.clone()),
        "client_files": files,
        "workers": workers,
        "live_worker": workers.iter().any(|w| w.live),
        "issue": issue,
        // Only the explicitly configured direct-provider mode calls a model API from EAEH.
        "direct_model_calls": st.harness.execution_mode == eaeh_runtime::ExecutionMode::DirectProvider,
    })))
}

#[derive(Deserialize)]
struct TaskQueueQuery {
    project_id: Option<ProjectId>,
    workflow: Option<String>,
    instance: Option<String>,
    stage: Option<String>,
    status: Option<String>,
    since: Option<String>,
    until: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

/// Tasks across the caller's projects (or one project), newest first, with filters by
/// workflow definition, run, stage, task status and date, and a keyset cursor
/// (`next_cursor`). Projects the caller may not read are listed as restricted.
async fn task_queue(
    State(st): St,
    Extension(p): Extension<Principal>,
    ApiQuery(q): ApiQuery<TaskQueueQuery>,
) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let filter = eaeh_workflow::TaskFilter {
        workflow: q.workflow.filter(|s| !s.is_empty()),
        instance: q.instance.filter(|s| !s.is_empty()),
        stage: q.stage.filter(|s| !s.is_empty()),
        status: q.status.filter(|s| !s.is_empty()),
        since: q.since.filter(|s| !s.is_empty()),
        until: q.until.filter(|s| !s.is_empty()),
        before: q.cursor.filter(|s| !s.is_empty()),
    };
    let projects = st.harness.identity.projects(p.tenant).await?;
    let mut items = vec![];
    let mut restricted = vec![];
    let mut found = false;
    for (pid, name) in projects {
        if q.project_id.is_some_and(|o| o != pid) {
            continue;
        }
        let scope = Scope::new(p.tenant, pid);
        if !p.can_access(&scope) {
            continue;
        }
        found = true;
        match st.harness.engine.task_list(&p, scope, &filter, limit).await {
            Ok(v) => items.extend(v.into_iter().map(|t| (pid, name.clone(), t))),
            Err(e) if e.kind == eaeh_core::ErrorKind::Forbidden => restricted.push(pid),
            Err(e) => return Err(e.into()),
        }
    }
    if q.project_id.is_some() && !found {
        return Err(HarnessError::not_found("project not found").into());
    }
    items.sort_by(|a, b| (&b.2.updated_at, &b.2.id).cmp(&(&a.2.updated_at, &a.2.id)));
    items.truncate(limit as usize);
    let next = (items.len() as u32 == limit)
        .then(|| items.last().map(|(_, _, t)| format!("{}|{}", t.updated_at, t.id)))
        .flatten();
    let items: Vec<Value> = items
        .into_iter()
        .map(|(pid, name, t)| json!({"project_id": pid, "project_name": name, "task": t}))
        .collect();
    Ok(Json(
        json!({"items": items, "next_cursor": next, "restricted_projects": restricted}),
    ))
}
