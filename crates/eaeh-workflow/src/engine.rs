//! Deterministic, durable workflow engine (authoritative half of hybrid orchestration).

use crate::definition::{
    Outcome, PlanProposal, StateDef, StateKind, Trigger, WorkflowDefinition, resolve_bindings,
};
use async_trait::async_trait;
use eaeh_core::{
    ApprovalId, ArtifactId, AutonomyLevel, HarnessError, Principal, PrincipalId, Result, Scope, TaskId,
    ToolId, TraceContext, Untrusted, WorkflowDefinitionId, WorkflowInstanceId, WorkspaceId,
};
use eaeh_events::catalog::{
    TraceLinksRecorded, ValidationFailed, WorkflowCompleted, WorkflowStarted, WorkflowTransitioned,
};
use eaeh_events::{DomainEvent, EventEnvelope, outbox};
use eaeh_extensibility::{HookContext, HookPayload, HookPoint, HookRegistry};
use eaeh_governance::authz::{Action, Authorizer, Target};
use eaeh_governance::policy::{Effect, PolicyEngine, PolicyRequest};
use eaeh_governance::{
    ApprovalDecision, ApprovalManager, ApprovalRequest, ApprovalStatus, ApprovalSubject, BudgetLedger,
};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, WriteTx, from_json, sqlx, to_json, ts};
use eaeh_validation::{QualityGate, ValidationEngine, ValidationSubject};
use eaeh_workspace::{ArtifactStore, Provenance, Workspace, WorkspaceManager};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const ENGINE_COMPONENT: &str = "workflow-engine";

/// What a work state asks the executor to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum TaskSpec {
    Agent {
        task_kind: String,
        capabilities: BTreeSet<String>,
        max_autonomy: AutonomyLevel,
    },
    Tool {
        tool: ToolId,
        input: Value,
    },
}

#[derive(Debug, Clone)]
pub struct TaskExecution {
    pub task_id: TaskId,
    pub instance: WorkflowInstanceId,
    pub state: String,
    pub spec: TaskSpec,
    pub input: Value,
    pub scope: Scope,
    pub trace: TraceContext,
    pub budget_owner: String,
    pub workspace: Workspace,
    pub cancel: CancellationToken,
}

#[derive(Debug, Clone)]
pub struct TaskOutput {
    pub output: Value,
    /// Principals that produced the output (excluded from approving it).
    pub authors: Vec<PrincipalId>,
    pub provenance: Provenance,
}

/// Executes work states (implemented by the composition root using the agent runtime
/// and tool gateway). The engine never calls models or tools directly.
#[async_trait]
pub trait TaskExecutor: Send + Sync {
    async fn execute(&self, task: TaskExecution) -> Result<TaskOutput>;
}

#[derive(Debug, Clone)]
pub struct PlanningContext {
    pub instance: WorkflowInstanceId,
    pub state: String,
    pub scope: Scope,
    pub trace: TraceContext,
    pub input: Value,
    pub allowed_task_kinds: Vec<String>,
    pub max_steps: u32,
    pub budget_owner: String,
    pub workspace: Workspace,
}

/// Intelligent planning (advisory). Its proposal is untrusted and validated by the engine.
#[async_trait]
pub trait Planner: Send + Sync {
    async fn propose(&self, ctx: PlanningContext) -> Result<Untrusted<PlanProposal>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    Running,
    WaitingApproval,
    /// An agent task was handed off to an external AI client (`external_ai` mode).
    WaitingExternal,
    Succeeded,
    Failed,
    Cancelled,
    Escalated,
}

impl InstanceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            InstanceStatus::Running => "running",
            InstanceStatus::WaitingApproval => "waiting_approval",
            InstanceStatus::WaitingExternal => "waiting_external",
            InstanceStatus::Succeeded => "succeeded",
            InstanceStatus::Failed => "failed",
            InstanceStatus::Cancelled => "cancelled",
            InstanceStatus::Escalated => "escalated",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "running" => Self::Running,
            "waiting_approval" => Self::WaitingApproval,
            "waiting_external" => Self::WaitingExternal,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "escalated" => Self::Escalated,
            _ => return Err(HarnessError::integrity("unknown workflow status")),
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Escalated
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: ArtifactId,
    pub name: String,
    pub version: i64,
    pub hash: String,
}

/// Durable workflow memory (persisted with every checkpoint).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkflowData {
    #[serde(default)]
    pub outputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub artifacts: BTreeMap<String, ArtifactRef>,
    #[serde(default)]
    pub authors: BTreeMap<String, Vec<PrincipalId>>,
    #[serde(default)]
    pub visits: BTreeMap<String, u32>,
    #[serde(default)]
    pub current_approval: BTreeMap<String, ApprovalId>,
    #[serde(default)]
    pub approved_gates: BTreeMap<String, ApprovalId>,
    #[serde(default)]
    pub feedback: BTreeMap<String, String>,
    /// `request_changes` decisions honoured per gate (Phase 3).
    #[serde(default)]
    pub revisions: BTreeMap<String, u32>,
    /// Quality-gate results per state (Phase 3).
    #[serde(default)]
    pub quality: BTreeMap<String, Value>,
    #[serde(default)]
    pub plans: BTreeMap<String, PlanProposal>,
    #[serde(default)]
    pub plan_results: BTreeMap<String, Vec<Value>>,
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub failure: Option<String>,
    /// Work state whose agent task is handed off to an external AI client. While set,
    /// re-entering that state resumes the same visit (same durable task) instead of
    /// starting a new one.
    #[serde(default)]
    pub awaiting_external: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInstance {
    pub id: WorkflowInstanceId,
    pub definition_id: String,
    pub definition_version: String,
    pub scope: Scope,
    pub state: String,
    pub status: InstanceStatus,
    pub version: i64,
    pub input: Value,
    pub data: WorkflowData,
    pub started_by: String,
    pub correlation_id: String,
    pub cancel_requested: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl WorkflowInstance {
    pub fn trace(&self) -> TraceContext {
        TraceContext {
            correlation: self.correlation_id.parse().unwrap_or_default(),
            causation: Some(self.id.as_uuid()),
        }
    }
    pub fn budget_owner(&self) -> String {
        format!("wf:{}", self.id)
    }
}

/// Row shape of `trace_links` as selected by [`WorkflowEngine::trace`].
type TraceRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
);

/// A recorded traceability link with the approved artifact it came from (Phase 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceLinkRecord {
    pub from_kind: String,
    pub from_id: String,
    pub relation: String,
    pub to_kind: String,
    pub to_id: String,
    pub artifact_id: String,
    pub artifact_name: String,
    pub artifact_version: i64,
    pub artifact_hash: String,
    pub workflow_instance_id: String,
    pub approval_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransitionRecord {
    pub seq: i64,
    pub from_state: String,
    pub to_state: String,
    pub trigger: String,
    pub actor_id: String,
    pub occurred_at: String,
}

pub struct WorkflowEngine {
    db: Db,
    authz: Arc<dyn Authorizer>,
    policy: Arc<dyn PolicyEngine>,
    approvals: ApprovalManager,
    budgets: BudgetLedger,
    hooks: Arc<HookRegistry>,
    executor: Arc<dyn TaskExecutor>,
    planner: Arc<dyn Planner>,
    artifacts: ArtifactStore,
    workspaces: WorkspaceManager,
    validation: Option<Arc<ValidationEngine>>,
    gates: BTreeMap<String, QualityGate>,
    locks: Mutex<HashMap<WorkflowInstanceId, Arc<tokio::sync::Mutex<()>>>>,
    cancels: Mutex<HashMap<WorkflowInstanceId, CancellationToken>>,
}

impl std::fmt::Debug for WorkflowEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowEngine").finish_non_exhaustive()
    }
}

fn engine_actor() -> String {
    format!("system:{ENGINE_COMPONENT}")
}

#[allow(clippy::too_many_arguments)]
impl WorkflowEngine {
    pub fn new(
        db: Db,
        authz: Arc<dyn Authorizer>,
        policy: Arc<dyn PolicyEngine>,
        approvals: ApprovalManager,
        budgets: BudgetLedger,
        hooks: Arc<HookRegistry>,
        executor: Arc<dyn TaskExecutor>,
        planner: Arc<dyn Planner>,
        artifacts: ArtifactStore,
        workspaces: WorkspaceManager,
    ) -> Self {
        Self {
            db,
            authz,
            policy,
            approvals,
            budgets,
            hooks,
            executor,
            planner,
            artifacts,
            workspaces,
            validation: None,
            gates: BTreeMap::new(),
            locks: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
        }
    }

    /// Enable quality gates on work states (Phase 3). Gates are platform configuration;
    /// workflow definitions can only reference them by id.
    pub fn with_quality_gates(mut self, validation: Arc<ValidationEngine>, gates: Vec<QualityGate>) -> Self {
        self.validation = Some(validation);
        self.gates = gates.into_iter().map(|g| (g.id.clone(), g)).collect();
        self
    }

    pub fn quality_gates(&self) -> Vec<&QualityGate> {
        self.gates.values().collect()
    }

    pub fn approvals(&self) -> &ApprovalManager {
        &self.approvals
    }

    pub fn artifacts(&self) -> &ArtifactStore {
        &self.artifacts
    }

    // ------------------------------------------------------------------ definitions

    pub async fn register_definition(
        &self,
        actor: Option<&Principal>,
        def: &WorkflowDefinition,
    ) -> Result<()> {
        if let Some(a) = actor {
            self.authz
                .authorize(a, Action::WorkflowDefine, Target::Platform)?;
        }
        def.validate()?;
        for s in &def.states {
            if let Some(g) = &s.quality_gate
                && !self.gates.contains_key(&g.gate)
            {
                return Err(HarnessError::invalid_input(format!(
                    "workflow {}: state '{}' references unknown quality gate '{}'",
                    def.reference(),
                    s.name,
                    g.gate
                )));
            }
        }
        let digest = eaeh_core::hash::canonical_json_hash(&serde_json::to_value(def)?);
        let existing: Option<(String,)> =
            sqlx::query_as("SELECT digest FROM workflow_definitions WHERE id = ? AND version = ?")
                .bind(def.id.as_str())
                .bind(def.version.to_string())
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        match existing {
            Some((d,)) if d == digest => return Ok(()),
            Some(_) => {
                return Err(HarnessError::conflict(format!(
                    "workflow {} already registered with different content",
                    def.reference()
                )));
            }
            None => {}
        }
        let mut tx = self.db.write().await?;
        sqlx::query("INSERT INTO workflow_definitions (id, version, document, digest, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(def.id.as_str())
            .bind(def.version.to_string())
            .bind(to_json(def)?)
            .bind(&digest)
            .bind(ts(self.db.now()))
            .execute(tx.conn())
            .await
            .db()?;
        let entry = match actor {
            Some(a) => AuditEntry::new(
                a,
                "workflow.define",
                "workflow_definition",
                def.reference(),
                AuditOutcome::Succeeded,
            ),
            None => AuditEntry::system(
                "workflow-loader",
                "workflow.define",
                "workflow_definition",
                def.reference(),
                AuditOutcome::Succeeded,
            ),
        };
        self.db
            .audit_in(&mut tx, entry.details(json!({"digest": digest})))
            .await?;
        tx.commit().await
    }

    pub async fn definition(&self, id: &str, version: &str) -> Result<WorkflowDefinition> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT document FROM workflow_definitions WHERE id = ? AND version = ?")
                .bind(id)
                .bind(version)
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        from_json(
            &row.ok_or_else(|| HarnessError::not_found(format!("workflow definition {id}@{version}")))?
                .0,
        )
    }

    pub async fn latest_definition(&self, id: &WorkflowDefinitionId) -> Result<WorkflowDefinition> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT document FROM workflow_definitions WHERE id = ?")
            .bind(id.as_str())
            .fetch_all(self.db.pool())
            .await
            .db()?;
        rows.iter()
            .map(|(d,)| from_json::<WorkflowDefinition>(d))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .max_by(|a, b| a.version.cmp(&b.version))
            .ok_or_else(|| HarnessError::not_found(format!("workflow definition {id}")))
    }

    pub async fn definitions(&self) -> Result<Vec<WorkflowDefinition>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT document FROM workflow_definitions ORDER BY id, created_at")
                .fetch_all(self.db.pool())
                .await
                .db()?;
        rows.iter().map(|(d,)| from_json(d)).collect()
    }

    // ------------------------------------------------------------------ instances

    pub async fn start(
        &self,
        actor: &Principal,
        definition: &WorkflowDefinitionId,
        scope: Scope,
        input: Value,
        trace: TraceContext,
    ) -> Result<WorkflowInstanceId> {
        if let Err(e) = self
            .authz
            .authorize(actor, Action::WorkflowStart, Target::Scope(scope))
        {
            self.db
                .audit(
                    AuditEntry::new(
                        actor,
                        "workflow.start",
                        "workflow_definition",
                        definition.to_string(),
                        AuditOutcome::Denied,
                    )
                    .scope(scope)
                    .correlation(trace.correlation)
                    .details(json!({"reason": e.to_string()})),
                )
                .await?;
            return Err(e);
        }
        let def = self.latest_definition(definition).await?;
        let id = WorkflowInstanceId::new();
        let ws = self.workspaces.create(scope, &actor.id).await?;
        let data = WorkflowData {
            workspace_id: Some(ws.id),
            ..Default::default()
        };
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO workflow_instances (id, definition_id, definition_version, tenant_id, project_id, state, status, version, input, data, \
             started_by, correlation_id, cancel_requested, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'running', 1, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(id.to_string())
        .bind(def.id.as_str())
        .bind(def.version.to_string())
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(&def.initial)
        .bind(to_json(&input)?)
        .bind(to_json(&data)?)
        .bind(actor.id.as_str())
        .bind(trace.correlation.to_string())
        .bind(&now)
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        self.budgets
            .init_in(&mut tx, &format!("wf:{id}"), &def.budget)
            .await?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "workflow.start",
                    "workflow_instance",
                    id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .scope(scope)
                .correlation(trace.correlation)
                .details(json!({"definition": def.reference()})),
            )
            .await?;
        emit(
            &self.db,
            &mut tx,
            &WorkflowStarted {
                instance_id: id.to_string(),
                definition_id: def.id.to_string(),
                definition_version: def.version.to_string(),
            },
            scope,
            trace,
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn get(&self, id: WorkflowInstanceId) -> Result<WorkflowInstance> {
        let row = sqlx::query("SELECT * FROM workflow_instances WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.db.pool())
            .await
            .db()?
            .ok_or_else(|| HarnessError::not_found("workflow instance not found"))?;
        row_to_instance(&row)
    }

    /// Read access for a principal (authorization + scope).
    pub async fn get_for(&self, actor: &Principal, id: WorkflowInstanceId) -> Result<WorkflowInstance> {
        let inst = self.get(id).await?;
        self.authz
            .authorize(actor, Action::WorkflowRead, Target::Scope(inst.scope))
            .map_err(|_| HarnessError::not_found("workflow instance not found"))?;
        Ok(inst)
    }

    pub async fn list(
        &self,
        actor: &Principal,
        scope: Scope,
        before: Option<WorkflowInstanceId>,
        limit: u32,
    ) -> Result<Vec<WorkflowInstance>> {
        self.authz
            .authorize(actor, Action::WorkflowRead, Target::Scope(scope))?;
        let rows = sqlx::query(
            "SELECT * FROM workflow_instances WHERE tenant_id = ? AND project_id = ? AND (? IS NULL OR id < ?) ORDER BY id DESC LIMIT ?",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(before.map(|b| b.to_string()))
        .bind(before.map(|b| b.to_string()))
        .bind(i64::from(limit.clamp(1, 100)))
        .fetch_all(self.db.pool())
        .await
        .db()?;
        rows.iter().map(row_to_instance).collect()
    }

    pub async fn history(&self, id: WorkflowInstanceId) -> Result<Vec<TransitionRecord>> {
        let rows: Vec<(i64, String, String, String, String, String)> = sqlx::query_as(
            "SELECT seq, from_state, to_state, trigger, actor_id, occurred_at FROM workflow_transitions WHERE instance_id = ? ORDER BY seq",
        )
        .bind(id.to_string())
        .fetch_all(self.db.pool())
        .await
        .db()?;
        Ok(rows
            .into_iter()
            .map(
                |(seq, from_state, to_state, trigger, actor_id, occurred_at)| TransitionRecord {
                    seq,
                    from_state,
                    to_state,
                    trigger,
                    actor_id,
                    occurred_at,
                },
            )
            .collect())
    }

    fn instance_lock(&self, id: WorkflowInstanceId) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .lock()
            .map(|mut m| m.entry(id).or_default().clone())
            .unwrap_or_default()
    }

    fn cancel_token(&self, id: WorkflowInstanceId) -> CancellationToken {
        self.cancels
            .lock()
            .map(|mut m| m.entry(id).or_default().clone())
            .unwrap_or_default()
    }

    /// Advance an instance until it completes or blocks on an approval.
    /// Runs inside a `workflow` span carrying instance and correlation ids.
    pub async fn drive(&self, id: WorkflowInstanceId) -> Result<InstanceStatus> {
        use tracing::Instrument;
        let correlation = self.get(id).await.map(|i| i.correlation_id).unwrap_or_default();
        let span = tracing::info_span!("workflow", instance = %id, correlation_id = %correlation);
        self.drive_inner(id).instrument(span).await
    }

    async fn drive_inner(&self, id: WorkflowInstanceId) -> Result<InstanceStatus> {
        let lock = self.instance_lock(id);
        let _g = lock.lock().await;
        for _ in 0..1000 {
            let inst = self.get(id).await?;
            if inst.status.is_terminal() {
                return Ok(inst.status);
            }
            if inst.cancel_requested {
                self.finish(&inst, InstanceStatus::Cancelled, "cancelled", None)
                    .await?;
                return Ok(InstanceStatus::Cancelled);
            }
            let def = self
                .definition(&inst.definition_id, &inst.definition_version)
                .await?;
            let state = def.state(&inst.state)?.clone();
            match &state.kind {
                StateKind::Terminal { outcome } => {
                    let status = match outcome {
                        Outcome::Succeeded => InstanceStatus::Succeeded,
                        Outcome::Failed => InstanceStatus::Failed,
                        Outcome::Cancelled => InstanceStatus::Cancelled,
                    };
                    self.finish(&inst, status, &inst.state, None).await?;
                    return Ok(status);
                }
                StateKind::ApprovalGate {
                    required_role,
                    artifact_from,
                    max_revisions,
                } => {
                    if let Some(aid) = inst.data.current_approval.get(&state.name).copied() {
                        let rec = self.approvals.get(aid).await?;
                        match rec.status {
                            ApprovalStatus::Pending => {
                                self.set_status(&inst, InstanceStatus::WaitingApproval).await?;
                                return Ok(InstanceStatus::WaitingApproval);
                            }
                            ApprovalStatus::Approved => {
                                self.record_trace_links(&inst, artifact_from, aid).await?;
                                let mut data = inst.data.clone();
                                data.current_approval.remove(&state.name);
                                data.approved_gates.insert(state.name.clone(), aid);
                                data.feedback.remove(&state.name);
                                self.transition(
                                    &inst,
                                    &state,
                                    Trigger::Approved,
                                    data,
                                    &rec.decision.map(|d| d.approver_id).unwrap_or_default(),
                                )
                                .await?;
                            }
                            ApprovalStatus::Rejected => {
                                let mut data = inst.data.clone();
                                data.current_approval.remove(&state.name);
                                data.approved_gates.remove(&state.name);
                                let d = rec.decision.unwrap_or(eaeh_governance::approval::DecisionRecord {
                                    approver_id: String::new(),
                                    approver_role: String::new(),
                                    decision: "reject".into(),
                                    comments: String::new(),
                                    artifact_hash: None,
                                    decided_at: String::new(),
                                });
                                data.feedback.insert(state.name.clone(), d.comments.clone());
                                self.hooks
                                    .dispatch(&self.wf_hook(
                                        HookPoint::OnRejection,
                                        &inst,
                                        Some(&state.name),
                                        "rejected",
                                    ))
                                    .await;
                                self.transition(&inst, &state, Trigger::Rejected, data, &d.approver_id)
                                    .await?;
                            }
                            ApprovalStatus::ChangesRequested => {
                                let mut data = inst.data.clone();
                                data.current_approval.remove(&state.name);
                                data.approved_gates.remove(&state.name);
                                let d = rec.decision.unwrap_or(eaeh_governance::approval::DecisionRecord {
                                    approver_id: String::new(),
                                    approver_role: String::new(),
                                    decision: "request_changes".into(),
                                    comments: String::new(),
                                    artifact_hash: None,
                                    decided_at: String::new(),
                                });
                                let used = data.revisions.get(&state.name).copied().unwrap_or(0);
                                let (trigger, note) = if used >= *max_revisions {
                                    (
                                        Trigger::Rejected,
                                        format!(
                                            "revision limit ({max_revisions}) reached; changes requested: {}",
                                            d.comments
                                        ),
                                    )
                                } else {
                                    data.revisions.insert(state.name.clone(), used + 1);
                                    let t = if state.transitions.contains_key(&Trigger::ChangesRequested) {
                                        Trigger::ChangesRequested
                                    } else {
                                        Trigger::Rejected
                                    };
                                    (t, d.comments.clone())
                                };
                                data.feedback.insert(state.name.clone(), note.clone());
                                self.db
                                    .audit(
                                        AuditEntry::system(
                                            ENGINE_COMPONENT,
                                            "workflow.revision",
                                            "workflow_instance",
                                            inst.id.to_string(),
                                            if trigger == Trigger::Rejected {
                                                AuditOutcome::Denied
                                            } else {
                                                AuditOutcome::Recorded
                                            },
                                        )
                                        .scope(inst.scope)
                                        .correlation(inst.trace().correlation)
                                        .details(json!({
                                            "gate": state.name,
                                            "revision": used + 1,
                                            "max_revisions": max_revisions,
                                            "limit_reached": trigger == Trigger::Rejected,
                                        })),
                                    )
                                    .await?;
                                self.hooks
                                    .dispatch(&self.wf_hook(
                                        HookPoint::OnRejection,
                                        &inst,
                                        Some(&state.name),
                                        "changes_requested",
                                    ))
                                    .await;
                                self.transition(&inst, &state, trigger, data, &d.approver_id)
                                    .await?;
                            }
                            ApprovalStatus::Superseded => {
                                let mut data = inst.data.clone();
                                data.current_approval.remove(&state.name);
                                self.save_data(&inst, &data).await?;
                            }
                        }
                    } else {
                        let art = inst.data.artifacts.get(artifact_from).cloned().ok_or_else(|| {
                            HarnessError::integrity(format!(
                                "gate '{}' has no artifact from '{artifact_from}'",
                                state.name
                            ))
                        })?;
                        // Everyone who produced work in this run so far is an author of what the
                        // gate approves: a rendered bundle is built from earlier outputs, and a
                        // person who submitted an external AI result for one of them must not
                        // approve it (separation of duties, ADR-0025).
                        let mut authors = inst.data.authors.get(artifact_from).cloned().unwrap_or_default();
                        for a in inst.data.authors.values().flatten() {
                            if !authors.contains(a) {
                                authors.push(a.clone());
                            }
                        }
                        let mut tx = self.db.write().await?;
                        let aid = self
                            .approvals
                            .request_in(
                                &mut tx,
                                ApprovalRequest {
                                    scope: inst.scope,
                                    subject: ApprovalSubject::WorkflowGate {
                                        instance: inst.id,
                                        gate: state.name.clone(),
                                        artifact: Some(art.id),
                                        artifact_hash: Some(art.hash.clone()),
                                    },
                                    required_role: *required_role,
                                    authors,
                                    requested_by: PrincipalId::new(engine_actor())?,
                                    trace: inst.trace(),
                                },
                            )
                            .await?;
                        let mut data = inst.data.clone();
                        data.current_approval.insert(state.name.clone(), aid);
                        self.update_in(&mut tx, &inst, None, Some(InstanceStatus::WaitingApproval), &data)
                            .await?;
                        tx.commit().await?;
                        return Ok(InstanceStatus::WaitingApproval);
                    }
                }
                _ => {
                    let trigger = self.run_work(&inst, &state).await?;
                    let fresh = self.get(id).await?;
                    if fresh.status.is_terminal() {
                        return Ok(fresh.status);
                    }
                    match trigger {
                        Some(t) => {
                            if state.transitions.contains_key(&t) {
                                self.transition(&fresh, &state, t, fresh.data.clone(), &engine_actor())
                                    .await?;
                            } else {
                                let reason = fresh
                                    .data
                                    .failure
                                    .clone()
                                    .unwrap_or_else(|| format!("{} without transition", t.as_str()));
                                self.finish(&fresh, InstanceStatus::Failed, &fresh.state, Some(reason))
                                    .await?;
                                return Ok(InstanceStatus::Failed);
                            }
                        }
                        None => return Ok(self.get(id).await?.status),
                    }
                }
            }
        }
        Err(HarnessError::supervision_halt(
            "workflow exceeded 1000 transitions in one drive (loop?)",
        ))
    }

    /// Execute a work state (with gate checks, retries, timeout, cancellation).
    async fn run_work(&self, inst: &WorkflowInstance, state: &StateDef) -> Result<Option<Trigger>> {
        // Mandatory gate check, independent of the transition graph.
        for gate in &state.requires_approval_of {
            let ok = match inst.data.approved_gates.get(gate) {
                Some(aid) => {
                    let rec = self.approvals.get(*aid).await?;
                    rec.status == ApprovalStatus::Approved
                        && rec.subject_ref == format!("{}:{gate}", inst.id)
                        && rec.scope == inst.scope
                }
                None => false,
            };
            if !ok {
                let reason = format!(
                    "state '{}' requires approval of gate '{gate}', which has not been granted",
                    state.name
                );
                self.db
                    .audit(
                        AuditEntry::system(
                            ENGINE_COMPONENT,
                            "workflow.gate_check",
                            "workflow_instance",
                            inst.id.to_string(),
                            AuditOutcome::Denied,
                        )
                        .scope(inst.scope)
                        .correlation(inst.trace().correlation)
                        .details(json!({"state": state.name, "gate": gate})),
                    )
                    .await?;
                self.finish(inst, InstanceStatus::Failed, &inst.state, Some(reason))
                    .await?;
                return Ok(None);
            }
        }
        let resuming = inst.data.awaiting_external.as_deref() == Some(state.name.as_str());
        let visit = inst.data.visits.get(&state.name).copied().unwrap_or(0) + u32::from(!resuming);
        let mut data = inst.data.clone();
        data.visits.insert(state.name.clone(), visit.max(1));
        data.awaiting_external = None;
        self.save_data(inst, &data).await?;
        let inst = self.get(inst.id).await?;
        let ws_id = inst
            .data
            .workspace_id
            .ok_or_else(|| HarnessError::integrity("instance has no workspace"))?;
        let workspace = self.workspaces.open(ws_id, inst.scope).await?;
        let cancel = self.cancel_token(inst.id);

        let mut attempt = 1u32;
        loop {
            let res = self
                .run_once(&inst, state, visit, attempt, &workspace, &cancel)
                .await;
            match res {
                Ok(()) => return Ok(Some(Trigger::Succeeded)),
                Err(e) if e.kind == eaeh_core::ErrorKind::AwaitingExternal => {
                    // Durable suspension, not a failure: the same task resumes on the next
                    // drive (after the external client submits its result).
                    let fresh = self.get(inst.id).await?;
                    let mut data = fresh.data.clone();
                    data.awaiting_external = Some(state.name.clone());
                    let mut tx = self.db.write().await?;
                    self.update_in(
                        &mut tx,
                        &fresh,
                        None,
                        Some(InstanceStatus::WaitingExternal),
                        &data,
                    )
                    .await?;
                    self.db
                        .audit_in(
                            &mut tx,
                            AuditEntry::system(
                                ENGINE_COMPONENT,
                                "workflow.await_external",
                                "workflow_instance",
                                inst.id.to_string(),
                                AuditOutcome::Recorded,
                            )
                            .scope(inst.scope)
                            .correlation(inst.trace().correlation)
                            .details(json!({"state": state.name, "visit": visit, "reason": e.message})),
                        )
                        .await?;
                    tx.commit().await?;
                    return Ok(None);
                }
                Err(e) if e.kind == eaeh_core::ErrorKind::Cancelled => {
                    self.hooks
                        .dispatch(&self.wf_hook(
                            HookPoint::OnCancellation,
                            &inst,
                            Some(&state.name),
                            "cancelled",
                        ))
                        .await;
                    let fresh = self.get(inst.id).await?;
                    self.finish(&fresh, InstanceStatus::Cancelled, &fresh.state, Some(e.message))
                        .await?;
                    return Ok(None);
                }
                Err(e) => {
                    let timed_out = e.kind == eaeh_core::ErrorKind::Timeout;
                    let point = if timed_out {
                        HookPoint::OnTimeout
                    } else {
                        HookPoint::OnFailure
                    };
                    self.hooks
                        .dispatch(&self.wf_hook(point, &inst, Some(&state.name), &e.to_string()))
                        .await;
                    let retryable = e.is_retryable() || timed_out;
                    if retryable && attempt < state.retry.max_attempts {
                        attempt += 1;
                        tokio::time::sleep(Duration::from_millis(
                            state.retry.backoff_ms.saturating_mul(u64::from(attempt)),
                        ))
                        .await;
                        continue;
                    }
                    let fresh = self.get(inst.id).await?;
                    let mut data = fresh.data.clone();
                    data.failure = Some(format!("{}: {e}", state.name));
                    self.save_data(&fresh, &data).await?;
                    if matches!(
                        e.kind,
                        eaeh_core::ErrorKind::SupervisionHalt | eaeh_core::ErrorKind::BudgetExceeded
                    ) && !state.transitions.contains_key(&Trigger::Failed)
                    {
                        self.finish(
                            &self.get(inst.id).await?,
                            InstanceStatus::Escalated,
                            &state.name,
                            Some(e.to_string()),
                        )
                        .await?;
                        return Ok(None);
                    }
                    return Ok(Some(if timed_out {
                        Trigger::TimedOut
                    } else {
                        Trigger::Failed
                    }));
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_once(
        &self,
        inst: &WorkflowInstance,
        state: &StateDef,
        visit: u32,
        attempt: u32,
        workspace: &Workspace,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let timeout = Duration::from_secs(state.timeout_secs.unwrap_or(3600));
        let artifacts = |s: &str| {
            inst.data
                .artifacts
                .get(s)
                .map(|a| json!({"id": a.id, "name": a.name, "version": a.version, "hash": a.hash}))
        };
        let mut input = json!({
            "workflow_input": inst.input,
            "outputs": inst.data.outputs,
            "revision_feedback": inst.data.feedback,
            "visit": visit,
        });
        if let StateKind::AgentTask { inputs, .. } = &state.kind
            && !inputs.is_empty()
        {
            let mut resolved = serde_json::Map::new();
            for (k, b) in inputs {
                resolved.insert(
                    k.clone(),
                    resolve_bindings(b, &inst.input, &inst.data.outputs, &artifacts)?,
                );
            }
            input["inputs"] = Value::Object(resolved);
        }
        let fut = async {
            match &state.kind {
                StateKind::AgentTask {
                    task_kind,
                    capabilities,
                    max_autonomy,
                    ..
                } => {
                    let spec = TaskSpec::Agent {
                        task_kind: task_kind.clone(),
                        capabilities: capabilities.clone(),
                        max_autonomy: *max_autonomy,
                    };
                    let key = format!("{}:{}:{visit}:{attempt}", inst.id, state.name);
                    let out = self
                        .run_task(inst, state, &key, spec, input.clone(), workspace, cancel)
                        .await?;
                    self.record_output(inst, state, out.output.clone(), out).await
                }
                StateKind::ToolStep {
                    tool,
                    input: tool_input,
                    ..
                } => {
                    let tool_input =
                        &resolve_bindings(tool_input, &inst.input, &inst.data.outputs, &artifacts)?;
                    let spec = TaskSpec::Tool {
                        tool: tool.clone(),
                        input: tool_input.clone(),
                    };
                    let key = format!("{}:{}:{visit}:{attempt}", inst.id, state.name);
                    let out = self
                        .run_task(inst, state, &key, spec, tool_input.clone(), workspace, cancel)
                        .await?;
                    self.record_output(inst, state, out.output.clone(), out).await
                }
                StateKind::Planned {
                    allowed_task_kinds,
                    max_steps,
                    max_autonomy,
                    ..
                } => {
                    self.run_planned(
                        inst,
                        state,
                        visit,
                        attempt,
                        allowed_task_kinds,
                        *max_steps,
                        *max_autonomy,
                        input.clone(),
                        workspace,
                        cancel,
                    )
                    .await
                }
                _ => Err(HarnessError::internal("not a work state")),
            }
        };
        tokio::select! {
            r = tokio::time::timeout(timeout, fut) => match r {
                Ok(r) => r,
                Err(_) => Err(HarnessError::timeout(format!("state '{}' exceeded {timeout:?}", state.name))),
            },
            _ = cancel.cancelled() => Err(HarnessError::cancelled("workflow cancelled")),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_planned(
        &self,
        inst: &WorkflowInstance,
        state: &StateDef,
        visit: u32,
        attempt: u32,
        allowed: &[String],
        max_steps: u32,
        max_autonomy: AutonomyLevel,
        input: Value,
        workspace: &Workspace,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let plan_key = format!("{}#{visit}", state.name);
        let fresh = self.get(inst.id).await?;
        let plan = match fresh.data.plans.get(&plan_key) {
            Some(p) => p.clone(),
            None => {
                let proposal = self
                    .planner
                    .propose(PlanningContext {
                        instance: inst.id,
                        state: state.name.clone(),
                        scope: inst.scope,
                        trace: inst.trace(),
                        input: input.clone(),
                        allowed_task_kinds: allowed.to_vec(),
                        max_steps,
                        budget_owner: inst.budget_owner(),
                        workspace: workspace.clone(),
                    })
                    .await?;
                let plan = self
                    .validate_plan(&fresh, state, proposal, allowed, max_steps)
                    .await?;
                let mut data = fresh.data.clone();
                data.plans.insert(plan_key.clone(), plan.clone());
                self.save_data(&fresh, &data).await?;
                plan
            }
        };
        let mut results = self
            .get(inst.id)
            .await?
            .data
            .plan_results
            .get(&plan_key)
            .cloned()
            .unwrap_or_default();
        let mut authors = vec![];
        let mut prov = Provenance {
            produced_by: engine_actor(),
            ..Default::default()
        };
        for (i, step) in plan.steps.iter().enumerate().skip(results.len()) {
            let spec = TaskSpec::Agent {
                task_kind: step.task_kind.clone(),
                capabilities: step.capabilities.clone(),
                max_autonomy,
            };
            let key = format!("{}:{}:{visit}:step{i}:{attempt}", inst.id, state.name);
            let step_input = json!({"step": step.input, "context": input, "previous_steps": results});
            let out = self
                .run_task(inst, state, &key, spec, step_input, workspace, cancel)
                .await?;
            authors.extend(out.authors.iter().cloned());
            prov.tool_invocations
                .extend(out.provenance.tool_invocations.iter().cloned());
            prov.models.extend(out.provenance.models.iter().cloned());
            prov.prompts.extend(out.provenance.prompts.iter().cloned());
            prov.skills.extend(out.provenance.skills.iter().cloned());
            prov.knowledge.extend(out.provenance.knowledge.iter().cloned());
            results.push(out.output);
            let f = self.get(inst.id).await?;
            let mut data = f.data.clone();
            data.plan_results.insert(plan_key.clone(), results.clone());
            self.save_data(&f, &data).await?;
        }
        let output = json!({"plan": plan, "results": results});
        self.record_output(
            inst,
            state,
            output,
            TaskOutput {
                output: Value::Null,
                authors,
                provenance: prov,
            },
        )
        .await
    }

    async fn validate_plan(
        &self,
        inst: &WorkflowInstance,
        state: &StateDef,
        proposal: Untrusted<PlanProposal>,
        allowed: &[String],
        max_steps: u32,
    ) -> Result<PlanProposal> {
        let plan = proposal.into_data();
        let mut problems = vec![];
        if plan.steps.is_empty() {
            problems.push("plan has no steps".to_string());
        }
        if plan.steps.len() > max_steps as usize {
            problems.push(format!("plan has {} steps (max {max_steps})", plan.steps.len()));
        }
        let mut matched = vec![];
        for (i, s) in plan.steps.iter().enumerate() {
            if !allowed.contains(&s.task_kind) {
                problems.push(format!("step {i}: task kind '{}' not allowed", s.task_kind));
            }
            let d = self.policy.evaluate(&PolicyRequest {
                action: "workflow.plan_step".into(),
                principal_id: engine_actor(),
                principal_kind: "service".into(),
                autonomy: None,
                risk: Some(s.risk),
                resource_type: "plan_step".into(),
                resource_id: s.task_kind.clone(),
                scope: Some(inst.scope),
                attributes: Default::default(),
            });
            matched.extend(d.matched_rules.clone());
            if d.effect != Effect::Permit {
                problems.push(format!("step {i}: policy {}: {}", d.effect_str(), d.reason));
            }
        }
        let accepted = problems.is_empty();
        let mut tx = self.db.write().await?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    ENGINE_COMPONENT,
                    if accepted {
                        "plan.accepted"
                    } else {
                        "plan.rejected"
                    },
                    "workflow_instance",
                    inst.id.to_string(),
                    if accepted {
                        AuditOutcome::Allowed
                    } else {
                        AuditOutcome::Denied
                    },
                )
                .scope(inst.scope)
                .correlation(inst.trace().correlation)
                .details(
                    json!({"state": state.name, "plan": plan, "problems": problems, "policy_rules": matched}),
                ),
            )
            .await?;
        if !accepted {
            emit(
                &self.db,
                &mut tx,
                &ValidationFailed {
                    gate_id: format!("plan:{}", state.name),
                    subject: inst.id.to_string(),
                    findings: u32::try_from(problems.len()).unwrap_or(u32::MAX),
                },
                inst.scope,
                inst.trace(),
            )
            .await?;
        }
        tx.commit().await?;
        if accepted {
            Ok(plan)
        } else {
            Err(HarnessError::policy_denied(format!(
                "planner proposal rejected: {}",
                problems.join("; ")
            )))
        }
    }

    /// Run (or replay) one task with idempotency.
    #[allow(clippy::too_many_arguments)]
    async fn run_task(
        &self,
        inst: &WorkflowInstance,
        state: &StateDef,
        key: &str,
        spec: TaskSpec,
        input: Value,
        workspace: &Workspace,
        cancel: &CancellationToken,
    ) -> Result<TaskOutput> {
        // Idempotency: a succeeded task with this key is never executed again.
        let existing: Option<(String, String, Option<String>)> =
            sqlx::query_as("SELECT id, status, output FROM tasks WHERE idempotency_key = ?")
                .bind(key)
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        let task_id = match &existing {
            Some((id, status, Some(out))) if status == "succeeded" => {
                let v: Value = from_json(out)?;
                return Ok(TaskOutput {
                    output: v.get("output").cloned().unwrap_or(Value::Null),
                    authors: serde_json::from_value(v.get("authors").cloned().unwrap_or(json!([])))
                        .unwrap_or_default(),
                    provenance: serde_json::from_value(v.get("provenance").cloned().unwrap_or(json!({})))
                        .unwrap_or_default(),
                })
                .inspect(|_| tracing::info!(task = %id, "replayed completed task (idempotent)"));
            }
            Some((id, _, _)) => id.parse::<TaskId>()?,
            None => {
                let id = TaskId::new();
                let now = ts(self.db.now());
                let mut tx = self.db.write().await?;
                sqlx::query(
                    "INSERT INTO tasks (id, workflow_instance_id, tenant_id, project_id, state_name, kind, idempotency_key, status, attempt, input, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', 1, ?, ?, ?)",
                )
                .bind(id.to_string())
                .bind(inst.id.to_string())
                .bind(inst.scope.tenant.to_string())
                .bind(inst.scope.project.to_string())
                .bind(&state.name)
                .bind(to_json(&spec)?)
                .bind(key)
                .bind(to_json(&input)?)
                .bind(&now)
                .bind(&now)
                .execute(tx.conn())
                .await
                .db()?;
                tx.commit().await?;
                id
            }
        };
        self.set_task(task_id, "running", None, None).await?;
        let hook = |point, err: Option<String>| HookContext {
            point,
            scope: Some(inst.scope),
            trace: inst.trace(),
            actor: Some(engine_actor()),
            subject: state.name.clone(),
            payload: HookPayload::Task {
                task_id: task_id.to_string(),
                kind: state.name.clone(),
                attempt: 1,
                error: err,
            },
        };
        if let Err(e) = self
            .hooks
            .dispatch(&hook(HookPoint::BeforeTaskExecution, None))
            .await
            .into_result()
        {
            self.set_task(task_id, "failed", None, Some(&e.to_string()))
                .await?;
            return Err(e);
        }
        let res = self
            .executor
            .execute(TaskExecution {
                task_id,
                instance: inst.id,
                state: state.name.clone(),
                spec,
                input,
                scope: inst.scope,
                trace: inst.trace().caused_by(task_id.as_uuid()),
                budget_owner: inst.budget_owner(),
                workspace: workspace.clone(),
                cancel: cancel.clone(),
            })
            .await;
        match &res {
            Ok(out) => {
                let stored =
                    json!({"output": out.output, "authors": out.authors, "provenance": out.provenance});
                self.set_task(task_id, "succeeded", Some(&stored), None).await?;
                self.hooks
                    .dispatch(&hook(HookPoint::AfterTaskExecution, None))
                    .await;
            }
            Err(e) if e.kind == eaeh_core::ErrorKind::AwaitingExternal => {
                self.set_task(task_id, "awaiting_external", None, None).await?;
            }
            Err(e) => {
                let status = if e.kind == eaeh_core::ErrorKind::Cancelled {
                    "cancelled"
                } else {
                    "failed"
                };
                self.set_task(task_id, status, None, Some(&e.to_string())).await?;
                self.hooks
                    .dispatch(&hook(HookPoint::AfterTaskExecution, Some(e.to_string())))
                    .await;
            }
        }
        res
    }

    async fn set_task(
        &self,
        id: TaskId,
        status: &str,
        output: Option<&Value>,
        error: Option<&str>,
    ) -> Result<()> {
        let mut tx = self.db.write().await?;
        sqlx::query(
            "UPDATE tasks SET status = ?, output = COALESCE(?, output), error = ?, heartbeat_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(status)
        .bind(output.map(to_json).transpose()?)
        .bind(error.map(|e| self.db.redactor().redact_str(e)))
        .bind(ts(self.db.now()))
        .bind(ts(self.db.now()))
        .bind(id.to_string())
        .execute(tx.conn())
        .await
        .db()?;
        tx.commit().await
    }

    async fn record_output(
        &self,
        inst: &WorkflowInstance,
        state: &StateDef,
        output: Value,
        out: TaskOutput,
    ) -> Result<()> {
        let fresh = self.get(inst.id).await?;
        let mut data = fresh.data.clone();
        let mut prov = out.provenance.clone();
        prov.workflow_instance = Some(inst.id.to_string());
        if let Some(g) = &state.quality_gate {
            let summary = self.run_quality_gate(&fresh, state, g, &output, &prov).await?;
            prov.quality_gates.push(summary.clone());
            data.quality.insert(state.name.clone(), json!(summary));
        }
        data.outputs.insert(state.name.clone(), output.clone());
        data.authors.insert(state.name.clone(), out.authors.clone());
        if let Some(name) = state.artifact_name() {
            let (bytes, prov) = if state.bundle {
                (self.store_bundle(inst, name, &output, &prov).await?, prov)
            } else {
                (serde_json::to_vec_pretty(&output)?, prov)
            };
            let meta = self
                .artifacts
                .store(inst.scope, name, "application/json", &bytes, prov, inst.trace())
                .await?;
            data.artifacts.insert(
                state.name.clone(),
                ArtifactRef {
                    id: meta.id,
                    name: meta.name,
                    version: meta.version,
                    hash: meta.content_hash,
                },
            );
        }
        self.save_data(&fresh, &data).await
    }

    /// Run the state's quality gate; blocking findings fail the state (fail closed).
    async fn run_quality_gate(
        &self,
        inst: &WorkflowInstance,
        state: &StateDef,
        g: &crate::definition::QualityGateRef,
        output: &Value,
        prov: &Provenance,
    ) -> Result<String> {
        let (Some(engine), Some(gate)) = (&self.validation, self.gates.get(&g.gate)) else {
            return Err(HarnessError::integrity(format!(
                "quality gate '{}' is not configured (fails closed)",
                g.gate
            )));
        };
        let artifacts = |s: &str| {
            inst.data
                .artifacts
                .get(s)
                .map(|a| json!({"id": a.id, "name": a.name, "version": a.version, "hash": a.hash}))
        };
        let mut ctx = serde_json::Map::new();
        for (k, b) in &g.context {
            ctx.insert(
                k.clone(),
                resolve_bindings(b, &inst.input, &inst.data.outputs, &artifacts)?,
            );
        }
        // Platform-supplied keys (never from the model): standards in force for the
        // producing run and the knowledge it retrieved.
        ctx.entry("standards").or_insert_with(|| json!(prov.standards));
        ctx.entry("knowledge").or_insert_with(|| json!(prov.knowledge));
        let subject = ValidationSubject {
            kind: "workflow_output".into(),
            name: state.name.clone(),
            content: json!({"output": output, "context": ctx}),
            metadata: BTreeMap::from([
                ("workflow".to_string(), inst.definition_id.clone()),
                ("state".to_string(), state.name.clone()),
            ]),
        };
        let r = engine.run_gate(gate, &subject).await;
        let summary = format!(
            "{}:{} ({} blocking, {} advisory)",
            gate.id,
            if r.passed { "passed" } else { "failed" },
            r.blocking.len(),
            r.advisory.len()
        );
        let mut tx = self.db.write().await?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    ENGINE_COMPONENT,
                    "workflow.quality_gate",
                    "workflow_instance",
                    inst.id.to_string(),
                    if r.passed {
                        AuditOutcome::Succeeded
                    } else {
                        AuditOutcome::Failed
                    },
                )
                .scope(inst.scope)
                .correlation(inst.trace().correlation)
                .details(json!({
                    "state": state.name,
                    "gate": gate.id,
                    "passed": r.passed,
                    "blocking": r.blocking.iter().map(|f| &f.message).collect::<Vec<_>>(),
                    "advisory": r.advisory.len(),
                })),
            )
            .await?;
        if !r.passed {
            emit(
                &self.db,
                &mut tx,
                &ValidationFailed {
                    gate_id: gate.id.clone(),
                    subject: format!("{}:{}", inst.id, state.name),
                    findings: r.blocking.len() as u32,
                },
                inst.scope,
                inst.trace(),
            )
            .await?;
        }
        tx.commit().await?;
        if r.passed {
            Ok(summary)
        } else {
            let first: Vec<String> = r.blocking.iter().take(5).map(|f| f.message.clone()).collect();
            Err(HarnessError::invalid_input(format!(
                "quality gate '{}' failed: {}",
                gate.id,
                first.join("; ")
            )))
        }
    }

    /// Store each file of a bundle output and return the manifest bytes (Phase 3).
    async fn store_bundle(
        &self,
        inst: &WorkflowInstance,
        name: &str,
        output: &Value,
        prov: &Provenance,
    ) -> Result<Vec<u8>> {
        let files = output
            .get("files")
            .and_then(Value::as_object)
            .ok_or_else(|| HarnessError::invalid_input("bundle output must contain a 'files' object"))?;
        let mut manifest = serde_json::Map::new();
        for (path, content) in files {
            let text = content
                .as_str()
                .ok_or_else(|| HarnessError::invalid_input(format!("bundle file '{path}' must be text")))?;
            let media = match path.rsplit('.').next() {
                Some("md") => "text/markdown",
                Some("mmd") => "text/vnd.mermaid",
                Some("json") => "application/json",
                _ => "text/plain",
            };
            let meta = self
                .artifacts
                .store(
                    inst.scope,
                    &format!("{name}/{path}"),
                    media,
                    text.as_bytes(),
                    prov.clone(),
                    inst.trace(),
                )
                .await?;
            manifest.insert(
                path.clone(),
                json!({"artifact_id": meta.id, "version": meta.version, "hash": meta.content_hash, "media_type": media}),
            );
        }
        let mut doc = json!({"bundle": name, "files": manifest});
        for k in ["document", "source", "trace", "summary"] {
            if let Some(v) = output.get(k) {
                doc[k] = v.clone();
            }
        }
        Ok(serde_json::to_vec_pretty(&doc)?)
    }

    /// Record the trace links declared by an approved artifact (`trace` array in its
    /// output). Only approved artifacts contribute to traceability (Phase 3, ADR-0022).
    async fn record_trace_links(
        &self,
        inst: &WorkflowInstance,
        artifact_from: &str,
        approval: ApprovalId,
    ) -> Result<()> {
        let Some(links) = inst
            .data
            .outputs
            .get(artifact_from)
            .and_then(|o| o.get("trace"))
            .and_then(Value::as_array)
        else {
            return Ok(());
        };
        let Some(art) = inst.data.artifacts.get(artifact_from) else {
            return Ok(());
        };
        let mut tx = self.db.write().await?;
        let now = ts(self.db.now());
        let mut n = 0u32;
        for l in links {
            let f = |k: &str| l.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
            let (fk, fi, rel, tk, ti) = (
                f("from_kind"),
                f("from_id"),
                f("relation"),
                f("to_kind"),
                f("to_id"),
            );
            if [&fk, &fi, &rel, &tk, &ti]
                .iter()
                .any(|x| x.is_empty() || x.len() > 512)
            {
                return Err(HarnessError::invalid_input(
                    "malformed trace link in approved artifact",
                ));
            }
            let r = sqlx::query(
                "INSERT OR IGNORE INTO trace_links (tenant_id, project_id, from_kind, from_id, relation, to_kind, to_id,                  artifact_id, artifact_name, artifact_version, artifact_hash, workflow_instance_id, approval_id, recorded_at)                  VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(inst.scope.tenant.to_string())
            .bind(inst.scope.project.to_string())
            .bind(&fk)
            .bind(&fi)
            .bind(&rel)
            .bind(&tk)
            .bind(&ti)
            .bind(art.id.to_string())
            .bind(&art.name)
            .bind(art.version)
            .bind(&art.hash)
            .bind(inst.id.to_string())
            .bind(approval.to_string())
            .bind(&now)
            .execute(tx.conn())
            .await
            .db()?;
            n += r.rows_affected() as u32;
        }
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    ENGINE_COMPONENT,
                    "trace.record",
                    "artifact",
                    art.id.to_string(),
                    AuditOutcome::Recorded,
                )
                .scope(inst.scope)
                .correlation(inst.trace().correlation)
                .details(json!({"links": n, "approval": approval.to_string(), "artifact_hash": art.hash})),
            )
            .await?;
        emit(
            &self.db,
            &mut tx,
            &TraceLinksRecorded {
                artifact_id: art.id.to_string(),
                links: n,
            },
            inst.scope,
            inst.trace(),
        )
        .await?;
        tx.commit().await
    }

    /// Trace links of a project, optionally restricted to those touching `item`, with
    /// `depth` hops of transitive closure in both directions (max 10).
    pub async fn trace(
        &self,
        actor: &Principal,
        scope: Scope,
        item: Option<&str>,
        depth: u32,
    ) -> Result<Vec<TraceLinkRecord>> {
        self.authz
            .authorize(actor, Action::WorkflowRead, Target::Scope(scope))?;
        let rows: Vec<TraceRow> =
            sqlx::query_as(
                "SELECT from_kind, from_id, relation, to_kind, to_id, artifact_id, artifact_name, artifact_version,                  artifact_hash, workflow_instance_id, approval_id FROM trace_links WHERE tenant_id = ? AND project_id = ?                  ORDER BY from_kind, from_id, relation, to_kind, to_id, artifact_id LIMIT 20000",
            )
            .bind(scope.tenant.to_string())
            .bind(scope.project.to_string())
            .fetch_all(self.db.pool())
            .await
            .db()?;
        let all: Vec<TraceLinkRecord> = rows
            .into_iter()
            .map(|r| TraceLinkRecord {
                from_kind: r.0,
                from_id: r.1,
                relation: r.2,
                to_kind: r.3,
                to_id: r.4,
                artifact_id: r.5,
                artifact_name: r.6,
                artifact_version: r.7,
                artifact_hash: r.8,
                workflow_instance_id: r.9,
                approval_id: r.10,
            })
            .collect();
        let Some(item) = item else {
            return Ok(all);
        };
        let mut frontier: BTreeSet<String> = BTreeSet::from([item.to_string()]);
        let mut seen: BTreeSet<String> = frontier.clone();
        let mut out: BTreeSet<usize> = BTreeSet::new();
        for _ in 0..depth.clamp(1, 10) {
            let mut next = BTreeSet::new();
            for (i, l) in all.iter().enumerate() {
                if frontier.contains(&l.from_id) || frontier.contains(&l.to_id) {
                    out.insert(i);
                    for x in [&l.from_id, &l.to_id] {
                        if seen.insert(x.clone()) {
                            next.insert(x.clone());
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        Ok(out.into_iter().map(|i| all[i].clone()).collect())
    }

    // ------------------------------------------------------------------ approvals & control

    /// Decide an approval for a workflow gate and resume the workflow (synchronously).
    /// All authorization and separation-of-duties checks happen in `ApprovalManager`.
    pub async fn decide_approval(
        &self,
        approver: &Principal,
        approval: ApprovalId,
        decision: ApprovalDecision,
        comments: &str,
        artifact_hash: Option<&str>,
        trace: TraceContext,
    ) -> Result<InstanceStatus> {
        let instance = self
            .record_decision(approver, approval, decision, comments, artifact_hash, trace)
            .await?;
        self.drive(instance).await
    }

    /// Record a gate decision (hooks + governed decision) without resuming; returns the
    /// workflow instance to resume (e.g. in the background).
    pub async fn record_decision(
        &self,
        approver: &Principal,
        approval: ApprovalId,
        decision: ApprovalDecision,
        comments: &str,
        artifact_hash: Option<&str>,
        trace: TraceContext,
    ) -> Result<WorkflowInstanceId> {
        let rec = self.approvals.get(approval).await?;
        let instance: WorkflowInstanceId = rec
            .workflow_instance_id
            .as_deref()
            .ok_or_else(|| HarnessError::invalid_input("approval is not a workflow gate"))?
            .parse()?;
        let inst = self.get(instance).await?;
        let gate = rec.gate.clone().unwrap_or_default();
        self.hooks
            .dispatch(&self.wf_hook(HookPoint::BeforeApproval, &inst, Some(&gate), "deciding"))
            .await
            .into_result()?;
        // Record the decision under the workflow's correlation id so the whole run is
        // traceable; the caller's request becomes the causation.
        let trace = TraceContext {
            correlation: inst.trace().correlation,
            causation: trace.causation.or(Some(trace.correlation.as_uuid())),
        };
        let decided = self
            .approvals
            .decide(approver, approval, decision, comments, artifact_hash, trace)
            .await?;
        let mut ctx = self.wf_hook(HookPoint::AfterApproval, &inst, Some(&gate), "decided");
        ctx.payload = HookPayload::Approval {
            approval_id: approval.to_string(),
            decision: decided.decision.as_ref().map(|d| d.decision.clone()),
        };
        self.hooks.dispatch(&ctx).await;
        let inst = self.get(instance).await?;
        if inst.status == InstanceStatus::WaitingApproval {
            self.set_status(&inst, InstanceStatus::Running).await?;
        }
        Ok(instance)
    }

    pub async fn cancel(&self, actor: &Principal, id: WorkflowInstanceId) -> Result<InstanceStatus> {
        let inst = self.get(id).await?;
        self.authz
            .authorize(actor, Action::WorkflowCancel, Target::Scope(inst.scope))?;
        if inst.status.is_terminal() {
            return Err(HarnessError::conflict(format!(
                "workflow already {}",
                inst.status.as_str()
            )));
        }
        let mut tx = self.db.write().await?;
        sqlx::query("UPDATE workflow_instances SET cancel_requested = 1 WHERE id = ?")
            .bind(id.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        tx.commit().await?;
        self.db
            .audit(
                AuditEntry::new(
                    actor,
                    "workflow.cancel",
                    "workflow_instance",
                    id.to_string(),
                    AuditOutcome::Requested,
                )
                .scope(inst.scope)
                .correlation(inst.trace().correlation),
            )
            .await?;
        self.cancel_token(id).cancel();
        self.hooks
            .dispatch(&self.wf_hook(HookPoint::OnCancellation, &inst, None, "cancel_requested"))
            .await;
        // If no drive is in progress (e.g. waiting for approval), finish now.
        let lock = self.instance_lock(id);
        if let Ok(_g) = lock.try_lock() {
            let inst = self.get(id).await?;
            if !inst.status.is_terminal() {
                self.finish(
                    &inst,
                    InstanceStatus::Cancelled,
                    &inst.state,
                    Some("cancelled by request".into()),
                )
                .await?;
            }
        }
        Ok(self.get(id).await?.status)
    }

    /// Crash recovery: mark in-flight tasks interrupted and return instances to resume.
    pub async fn recover(&self) -> Result<Vec<WorkflowInstanceId>> {
        let mut tx = self.db.write().await?;
        let interrupted = sqlx::query(
            "UPDATE tasks SET status = 'interrupted', updated_at = ? WHERE status IN ('running', 'pending')",
        )
        .bind(ts(self.db.now()))
        .execute(tx.conn())
        .await
        .db()?
        .rows_affected();
        let ids: Vec<(String,)> =
            sqlx::query_as("SELECT id FROM workflow_instances WHERE status = 'running' ORDER BY id")
                .fetch_all(tx.conn())
                .await
                .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    ENGINE_COMPONENT,
                    "workflow.recover",
                    "workflow_engine",
                    "startup",
                    AuditOutcome::Recorded,
                )
                .details(json!({"interrupted_tasks": interrupted, "instances": ids.len()})),
            )
            .await?;
        tx.commit().await?;
        let mut out = vec![];
        for (id,) in ids {
            let id: WorkflowInstanceId = id.parse()?;
            if let Ok(inst) = self.get(id).await {
                self.hooks
                    .dispatch(&self.wf_hook(HookPoint::OnRecovery, &inst, None, "recovering"))
                    .await;
            }
            out.push(id);
        }
        Ok(out)
    }

    /// Instances whose running task has not heartbeated since `stale_before` (supervision).
    pub async fn stalled_tasks(
        &self,
        stale_before: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<(String, String)>> {
        sqlx::query_as(
            "SELECT id, workflow_instance_id FROM tasks WHERE status = 'running' AND heartbeat_at < ?",
        )
        .bind(ts(stale_before))
        .fetch_all(self.db.pool())
        .await
        .db()
    }

    // ------------------------------------------------------------------ persistence helpers

    fn wf_hook(
        &self,
        point: HookPoint,
        inst: &WorkflowInstance,
        to: Option<&str>,
        trigger: &str,
    ) -> HookContext {
        HookContext {
            point,
            scope: Some(inst.scope),
            trace: inst.trace(),
            actor: Some(engine_actor()),
            subject: inst.definition_id.clone(),
            payload: HookPayload::Workflow {
                instance_id: inst.id.to_string(),
                from_state: inst.state.clone(),
                to_state: to.map(String::from),
                trigger: trigger.into(),
            },
        }
    }

    async fn transition(
        &self,
        inst: &WorkflowInstance,
        state: &StateDef,
        trigger: Trigger,
        data: WorkflowData,
        actor: &str,
    ) -> Result<()> {
        let to = state
            .transitions
            .get(&trigger)
            .ok_or_else(|| HarnessError::integrity(format!("no {trigger:?} transition from {}", state.name)))?
            .clone();
        if let Err(e) = self
            .hooks
            .dispatch(&self.wf_hook(
                HookPoint::BeforeWorkflowTransition,
                inst,
                Some(&to),
                trigger.as_str(),
            ))
            .await
            .into_result()
        {
            self.finish(inst, InstanceStatus::Failed, &inst.state, Some(e.to_string()))
                .await?;
            return Err(e);
        }
        let mut tx = self.db.write().await?;
        self.update_in(&mut tx, inst, Some(&to), Some(InstanceStatus::Running), &data)
            .await?;
        sqlx::query(
            "INSERT INTO workflow_transitions (instance_id, seq, from_state, to_state, trigger, actor_id, checkpoint, occurred_at) \
             VALUES (?, (SELECT COALESCE(MAX(seq), 0) + 1 FROM workflow_transitions WHERE instance_id = ?), ?, ?, ?, ?, ?, ?)",
        )
        .bind(inst.id.to_string())
        .bind(inst.id.to_string())
        .bind(&inst.state)
        .bind(&to)
        .bind(trigger.as_str())
        .bind(actor)
        .bind(to_json(&data)?)
        .bind(ts(self.db.now()))
        .execute(tx.conn())
        .await
        .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    ENGINE_COMPONENT,
                    "workflow.transition",
                    "workflow_instance",
                    inst.id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .scope(inst.scope)
                .correlation(inst.trace().correlation)
                .details(json!({"from": inst.state, "to": to, "trigger": trigger.as_str(), "actor": actor})),
            )
            .await?;
        emit(
            &self.db,
            &mut tx,
            &WorkflowTransitioned {
                instance_id: inst.id.to_string(),
                from_state: inst.state.clone(),
                to_state: to.clone(),
                trigger: trigger.as_str().into(),
            },
            inst.scope,
            inst.trace(),
        )
        .await?;
        tx.commit().await?;
        let after = self.get(inst.id).await?;
        self.hooks
            .dispatch(&self.wf_hook(
                HookPoint::AfterWorkflowTransition,
                &after,
                Some(&to),
                trigger.as_str(),
            ))
            .await;
        Ok(())
    }

    /// Optimistic-concurrency update of state/status/data.
    async fn update_in(
        &self,
        tx: &mut WriteTx,
        inst: &WorkflowInstance,
        state: Option<&str>,
        status: Option<InstanceStatus>,
        data: &WorkflowData,
    ) -> Result<()> {
        let r = sqlx::query(
            "UPDATE workflow_instances SET state = COALESCE(?, state), status = COALESCE(?, status), data = ?, version = version + 1, updated_at = ? \
             WHERE id = ? AND version = ?",
        )
        .bind(state)
        .bind(status.map(|s| s.as_str()))
        .bind(to_json(data)?)
        .bind(ts(self.db.now()))
        .bind(inst.id.to_string())
        .bind(inst.version)
        .execute(tx.conn())
        .await
        .db()?;
        if r.rows_affected() != 1 {
            return Err(HarnessError::conflict(
                "workflow instance was modified concurrently",
            ));
        }
        Ok(())
    }

    async fn save_data(&self, inst: &WorkflowInstance, data: &WorkflowData) -> Result<()> {
        let mut tx = self.db.write().await?;
        self.update_in(&mut tx, inst, None, None, data).await?;
        tx.commit().await
    }

    async fn set_status(&self, inst: &WorkflowInstance, status: InstanceStatus) -> Result<()> {
        let mut tx = self.db.write().await?;
        self.update_in(&mut tx, inst, None, Some(status), &inst.data)
            .await?;
        tx.commit().await
    }

    async fn finish(
        &self,
        inst: &WorkflowInstance,
        status: InstanceStatus,
        state: &str,
        reason: Option<String>,
    ) -> Result<()> {
        let mut data = inst.data.clone();
        if reason.is_some() {
            data.failure = reason.clone();
        }
        let mut tx = self.db.write().await?;
        self.update_in(&mut tx, inst, Some(state), Some(status), &data)
            .await?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    ENGINE_COMPONENT,
                    "workflow.complete",
                    "workflow_instance",
                    inst.id.to_string(),
                    AuditOutcome::Recorded,
                )
                .scope(inst.scope)
                .correlation(inst.trace().correlation)
                .details(json!({"status": status.as_str(), "state": state, "reason": reason})),
            )
            .await?;
        emit(
            &self.db,
            &mut tx,
            &WorkflowCompleted {
                instance_id: inst.id.to_string(),
                status: status.as_str().into(),
            },
            inst.scope,
            inst.trace(),
        )
        .await?;
        tx.commit().await?;
        if let Ok(mut c) = self.cancels.lock() {
            c.remove(&inst.id);
        }
        Ok(())
    }
}

async fn emit<E: DomainEvent>(
    db: &Db,
    tx: &mut WriteTx,
    ev: &E,
    scope: Scope,
    trace: TraceContext,
) -> Result<()> {
    let env = EventEnvelope::new(ev, ENGINE_COMPONENT, Some(scope), trace, db.now())?;
    outbox::enqueue_in(tx, &env).await.map(|_| ())
}

fn row_to_instance(row: &sqlx::sqlite::SqliteRow) -> Result<WorkflowInstance> {
    use sqlx::Row;
    Ok(WorkflowInstance {
        id: row.try_get::<String, _>("id").db()?.parse()?,
        definition_id: row.try_get("definition_id").db()?,
        definition_version: row.try_get("definition_version").db()?,
        scope: Scope::new(
            row.try_get::<String, _>("tenant_id").db()?.parse()?,
            row.try_get::<String, _>("project_id").db()?.parse()?,
        ),
        state: row.try_get("state").db()?,
        status: InstanceStatus::parse(&row.try_get::<String, _>("status").db()?)?,
        version: row.try_get("version").db()?,
        input: from_json(&row.try_get::<String, _>("input").db()?)?,
        data: from_json(&row.try_get::<String, _>("data").db()?)?,
        started_by: row.try_get("started_by").db()?,
        correlation_id: row.try_get("correlation_id").db()?,
        cancel_requested: row.try_get::<i64, _>("cancel_requested").db()? != 0,
        created_at: row.try_get("created_at").db()?,
        updated_at: row.try_get("updated_at").db()?,
    })
}
