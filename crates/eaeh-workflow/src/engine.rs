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
use eaeh_workspace::{
    ArtifactStore, MaterializationStatus, MaterializeReport, MaterializeRequest, Materializer, Provenance,
    Workspace, WorkspaceManager,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[path = "restart.rs"]
mod restart;
pub use restart::{RestartPlan, StagePlan, TaskDetail};

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
    /// Set while an approved gate cannot take its `approved` transition because the
    /// approved bundle could not be materialized into the project workspace
    /// (ARTIFACT-MATERIALIZATION-PLAN, M1). The instance stays `running` at the gate;
    /// every drive (and `eaeh artifact sync`) retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<BlockedGate>,
    /// Derived runs (restart/retry/reinitiation, PORTAL-OPERATIONS-PLAN W1): approvals of
    /// gates before the restart stage, carried from an ancestor run. `check_gate` accepts
    /// them only through verified lineage (`workflow_lineage`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub carried_approvals: BTreeMap<String, CarriedApproval>,
    /// The run this one was derived from, and the stage it restarted at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restarted_from: Option<RestartOrigin>,
}

/// An approval made in an ancestor run, carried into a derived run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarriedApproval {
    /// The run in which the approval was made (its subject is `"{origin}:{gate}"`).
    pub origin_instance: WorkflowInstanceId,
    pub approval_id: ApprovalId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartOrigin {
    pub source: WorkflowInstanceId,
    pub stage: String,
}

/// Why an instance is held at an approved gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedGate {
    pub gate: String,
    pub reason: String,
    pub since: String,
}

/// An approval gate's document: the reviewed artifact, its approval verification and
/// where (and whether) the approved version is materialized ([`WorkflowEngine::documents`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateDocument {
    pub gate: String,
    pub artifact_from: String,
    pub artifact: Option<ArtifactRef>,
    pub approval: GateCheck,
    /// The gate holds a valid approval of the current artifact version.
    pub approved: bool,
    /// Repository-relative target directory, if the gate materializes its bundle.
    pub target_dir: Option<String>,
    pub materialization: Option<MaterializationStatus>,
    pub blocked: Option<String>,
}

/// Result of [`WorkflowEngine::sync_documents`] for one gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncOutcome {
    pub gate: String,
    pub target_dir: String,
    /// `succeeded`, `conflict`, `failed` or `skipped` (not approved).
    pub status: String,
    pub detail: String,
    pub report: Option<MaterializeReport>,
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

/// `(instance, definition, version, from, to, trigger, actor, at)` of a transition joined to its instance.
type MoveRow = (String, String, String, String, String, String, String, String);

/// Verification of one approval gate ([`WorkflowEngine::gate_checks`]). `approval_id` is
/// `None` while the gate has no granted approval; `issues` is empty when the approval
/// satisfies the completion guard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateCheck {
    pub gate: String,
    pub artifact_from: String,
    pub approval_id: Option<String>,
    pub approver_id: Option<String>,
    pub approver_role: Option<String>,
    pub decided_at: Option<String>,
    pub approved_hash: Option<String>,
    pub current_hash: Option<String>,
    pub issues: Vec<String>,
}

/// One workflow event in [`WorkflowEngine::recent_activity`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityItem {
    /// `workflow_started`, `task_completed`, `artifact_generated`, `approval_requested`,
    /// `approval_granted`, `approval_rejected`, `changes_requested`, `stage_failed`,
    /// `workflow_completed`, `workflow_failed`, `workflow_cancelled` or `transition`.
    pub kind: String,
    pub occurred_at: String,
    pub workflow_instance_id: String,
    pub definition_id: String,
    pub state: Option<String>,
    pub to_state: Option<String>,
    pub actor_id: String,
}

/// Read model of one task execution (portal, status views). `name` is the agent task kind
/// or the tool id; `agent` the agent definition (`id@version`) instantiated for it, if any.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub workflow_instance_id: String,
    pub state: String,
    pub kind: String,
    pub name: String,
    pub status: String,
    pub attempt: i64,
    pub agent: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Filters of [`WorkflowEngine::task_list`]; `None` means "any".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskFilter {
    pub workflow: Option<String>,
    pub instance: Option<String>,
    pub stage: Option<String>,
    pub status: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub before: Option<String>,
}

/// Static SQL prefix shared by the task read models (a macro so `concat!` keeps it `&'static`).
macro_rules! task_select {
    () => {
        "SELECT t.id, t.workflow_instance_id, t.state_name, t.kind, t.status, t.attempt, t.error, \
     t.created_at, t.updated_at, \
     (SELECT a.definition_id || '@' || a.definition_version FROM agent_instances a WHERE a.task_id = t.id \
      ORDER BY a.created_at DESC LIMIT 1) AS agent \
     FROM tasks t"
    };
}

type TaskRow = (
    String,
    Option<String>,
    String,
    String,
    String,
    i64,
    Option<String>,
    String,
    String,
    Option<String>,
);

fn task_summary(r: TaskRow) -> TaskSummary {
    let (id, instance, state, kind_json, status, attempt, error, created_at, updated_at, agent) = r;
    let (kind, name) = match serde_json::from_str::<TaskSpec>(&kind_json) {
        Ok(TaskSpec::Agent { task_kind, .. }) => ("agent".to_string(), task_kind),
        Ok(TaskSpec::Tool { tool, .. }) => ("tool".to_string(), tool.to_string()),
        Err(_) => ("unknown".to_string(), state.clone()),
    };
    TaskSummary {
        id,
        workflow_instance_id: instance.unwrap_or_default(),
        state,
        kind,
        name,
        status,
        attempt,
        agent,
        error,
        created_at,
        updated_at,
    }
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
    materializer: Materializer,
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
        let materializer = Materializer::new(db.clone(), artifacts.clone(), hooks.clone());
        Self {
            db,
            authz,
            policy,
            approvals,
            budgets,
            hooks,
            executor,
            planner,
            materializer,
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

    /// Materialization of approved bundles into the registered project workspace.
    pub fn materializer(&self) -> &Materializer {
        &self.materializer
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

    // ------------------------------------------------------------ read models (portal)

    /// Task executions of one instance, oldest first (authorized like [`Self::get_for`]).
    pub async fn tasks_for(&self, actor: &Principal, id: WorkflowInstanceId) -> Result<Vec<TaskSummary>> {
        self.get_for(actor, id).await?;
        let rows: Vec<TaskRow> = sqlx::query_as(concat!(
            task_select!(),
            " WHERE t.workflow_instance_id = ? ORDER BY t.created_at, t.id"
        ))
        .bind(id.to_string())
        .fetch_all(self.db.pool())
        .await
        .db()?;
        Ok(rows.into_iter().map(task_summary).collect())
    }

    /// Tasks that are pending, running or handed off to an external AI client.
    pub async fn active_tasks(
        &self,
        actor: &Principal,
        scope: Scope,
        limit: u32,
    ) -> Result<Vec<TaskSummary>> {
        self.authz
            .authorize(actor, Action::WorkflowRead, Target::Scope(scope))?;
        let rows: Vec<TaskRow> = sqlx::query_as(concat!(
            task_select!(),
            " WHERE t.tenant_id = ? AND t.project_id = ? \
             AND t.status IN ('pending', 'running', 'awaiting_external') ORDER BY t.updated_at DESC LIMIT ?"
        ))
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(i64::from(limit.clamp(1, 100)))
        .fetch_all(self.db.pool())
        .await
        .db()?;
        Ok(rows.into_iter().map(task_summary).collect())
    }

    /// Tasks of a project, most recently updated first, filtered by workflow definition,
    /// stage, task status and update time (`since`/`until`, RFC 3339), with a keyset cursor
    /// (`before` = the last `updated_at|id` of the previous page).
    pub async fn task_list(
        &self,
        actor: &Principal,
        scope: Scope,
        filter: &TaskFilter,
        limit: u32,
    ) -> Result<Vec<TaskSummary>> {
        self.authz
            .authorize(actor, Action::WorkflowRead, Target::Scope(scope))?;
        let (before_ts, before_id) = match filter.before.as_deref().and_then(|c| c.split_once('|')) {
            Some((t, i)) => (Some(t.to_string()), Some(i.to_string())),
            None => (None, None),
        };
        let rows: Vec<TaskRow> = sqlx::query_as(concat!(
            task_select!(),
            " WHERE t.tenant_id = ? AND t.project_id = ? \
             AND (? IS NULL OR t.workflow_instance_id IN (SELECT w.id FROM workflow_instances w WHERE w.definition_id = ?)) \
             AND (? IS NULL OR t.workflow_instance_id = ?) \
             AND (? IS NULL OR t.state_name = ?) AND (? IS NULL OR t.status = ?) \
             AND (? IS NULL OR t.updated_at >= ?) AND (? IS NULL OR t.updated_at <= ?) \
             AND (? IS NULL OR t.updated_at < ? OR (t.updated_at = ? AND t.id < ?)) \
             ORDER BY t.updated_at DESC, t.id DESC LIMIT ?"
        ))
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(&filter.workflow)
        .bind(&filter.workflow)
        .bind(&filter.instance)
        .bind(&filter.instance)
        .bind(&filter.stage)
        .bind(&filter.stage)
        .bind(&filter.status)
        .bind(&filter.status)
        .bind(&filter.since)
        .bind(&filter.since)
        .bind(&filter.until)
        .bind(&filter.until)
        .bind(&before_ts)
        .bind(&before_ts)
        .bind(&before_ts)
        .bind(&before_id)
        .bind(i64::from(limit.clamp(1, 200)))
        .fetch_all(self.db.pool())
        .await
        .db()?;
        Ok(rows.into_iter().map(task_summary).collect())
    }

    /// Non-terminal instances (running, waiting for an approval or an external AI client),
    /// most recently updated first.
    pub async fn list_active(
        &self,
        actor: &Principal,
        scope: Scope,
        limit: u32,
    ) -> Result<Vec<WorkflowInstance>> {
        self.authz
            .authorize(actor, Action::WorkflowRead, Target::Scope(scope))?;
        let rows = sqlx::query(
            "SELECT * FROM workflow_instances WHERE tenant_id = ? AND project_id = ? \
             AND status IN ('running', 'waiting_approval', 'waiting_external') ORDER BY updated_at DESC LIMIT ?",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(i64::from(limit.clamp(1, 100)))
        .fetch_all(self.db.pool())
        .await
        .db()?;
        rows.iter().map(row_to_instance).collect()
    }

    /// Recent workflow events for one project, newest first: instance starts plus recorded
    /// transitions, classified against the instance's definition (`workflow.read`; it does
    /// not need `audit.read` because it exposes only workflow history).
    pub async fn recent_activity(
        &self,
        actor: &Principal,
        scope: Scope,
        limit: u32,
    ) -> Result<Vec<ActivityItem>> {
        self.authz
            .authorize(actor, Action::WorkflowRead, Target::Scope(scope))?;
        let limit = i64::from(limit.clamp(1, 100));
        let starts: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, definition_id, definition_version, started_by, created_at FROM workflow_instances \
             WHERE tenant_id = ? AND project_id = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(limit)
        .fetch_all(self.db.pool())
        .await
        .db()?;
        let moves: Vec<MoveRow> = sqlx::query_as(
            "SELECT t.instance_id, w.definition_id, w.definition_version, t.from_state, t.to_state, t.trigger, t.actor_id, t.occurred_at \
             FROM workflow_transitions t JOIN workflow_instances w ON w.id = t.instance_id \
             WHERE w.tenant_id = ? AND w.project_id = ? ORDER BY t.occurred_at DESC, t.seq DESC LIMIT ?",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(limit)
        .fetch_all(self.db.pool())
        .await
        .db()?;
        let mut defs: HashMap<(String, String), Option<WorkflowDefinition>> = HashMap::new();
        let mut items = Vec::with_capacity(starts.len() + moves.len());
        for (id, def, _, by, at) in starts {
            items.push(ActivityItem {
                kind: "workflow_started".into(),
                occurred_at: at,
                workflow_instance_id: id,
                definition_id: def,
                state: None,
                to_state: None,
                actor_id: by,
            });
        }
        for (id, def_id, ver, from, to, trigger, by, at) in moves {
            let key = (def_id.clone(), ver.clone());
            if !defs.contains_key(&key) {
                let d = self.definition(&def_id, &ver).await.ok();
                defs.insert(key.clone(), d);
            }
            let from_def = defs
                .get(&key)
                .and_then(|d| d.as_ref())
                .and_then(|d| d.states.iter().find(|s| s.name == from));
            let to_def = defs
                .get(&key)
                .and_then(|d| d.as_ref())
                .and_then(|d| d.states.iter().find(|s| s.name == to));
            let kind = match trigger.as_str() {
                "approved" => "approval_granted",
                "rejected" => "approval_rejected",
                "changes_requested" => "changes_requested",
                "failed" | "timed_out" => "stage_failed",
                "succeeded" if from_def.is_some_and(|s| s.artifact_name().is_some()) => "artifact_generated",
                "succeeded" => "task_completed",
                _ => "transition",
            };
            items.push(ActivityItem {
                kind: kind.into(),
                occurred_at: at.clone(),
                workflow_instance_id: id.clone(),
                definition_id: def_id.clone(),
                state: Some(from),
                to_state: Some(to.clone()),
                actor_id: by.clone(),
            });
            if let Some(s) = to_def {
                let follow = match &s.kind {
                    StateKind::ApprovalGate { .. } => Some("approval_requested"),
                    StateKind::Terminal {
                        outcome: Outcome::Succeeded,
                    } => Some("workflow_completed"),
                    StateKind::Terminal {
                        outcome: Outcome::Failed,
                    } => Some("workflow_failed"),
                    StateKind::Terminal {
                        outcome: Outcome::Cancelled,
                    } => Some("workflow_cancelled"),
                    _ => None,
                };
                if let Some(k) = follow {
                    items.push(ActivityItem {
                        kind: k.into(),
                        occurred_at: at,
                        workflow_instance_id: id,
                        definition_id: def_id,
                        state: Some(to),
                        to_state: None,
                        actor_id: by,
                    });
                }
            }
        }
        items.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at));
        items.truncate(limit as usize);
        Ok(items)
    }

    /// Instance counts by status for one project.
    pub async fn status_counts(&self, actor: &Principal, scope: Scope) -> Result<BTreeMap<String, i64>> {
        self.authz
            .authorize(actor, Action::WorkflowRead, Target::Scope(scope))?;
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT status, COUNT(*) FROM workflow_instances WHERE tenant_id = ? AND project_id = ? GROUP BY status",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .fetch_all(self.db.pool())
        .await
        .db()?;
        Ok(rows.into_iter().collect())
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
                    // Completion guard: success is recorded only when every approval gate
                    // leading to this terminal holds a valid human approval of the
                    // artifact version it reviewed – independent of the transition graph.
                    if status == InstanceStatus::Succeeded {
                        let violations = self.completion_violations(&inst, &def, &state.name).await?;
                        self.db
                            .audit(
                                AuditEntry::system(
                                    ENGINE_COMPONENT,
                                    "workflow.completion_check",
                                    "workflow_instance",
                                    inst.id.to_string(),
                                    if violations.is_empty() {
                                        AuditOutcome::Succeeded
                                    } else {
                                        AuditOutcome::Denied
                                    },
                                )
                                .scope(inst.scope)
                                .correlation(inst.trace().correlation)
                                .details(json!({
                                    "terminal": state.name,
                                    "approved_gates": inst.data.approved_gates,
                                    "violations": violations,
                                })),
                            )
                            .await?;
                        if !violations.is_empty() {
                            let reason = format!("completion guard: {}", violations.join("; "));
                            self.finish(&inst, InstanceStatus::Failed, &inst.state, Some(reason))
                                .await?;
                            return Ok(InstanceStatus::Failed);
                        }
                    }
                    self.finish(&inst, status, &inst.state, None).await?;
                    return Ok(status);
                }
                StateKind::ApprovalGate {
                    required_role,
                    artifact_from,
                    max_revisions,
                    materialize,
                } => {
                    if let Some(aid) = inst.data.current_approval.get(&state.name).copied() {
                        let rec = self.approvals.get(aid).await?;
                        match rec.status {
                            ApprovalStatus::Pending => {
                                self.set_status(&inst, InstanceStatus::WaitingApproval).await?;
                                return Ok(InstanceStatus::WaitingApproval);
                            }
                            ApprovalStatus::Approved => {
                                // Write the approved version into the project workspace
                                // before moving on; a failure holds the run at this gate.
                                if let Some(dir) = materialize
                                    && let Err(e) = self
                                        .materialize_gate(
                                            &inst,
                                            &def,
                                            &state.name,
                                            artifact_from,
                                            dir,
                                            &rec,
                                            false,
                                            &engine_actor(),
                                        )
                                        .await
                                {
                                    self.block(&inst, &state.name, &e.message).await?;
                                    return Ok(InstanceStatus::Running);
                                }
                                self.record_trace_links(&inst, artifact_from, aid).await?;
                                let mut data = inst.data.clone();
                                data.blocked = None;
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

    /// Approval gates from which `terminal` is reachable, each of which must hold a valid
    /// approval before the instance may succeed. Returns human-readable violations.
    async fn completion_violations(
        &self,
        inst: &WorkflowInstance,
        def: &WorkflowDefinition,
        terminal: &str,
    ) -> Result<Vec<String>> {
        let mut preds: HashMap<&str, Vec<&str>> = HashMap::new();
        for s in &def.states {
            for to in s.transitions.values() {
                preds.entry(to.as_str()).or_default().push(s.name.as_str());
            }
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut stack = vec![terminal];
        while let Some(x) = stack.pop() {
            for p in preds.get(x).into_iter().flatten() {
                if seen.insert(p) {
                    stack.push(p);
                }
            }
        }
        let mut out = vec![];
        for gate in def.states.iter().filter(|s| seen.contains(s.name.as_str())) {
            if matches!(gate.kind, StateKind::ApprovalGate { .. }) {
                let c = self.check_gate(inst, gate).await;
                if c.approval_id.is_none() {
                    out.push(format!("gate '{}' has no granted approval", gate.name));
                }
                out.extend(c.issues.into_iter().map(|i| format!("gate '{}': {i}", gate.name)));
            }
        }
        Ok(out)
    }

    /// Verification of one approval gate of an instance: the granted approval (if any) and
    /// every reason it would not satisfy the completion guard. Used by the guard itself and
    /// by read models (portal), so both apply exactly the same rules.
    async fn check_gate(&self, inst: &WorkflowInstance, gate: &StateDef) -> GateCheck {
        let g = &gate.name;
        let artifact_from = match &gate.kind {
            StateKind::ApprovalGate { artifact_from, .. } => artifact_from.clone(),
            _ => String::new(),
        };
        let mut c = GateCheck {
            gate: g.clone(),
            artifact_from: artifact_from.clone(),
            approval_id: None,
            approver_id: None,
            approver_role: None,
            decided_at: None,
            approved_hash: None,
            current_hash: inst.data.artifacts.get(&artifact_from).map(|a| a.hash.clone()),
            issues: vec![],
        };
        let Some(aid) = inst.data.approved_gates.get(g) else {
            return c;
        };
        c.approval_id = Some(aid.to_string());
        let rec = match self.approvals.get(*aid).await {
            Ok(r) => r,
            Err(_) => {
                c.issues.push(format!("approval {aid} not found"));
                return c;
            }
        };
        c.approved_hash = rec.artifact_hash.clone();
        if rec.status != ApprovalStatus::Approved {
            c.issues
                .push(format!("approval {aid} is {:?}, not approved", rec.status));
        }
        // Own approval, or one carried from an ancestor run through verified lineage.
        let expected_subject = match inst.data.carried_approvals.get(g) {
            Some(cr) if cr.approval_id == *aid => {
                let ancestors = self.ancestors(inst.id).await.unwrap_or_default();
                if !ancestors.contains(&cr.origin_instance) {
                    c.issues.push(format!(
                        "carried approval {aid} is not from an ancestor run of this workflow"
                    ));
                }
                format!("{}:{g}", cr.origin_instance)
            }
            _ => format!("{}:{g}", inst.id),
        };
        if rec.subject_ref != expected_subject || rec.scope != inst.scope {
            c.issues
                .push(format!("approval {aid} belongs to another workflow or project"));
        }
        match &rec.decision {
            None => c.issues.push(format!("approval {aid} has no recorded decision")),
            Some(d) => {
                c.approver_id = Some(d.approver_id.clone());
                c.approver_role = Some(d.approver_role.clone());
                c.decided_at = Some(d.decided_at.clone());
                if d.decision != "approve" {
                    c.issues.push(format!("recorded decision is '{}'", d.decision));
                }
                if d.approver_id.is_empty() || rec.authors.iter().any(|a| a.as_str() == d.approver_id) {
                    c.issues
                        .push(format!("approver '{}' is missing or an author", d.approver_id));
                }
                if d.artifact_hash.is_none() || d.artifact_hash != rec.artifact_hash {
                    c.issues
                        .push("decision is not bound to the reviewed artifact hash".into());
                }
            }
        }
        match (&c.current_hash, rec.artifact_hash.as_deref()) {
            (Some(a), Some(h)) if a == h => {}
            (Some(_), _) => c.issues.push(format!(
                "approved hash differs from the current '{artifact_from}' artifact"
            )),
            (None, _) => c
                .issues
                .push(format!("reviewed artifact '{artifact_from}' is missing")),
        }
        c
    }

    /// Verification of every approval gate of an instance (`workflow.read`).
    pub async fn gate_checks(&self, actor: &Principal, id: WorkflowInstanceId) -> Result<Vec<GateCheck>> {
        let inst = self.get_for(actor, id).await?;
        let def = self
            .definition(&inst.definition_id, &inst.definition_version)
            .await?;
        let mut out = vec![];
        for s in def
            .states
            .iter()
            .filter(|s| matches!(s.kind, StateKind::ApprovalGate { .. }))
        {
            out.push(self.check_gate(&inst, s).await);
        }
        Ok(out)
    }

    /// The instance with approvals of gates that are approved but not yet passed (held by
    /// a blocked materialization) counted as granted.
    async fn effective(&self, inst: &WorkflowInstance) -> Result<WorkflowInstance> {
        let mut e = inst.clone();
        for (gate, aid) in &inst.data.current_approval {
            if self.approvals.get(*aid).await?.status == ApprovalStatus::Approved {
                e.data.approved_gates.insert(gate.clone(), *aid);
            }
        }
        Ok(e)
    }

    /// Target directory of a gate: its own `materialize`, or – for runs started before
    /// the gate declared one – the same gate of the latest definition version.
    fn target_dir(gate: &StateDef, latest: Option<&WorkflowDefinition>) -> Option<String> {
        let own = |s: &StateDef| match &s.kind {
            StateKind::ApprovalGate { materialize, .. } => materialize.clone(),
            _ => None,
        };
        own(gate).or_else(|| latest.and_then(|d| d.state(&gate.name).ok()).and_then(own))
    }

    /// Write the approved bundle of `gate` into the project workspace.
    #[allow(clippy::too_many_arguments)]
    async fn materialize_gate(
        &self,
        inst: &WorkflowInstance,
        def: &WorkflowDefinition,
        gate: &str,
        artifact_from: &str,
        dir: &str,
        rec: &eaeh_governance::ApprovalRecord,
        replace_modified: bool,
        actor: &str,
    ) -> Result<MaterializeReport> {
        let art = inst.data.artifacts.get(artifact_from).ok_or_else(|| {
            HarnessError::integrity(format!("gate '{gate}' has no artifact from '{artifact_from}'"))
        })?;
        if rec.status != ApprovalStatus::Approved || rec.artifact_hash.as_deref() != Some(art.hash.as_str()) {
            return Err(HarnessError::approval_required(format!(
                "gate '{gate}': only the approved version of '{}' is materialized",
                art.name
            )));
        }
        let d = rec.decision.as_ref();
        let approval = json!({
            "id": rec.id.to_string(),
            "approver": d.map(|d| d.approver_id.clone()),
            "role": d.map(|d| d.approver_role.clone()),
            "decided_at": d.map(|d| d.decided_at.clone()),
            "artifact_hash": rec.artifact_hash,
        });
        // Approved artifacts this one derives from: the gates that precede it in the
        // definition (requirements ← HLD ← LLD), also when reconciling a finished run.
        let earlier: Vec<&str> = def
            .states
            .iter()
            .take_while(|s| s.name != gate)
            .filter(|s| matches!(s.kind, StateKind::ApprovalGate { .. }))
            .map(|s| s.name.as_str())
            .collect();
        let mut sources = vec![];
        for (g, aid) in &inst.data.approved_gates {
            if !earlier.contains(&g.as_str()) {
                continue;
            }
            if let Ok(StateKind::ApprovalGate {
                artifact_from: af, ..
            }) = def.state(g).map(|s| &s.kind)
                && let Some(a) = inst.data.artifacts.get(af)
            {
                sources.push(json!({
                    "gate": g, "approval_id": aid.to_string(),
                    "artifact_id": a.id.to_string(), "name": a.name, "version": a.version, "hash": a.hash,
                }));
            }
        }
        if let Some(brd) = inst
            .input
            .get("brd_citation")
            .or_else(|| inst.input.get("citation"))
        {
            sources.push(json!({"brd": brd}));
        }
        self.materializer
            .materialize(&MaterializeRequest {
                scope: inst.scope,
                bundle: art.id,
                target_dir: dir.to_string(),
                workflow_instance: Some(inst.id.to_string()),
                gate: Some(gate.to_string()),
                approval,
                sources,
                replace_modified,
                actor: actor.to_string(),
                trace: inst.trace(),
            })
            .await
    }

    /// Hold the instance at an approved gate whose bundle could not be materialized.
    async fn block(&self, inst: &WorkflowInstance, gate: &str, reason: &str) -> Result<()> {
        let mut data = inst.data.clone();
        let since = match &data.blocked {
            Some(b) if b.gate == gate => b.since.clone(),
            _ => ts(self.db.now()),
        };
        let newly = data.blocked.as_ref().map(|b| b.reason != reason).unwrap_or(true);
        data.blocked = Some(BlockedGate {
            gate: gate.into(),
            reason: reason.into(),
            since,
        });
        let mut tx = self.db.write().await?;
        self.update_in(&mut tx, inst, None, Some(InstanceStatus::Running), &data)
            .await?;
        if newly {
            self.db
                .audit_in(
                    &mut tx,
                    AuditEntry::system(
                        ENGINE_COMPONENT,
                        "workflow.blocked",
                        "workflow_instance",
                        inst.id.to_string(),
                        AuditOutcome::Denied,
                    )
                    .scope(inst.scope)
                    .correlation(inst.trace().correlation)
                    .details(json!({"gate": gate, "reason": reason})),
                )
                .await?;
        }
        tx.commit().await
    }

    /// Every approval gate's document with its approval verification and verified
    /// materialization state (`workflow.read`; files on disk are re-hashed now).
    pub async fn documents(&self, actor: &Principal, id: WorkflowInstanceId) -> Result<Vec<GateDocument>> {
        let inst = self.get_for(actor, id).await?;
        let eff = self.effective(&inst).await?;
        let def = self
            .definition(&inst.definition_id, &inst.definition_version)
            .await?;
        let latest = self
            .latest_definition(&WorkflowDefinitionId::new(&inst.definition_id)?)
            .await
            .ok();
        let mut out = vec![];
        for g in def
            .states
            .iter()
            .filter(|s| matches!(s.kind, StateKind::ApprovalGate { .. }))
        {
            let check = self.check_gate(&eff, g).await;
            let approved = check.approval_id.is_some() && check.issues.is_empty();
            let artifact = inst.data.artifacts.get(&check.artifact_from).cloned();
            let target_dir = Self::target_dir(g, latest.as_ref());
            let materialization = match (&target_dir, &artifact) {
                (Some(dir), Some(a)) => {
                    let mut st = self.materializer.status(inst.scope, a.id, dir).await?;
                    if !approved && matches!(st.state.as_str(), "not_materialized" | "outdated") {
                        st.state = "awaiting_approval".into();
                        st.explanation = format!(
                            "Version {} is not approved yet. Only approved versions are written to {dir}; review it in the portal or with `eaeh approval review`.",
                            a.version
                        );
                    }
                    Some(st)
                }
                _ => None,
            };
            out.push(GateDocument {
                gate: g.name.clone(),
                artifact_from: check.artifact_from.clone(),
                artifact,
                approval: check,
                approved,
                target_dir,
                materialization,
                blocked: inst
                    .data
                    .blocked
                    .as_ref()
                    .filter(|b| b.gate == g.name)
                    .map(|b| b.reason.clone()),
            });
        }
        Ok(out)
    }

    /// Reconcile: materialize the approved version of every gate's bundle into the
    /// registered workspace (`workflow.start` on the project, like starting the run).
    /// Unapproved versions are skipped. An instance held at a gate resumes afterwards.
    pub async fn sync_documents(
        &self,
        actor: &Principal,
        id: WorkflowInstanceId,
        replace_modified: bool,
    ) -> Result<Vec<SyncOutcome>> {
        let inst = self.get_for(actor, id).await?;
        self.authz
            .authorize(actor, Action::WorkflowStart, Target::Scope(inst.scope))?;
        let eff = self.effective(&inst).await?;
        let def = self
            .definition(&inst.definition_id, &inst.definition_version)
            .await?;
        let latest = self
            .latest_definition(&WorkflowDefinitionId::new(&inst.definition_id)?)
            .await
            .ok();
        let mut out = vec![];
        for g in def
            .states
            .iter()
            .filter(|s| matches!(s.kind, StateKind::ApprovalGate { .. }))
        {
            let Some(dir) = Self::target_dir(g, latest.as_ref()) else {
                continue;
            };
            let check = self.check_gate(&eff, g).await;
            let (Some(aid), true) = (check.approval_id.as_ref(), check.issues.is_empty()) else {
                out.push(SyncOutcome {
                    gate: g.name.clone(),
                    target_dir: dir,
                    status: "skipped".into(),
                    detail: if check.approval_id.is_none() {
                        "not approved – only approved versions are materialized".into()
                    } else {
                        format!(
                            "approval not valid for the current version: {}",
                            check.issues.join("; ")
                        )
                    },
                    report: None,
                });
                continue;
            };
            let rec = self.approvals.get(aid.parse()?).await?;
            let res = self
                .materialize_gate(
                    &eff,
                    &def,
                    &g.name,
                    &check.artifact_from,
                    &dir,
                    &rec,
                    replace_modified,
                    actor.id.as_str(),
                )
                .await;
            out.push(match res {
                Ok(r) => SyncOutcome {
                    gate: g.name.clone(),
                    target_dir: dir,
                    status: "succeeded".into(),
                    detail: "materialized and verified".into(),
                    report: Some(r),
                },
                Err(e) => SyncOutcome {
                    gate: g.name.clone(),
                    target_dir: dir,
                    status: if e.kind == eaeh_core::ErrorKind::Conflict {
                        "conflict".into()
                    } else {
                        "failed".into()
                    },
                    detail: e.message,
                    report: None,
                },
            });
        }
        if inst.data.blocked.is_some() {
            self.drive(id).await?;
        }
        Ok(out)
    }

    /// Execute a work state (with gate checks, retries, timeout, cancellation).
    async fn run_work(&self, inst: &WorkflowInstance, state: &StateDef) -> Result<Option<Trigger>> {
        // Mandatory gate check, independent of the transition graph.
        // The approval must be valid *and* bound to the current version of the artifact
        // the gate reviewed, so that e.g. an LLD is never generated from an outdated HLD.
        let def = if state.requires_approval_of.is_empty() {
            None
        } else {
            Some(
                self.definition(&inst.definition_id, &inst.definition_version)
                    .await?,
            )
        };
        for gate in &state.requires_approval_of {
            let issues = match def.as_ref().and_then(|d| d.state(gate).ok()) {
                Some(g) if inst.data.approved_gates.contains_key(gate) => {
                    self.check_gate(inst, g).await.issues
                }
                Some(_) => vec!["not granted".to_string()],
                None => vec!["gate not defined".to_string()],
            };
            if !issues.is_empty() {
                let reason = format!(
                    "state '{}' requires approval of gate '{gate}', which has not been granted for the current artifact version ({})",
                    state.name,
                    issues.join("; ")
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
                        .details(json!({"state": state.name, "gate": gate, "issues": issues})),
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
