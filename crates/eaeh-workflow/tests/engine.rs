//! Workflow engine: gates, rejection/revision, gate-bypass prevention, retry/timeout,
//! cancellation, crash recovery and planner validation (AC-03, AC-04, AC-11).

use async_trait::async_trait;
use eaeh_core::{
    AgentDefinitionId, AgentInstanceId, AutonomyLevel, ErrorKind, HarnessError, Principal, PrincipalId,
    ProjectAccess, ProjectId, Result, RiskClass, Role, Scope, TenantId, TraceContext, Untrusted,
    WorkflowDefinitionId,
};
use eaeh_extensibility::HookRegistry;
use eaeh_governance::authz::RbacAuthorizer;
use eaeh_governance::policy::RuleEngine;
use eaeh_governance::{ApprovalDecision, ApprovalManager, ApprovalStatus, ApprovalSubject, BudgetLedger};
use eaeh_store::Db;
use eaeh_workflow::{
    InstanceStatus, PlanProposal, PlanStep, Planner, PlanningContext, TaskExecution, TaskExecutor,
    TaskOutput, TaskSpec, WorkflowDefinition, WorkflowEngine,
};
use eaeh_workspace::{ArtifactStore, Provenance, WorkspaceManager};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

#[derive(Clone)]
enum Behaviour {
    Ok,
    FailTimes(u32),
    Slow(Duration),
    BlockUntilNotified,
}

#[derive(Default)]
struct FakeExecutor {
    behaviours: Mutex<HashMap<String, Behaviour>>,
    calls: Mutex<Vec<(String, Value)>>,
    started: Notify,
    release: Notify,
    author: Mutex<Option<Principal>>,
}

impl FakeExecutor {
    fn set(&self, state: &str, b: Behaviour) {
        self.behaviours.lock().unwrap().insert(state.into(), b);
    }
    fn calls_for(&self, state: &str) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| s == state)
            .map(|(_, v)| v.clone())
            .collect()
    }
}

#[async_trait]
impl TaskExecutor for FakeExecutor {
    async fn execute(&self, t: TaskExecution) -> Result<TaskOutput> {
        self.calls
            .lock()
            .unwrap()
            .push((t.state.clone(), t.input.clone()));
        let b = self
            .behaviours
            .lock()
            .unwrap()
            .get(&t.state)
            .cloned()
            .unwrap_or(Behaviour::Ok);
        match b {
            Behaviour::Ok => {}
            Behaviour::FailTimes(n) => {
                if n > 0 {
                    self.set(&t.state, Behaviour::FailTimes(n - 1));
                    return Err(HarnessError::unavailable("transient"));
                }
            }
            Behaviour::Slow(d) => tokio::time::sleep(d).await,
            Behaviour::BlockUntilNotified => {
                self.started.notify_one();
                self.release.notified().await;
            }
        }
        let author = self
            .author
            .lock()
            .unwrap()
            .clone()
            .map(|p| p.id)
            .into_iter()
            .collect();
        let kind = match &t.spec {
            TaskSpec::Agent { task_kind, .. } => task_kind.clone(),
            TaskSpec::Tool { tool, .. } => tool.to_string(),
        };
        Ok(TaskOutput {
            output: json!({"state": t.state, "kind": kind, "feedback": t.input.get("revision_feedback")}),
            authors: author,
            provenance: Provenance {
                produced_by: "agent:test".into(),
                ..Default::default()
            },
        })
    }
}

struct FixedPlanner(PlanProposal);
#[async_trait]
impl Planner for FixedPlanner {
    async fn propose(&self, _ctx: PlanningContext) -> Result<Untrusted<PlanProposal>> {
        Ok(Untrusted::new(self.0.clone()))
    }
}

struct Fx {
    _d: tempfile::TempDir,
    db: Db,
    engine: WorkflowEngine,
    exec: Arc<FakeExecutor>,
    scope: Scope,
    operator: Principal,
    approver: Principal,
    agent: Principal,
}

fn engine(db: &Db, dir: &Path, exec: Arc<FakeExecutor>, plan: PlanProposal) -> WorkflowEngine {
    let authz = Arc::new(RbacAuthorizer);
    let hooks = Arc::new(HookRegistry::new());
    WorkflowEngine::new(
        db.clone(),
        authz.clone(),
        Arc::new(RuleEngine::with_baseline().unwrap()),
        ApprovalManager::new(db.clone(), authz),
        BudgetLedger::new(db.clone()),
        hooks.clone(),
        exec,
        Arc::new(FixedPlanner(plan)),
        ArtifactStore::new(db.clone(), &dir.join("artifacts"), hooks.clone()).unwrap(),
        WorkspaceManager::new(db.clone(), &dir.join("ws"), hooks).unwrap(),
    )
}

fn default_plan() -> PlanProposal {
    PlanProposal {
        steps: vec![
            PlanStep {
                task_kind: "analyze".into(),
                capabilities: ["analyze".into()].into(),
                input: json!({"part": 1}),
                risk: RiskClass::ReadOnly,
            },
            PlanStep {
                task_kind: "write".into(),
                capabilities: ["write".into()].into(),
                input: json!({"part": 2}),
                risk: RiskClass::WorkspaceWrite,
            },
        ],
        rationale: "two steps".into(),
    }
}

async fn fx_with(plan: PlanProposal) -> Fx {
    let d = tempfile::tempdir().unwrap();
    let db = Db::open(&d.path().join("w.db")).await.unwrap();
    let exec = Arc::new(FakeExecutor::default());
    let scope = Scope::new(TenantId::new(), ProjectId::new());
    let agent = Principal::agent(
        AgentInstanceId::new(),
        AgentDefinitionId::new("designer").unwrap(),
        "1.0.0".into(),
        AutonomyLevel::WorkspaceModify,
        scope,
    );
    *exec.author.lock().unwrap() = Some(agent.clone());
    let engine = engine(&db, d.path(), exec.clone(), plan);
    let operator = Principal::human(
        PrincipalId::new("olivia").unwrap(),
        scope.tenant,
        [Role::Operator].into(),
        ProjectAccess::All,
    );
    let approver = Principal::human(
        PrincipalId::new("arthur").unwrap(),
        scope.tenant,
        [Role::Approver].into(),
        ProjectAccess::All,
    );
    Fx {
        _d: d,
        db,
        engine,
        exec,
        scope,
        operator,
        approver,
        agent,
    }
}

async fn fx() -> Fx {
    fx_with(default_plan()).await
}

fn gated_definition(id: &str) -> WorkflowDefinition {
    serde_json::from_value(json!({
        "id": id, "version": "1.0.0", "description": "design, review, implement", "initial": "design",
        "states": [
            {"name": "design", "type": "agent_task", "task_kind": "design", "capabilities": ["design"], "max_autonomy": "workspace_modify",
             "artifact": "design/hld.json", "transitions": {"succeeded": "review", "failed": "failed"}},
            {"name": "review", "type": "approval_gate", "required_role": "approver", "artifact_from": "design",
             "transitions": {"approved": "implement", "rejected": "design"}},
            {"name": "implement", "type": "agent_task", "task_kind": "implement", "capabilities": ["implement"], "max_autonomy": "workspace_modify",
             "requires_approval_of": ["review"], "transitions": {"succeeded": "done", "failed": "failed"}},
            {"name": "done", "type": "terminal", "outcome": "succeeded"},
            {"name": "failed", "type": "terminal", "outcome": "failed"}
        ]
    }))
    .unwrap()
}

async fn start(f: &Fx, def: &WorkflowDefinition) -> eaeh_core::WorkflowInstanceId {
    f.engine.register_definition(None, def).await.unwrap();
    f.engine
        .start(
            &f.operator,
            &def.id,
            f.scope,
            json!({"brief": "x"}),
            TraceContext::new_root(),
        )
        .await
        .unwrap()
}

#[test]
fn definition_validation() {
    let mut d = gated_definition("v");
    d.validate().unwrap();
    d.states[1] = serde_json::from_value(
        json!({"name": "review", "type": "approval_gate", "required_role": "approver",
        "artifact_from": "implement", "transitions": {"approved": "implement", "rejected": "design"}}),
    )
    .unwrap();
    assert!(
        d.validate().is_err(),
        "gate must review a state that produces an artifact"
    );
    let mut d = gated_definition("v");
    d.states[2].requires_approval_of = vec!["design".into()];
    assert!(
        d.validate().is_err(),
        "requires_approval_of must reference a gate"
    );
    let mut d = gated_definition("v");
    d.states.retain(|s| s.name != "done" && s.name != "failed");
    assert!(d.validate().is_err());
}

#[tokio::test]
async fn gate_blocks_until_human_approval_and_enforces_sod() {
    let f = fx().await;
    let id = start(&f, &gated_definition("wf.gated")).await;
    assert_eq!(f.engine.drive(id).await.unwrap(), InstanceStatus::WaitingApproval);
    assert!(
        f.exec.calls_for("implement").is_empty(),
        "implementation must not start before approval"
    );
    let pending = f
        .engine
        .approvals()
        .list(f.scope, Some(ApprovalStatus::Pending), None, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    let a = &pending[0];
    let hash = a.artifact_hash.clone().unwrap();
    let t = TraceContext::new_root();
    // Author (agent) and operator cannot approve; wrong artifact hash rejected.
    assert_eq!(
        f.engine
            .decide_approval(&f.agent, a.id, ApprovalDecision::Approve, "", Some(&hash), t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
    assert_eq!(
        f.engine
            .decide_approval(&f.operator, a.id, ApprovalDecision::Approve, "", Some(&hash), t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
    assert_eq!(
        f.engine
            .decide_approval(&f.approver, a.id, ApprovalDecision::Approve, "", Some("0000"), t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        f.engine.get(id).await.unwrap().status,
        InstanceStatus::WaitingApproval
    );
    let st = f
        .engine
        .decide_approval(
            &f.approver,
            a.id,
            ApprovalDecision::Approve,
            "approved design",
            Some(&hash),
            t,
        )
        .await
        .unwrap();
    assert_eq!(st, InstanceStatus::Succeeded);
    assert_eq!(f.exec.calls_for("implement").len(), 1);
    let h = f.engine.history(id).await.unwrap();
    let path: Vec<String> = h
        .iter()
        .map(|t| format!("{}->{}", t.from_state, t.to_state))
        .collect();
    assert_eq!(
        path,
        vec!["design->review", "review->implement", "implement->done"]
    );
    assert_eq!(h[1].actor_id, "arthur");
}

#[tokio::test]
async fn rejection_returns_to_revision_with_history() {
    let f = fx().await;
    let id = start(&f, &gated_definition("wf.reject")).await;
    f.engine.drive(id).await.unwrap();
    let first = f
        .engine
        .approvals()
        .list(f.scope, Some(ApprovalStatus::Pending), None, 10)
        .await
        .unwrap()[0]
        .clone();
    let t = TraceContext::new_root();
    let st = f
        .engine
        .decide_approval(
            &f.approver,
            first.id,
            ApprovalDecision::Reject,
            "add security section",
            first.artifact_hash.as_deref(),
            t,
        )
        .await
        .unwrap();
    assert_eq!(
        st,
        InstanceStatus::WaitingApproval,
        "re-designed and back at the gate"
    );
    let design_calls = f.exec.calls_for("design");
    assert_eq!(design_calls.len(), 2);
    assert_eq!(
        design_calls[1]["revision_feedback"]["review"],
        "add security section"
    );
    let inst = f.engine.get(id).await.unwrap();
    let art = &inst.data.artifacts["design"];
    assert_eq!(art.version, 2, "revised artifact is a new version");
    let history = f
        .engine
        .approvals()
        .history(&ApprovalSubject::WorkflowGate {
            instance: id,
            gate: "review".into(),
            artifact: None,
            artifact_hash: None,
        })
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].status, ApprovalStatus::Rejected);
    assert_eq!(
        history[0].decision.as_ref().unwrap().comments,
        "add security section"
    );
    assert_eq!(history[1].status, ApprovalStatus::Pending);
    assert_eq!(history[1].artifact_hash.as_deref(), Some(art.hash.as_str()));
    assert_ne!(history[0].artifact_hash, history[1].artifact_hash);
}

#[tokio::test]
async fn gate_cannot_be_bypassed_by_graph() {
    let f = fx().await;
    // Malformed-but-valid graph: design routes straight to implement, skipping the gate.
    let mut def = gated_definition("wf.bypass");
    def.states[0]
        .transitions
        .insert(eaeh_workflow::Trigger::Succeeded, "implement".into());
    let id = start(&f, &def).await;
    assert_eq!(f.engine.drive(id).await.unwrap(), InstanceStatus::Failed);
    assert!(
        f.exec.calls_for("implement").is_empty(),
        "gated state never executed"
    );
    let inst = f.engine.get(id).await.unwrap();
    assert!(
        inst.data
            .failure
            .unwrap()
            .contains("requires approval of gate 'review'")
    );
    let denied: (i64,) = eaeh_store::sqlx::query_as(
        "SELECT COUNT(*) FROM audit_log WHERE action = 'workflow.gate_check' AND outcome = 'denied'",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(denied.0, 1);
}

#[tokio::test]
async fn retry_and_timeout() {
    let f = fx().await;
    let mut def = gated_definition("wf.retry");
    def.states[0].retry.max_attempts = 3;
    def.states[0].retry.backoff_ms = 1;
    f.exec.set("design", Behaviour::FailTimes(2));
    let id = start(&f, &def).await;
    assert_eq!(f.engine.drive(id).await.unwrap(), InstanceStatus::WaitingApproval);
    assert_eq!(f.exec.calls_for("design").len(), 3);

    let f = fx().await;
    let mut def = gated_definition("wf.timeout");
    def.states[0].timeout_secs = Some(1);
    def.states[0]
        .transitions
        .insert(eaeh_workflow::Trigger::TimedOut, "failed".into());
    f.exec.set("design", Behaviour::Slow(Duration::from_secs(10)));
    let id = start(&f, &def).await;
    let started = std::time::Instant::now();
    assert_eq!(f.engine.drive(id).await.unwrap(), InstanceStatus::Failed);
    assert!(started.elapsed() < Duration::from_secs(5));
    let h = f.engine.history(id).await.unwrap();
    assert_eq!(h[0].trigger, "timed_out");
}

#[tokio::test]
async fn cancellation_waiting_and_running() {
    let f = fx().await;
    let id = start(&f, &gated_definition("wf.cancel1")).await;
    f.engine.drive(id).await.unwrap();
    assert_eq!(
        f.engine.cancel(&f.operator, id).await.unwrap(),
        InstanceStatus::Cancelled
    );
    let viewer = Principal::human(
        PrincipalId::new("v").unwrap(),
        f.scope.tenant,
        [Role::Viewer].into(),
        ProjectAccess::All,
    );
    let id2 = start(&f, &gated_definition("wf.cancel2")).await;
    assert_eq!(
        f.engine.cancel(&viewer, id2).await.unwrap_err().kind,
        ErrorKind::Forbidden
    );

    let f = Arc::new(fx().await);
    f.exec.set("design", Behaviour::BlockUntilNotified);
    let id = start(&f, &gated_definition("wf.cancel3")).await;
    let f2 = f.clone();
    let h = tokio::spawn(async move { f2.engine.drive(id).await });
    f.exec.started.notified().await;
    f.engine.cancel(&f.operator, id).await.unwrap();
    assert_eq!(h.await.unwrap().unwrap(), InstanceStatus::Cancelled);
}

#[tokio::test]
async fn interrupted_workflow_recovers_without_duplicate_side_effects() {
    let d = tempfile::tempdir().unwrap();
    let db = Db::open(&d.path().join("r.db")).await.unwrap();
    let scope = Scope::new(TenantId::new(), ProjectId::new());
    let operator = Principal::human(
        PrincipalId::new("olivia").unwrap(),
        scope.tenant,
        [Role::Operator].into(),
        ProjectAccess::All,
    );
    let def: WorkflowDefinition = serde_json::from_value(json!({
        "id": "wf.recover", "version": "1.0.0", "description": "three steps", "initial": "s1",
        "states": [
            {"name": "s1", "type": "agent_task", "task_kind": "a", "capabilities": ["a"], "max_autonomy": "read_only", "transitions": {"succeeded": "s2"}},
            {"name": "s2", "type": "agent_task", "task_kind": "b", "capabilities": ["b"], "max_autonomy": "read_only", "transitions": {"succeeded": "s3"}},
            {"name": "s3", "type": "agent_task", "task_kind": "c", "capabilities": ["c"], "max_autonomy": "read_only", "transitions": {"succeeded": "done"}},
            {"name": "done", "type": "terminal", "outcome": "succeeded"}
        ]
    }))
    .unwrap();

    // Process 1: crashes while s2 is executing.
    let exec1 = Arc::new(FakeExecutor::default());
    exec1.set("s2", Behaviour::BlockUntilNotified);
    let e1 = Arc::new(engine(&db, d.path(), exec1.clone(), default_plan()));
    e1.register_definition(None, &def).await.unwrap();
    let id = e1
        .start(
            &operator,
            &WorkflowDefinitionId::new("wf.recover").unwrap(),
            scope,
            json!({}),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    let e1c = e1.clone();
    let h = tokio::spawn(async move { e1c.drive(id).await });
    exec1.started.notified().await;
    h.abort(); // simulated crash
    let _ = h.await;
    drop(e1);
    let (status,): (String,) = eaeh_store::sqlx::query_as("SELECT status FROM tasks WHERE state_name = 's2'")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(status, "running");

    // Process 2: recovers and completes.
    let exec2 = Arc::new(FakeExecutor::default());
    let e2 = engine(&db, d.path(), exec2.clone(), default_plan());
    assert_eq!(e2.recover().await.unwrap(), vec![id]);
    assert_eq!(e2.drive(id).await.unwrap(), InstanceStatus::Succeeded);
    assert_eq!(exec1.calls_for("s1").len(), 1);
    assert!(
        exec2.calls_for("s1").is_empty(),
        "completed step is not re-executed after recovery"
    );
    assert_eq!(exec2.calls_for("s2").len(), 1, "interrupted step resumes");
    assert_eq!(exec2.calls_for("s3").len(), 1);
    let rec: (i64,) =
        eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM audit_log WHERE action = 'workflow.recover'")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(rec.0, 1);
    assert!(db.verify_audit_chain().await.unwrap().valid);
}

fn planned_definition(id: &str) -> WorkflowDefinition {
    serde_json::from_value(json!({
        "id": id, "version": "1.0.0", "description": "planned", "initial": "plan",
        "states": [
            {"name": "plan", "type": "planned", "allowed_task_kinds": ["analyze", "write"], "max_steps": 3, "max_autonomy": "workspace_modify",
             "artifact": "plan/result.json", "transitions": {"succeeded": "done", "failed": "failed"}},
            {"name": "done", "type": "terminal", "outcome": "succeeded"},
            {"name": "failed", "type": "terminal", "outcome": "failed"}
        ]
    }))
    .unwrap()
}

#[tokio::test]
async fn hybrid_orchestration_validates_planner_proposals() {
    let f = fx().await;
    let id = start(&f, &planned_definition("wf.plan.ok")).await;
    assert_eq!(f.engine.drive(id).await.unwrap(), InstanceStatus::Succeeded);
    let calls = f.exec.calls_for("plan");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1]["previous_steps"].as_array().unwrap().len(), 1);

    for (name, bad) in [
        (
            "disallowed-kind",
            PlanStep {
                task_kind: "deploy".into(),
                capabilities: ["deploy".into()].into(),
                input: json!({}),
                risk: RiskClass::ReadOnly,
            },
        ),
        (
            "high-risk",
            PlanStep {
                task_kind: "write".into(),
                capabilities: ["write".into()].into(),
                input: json!({}),
                risk: RiskClass::Deployment,
            },
        ),
    ] {
        let f = fx_with(PlanProposal {
            steps: vec![bad],
            rationale: name.into(),
        })
        .await;
        let id = start(&f, &planned_definition(&format!("wf.plan.{name}"))).await;
        assert_eq!(
            f.engine.drive(id).await.unwrap(),
            InstanceStatus::Failed,
            "{name}"
        );
        assert!(
            f.exec.calls_for("plan").is_empty(),
            "{name}: rejected plan must not execute"
        );
        let rejected: (i64,) =
            eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM audit_log WHERE action = 'plan.rejected'")
                .fetch_one(f.db.pool())
                .await
                .unwrap();
        assert_eq!(rejected.0, 1);
    }

    let too_many = PlanProposal {
        steps: default_plan().steps.into_iter().cycle().take(4).collect(),
        rationale: String::new(),
    };
    let f = fx_with(too_many).await;
    let id = start(&f, &planned_definition("wf.plan.long")).await;
    assert_eq!(f.engine.drive(id).await.unwrap(), InstanceStatus::Failed);
}

#[tokio::test]
async fn start_requires_authorization_and_scope() {
    let f = fx().await;
    let def = gated_definition("wf.auth");
    f.engine.register_definition(None, &def).await.unwrap();
    let viewer = Principal::human(
        PrincipalId::new("v").unwrap(),
        f.scope.tenant,
        [Role::Viewer].into(),
        ProjectAccess::All,
    );
    let t = TraceContext::new_root();
    assert_eq!(
        f.engine
            .start(&viewer, &def.id, f.scope, json!({}), t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
    let other_tenant = Scope::new(TenantId::new(), f.scope.project);
    assert_eq!(
        f.engine
            .start(&f.operator, &def.id, other_tenant, json!({}), t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
    assert_eq!(
        f.engine
            .start(&f.agent, &def.id, f.scope, json!({}), t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
    let id = f
        .engine
        .start(&f.operator, &def.id, f.scope, json!({}), t)
        .await
        .unwrap();
    let outsider = Principal::human(
        PrincipalId::new("x").unwrap(),
        TenantId::new(),
        [Role::Viewer].into(),
        ProjectAccess::All,
    );
    assert_eq!(
        f.engine.get_for(&outsider, id).await.unwrap_err().kind,
        ErrorKind::NotFound
    );
}
