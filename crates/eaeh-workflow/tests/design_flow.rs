//! Phase 3 engine capabilities: request-changes revisions with bounded loops, final
//! rejection, input bindings, fail-closed quality gates, multi-file bundles bound to one
//! approval, and traceability recorded only for approved artifacts (AC3-11…AC3-14).

use async_trait::async_trait;
use eaeh_core::{
    AgentDefinitionId, AgentInstanceId, AutonomyLevel, ErrorKind, Principal, PrincipalId, ProjectAccess,
    ProjectId, Result, Role, Scope, TenantId, TraceContext, Untrusted, WorkflowDefinitionId,
};
use eaeh_extensibility::HookRegistry;
use eaeh_governance::authz::RbacAuthorizer;
use eaeh_governance::policy::RuleEngine;
use eaeh_governance::{ApprovalDecision, ApprovalManager, ApprovalStatus, BudgetLedger};
use eaeh_store::Db;
use eaeh_validation::{
    Finding, QualityGate, Severity, ValidationEngine, ValidationReport, ValidationSubject, Validator,
    ValidatorKind,
};
use eaeh_workflow::{
    InstanceStatus, PlanProposal, Planner, PlanningContext, TaskExecution, TaskExecutor, TaskOutput,
    TaskSpec, WorkflowDefinition, WorkflowEngine,
};
use eaeh_workspace::{ArtifactStore, Provenance, WorkspaceManager};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Returns scripted outputs per state (last one repeats) and records inputs.
#[derive(Default)]
struct Scripted {
    outputs: Mutex<HashMap<String, VecDeque<Value>>>,
    calls: Mutex<Vec<(String, Value, Option<Value>)>>,
    author: Mutex<Option<Principal>>,
}

impl Scripted {
    fn script(&self, state: &str, outs: Vec<Value>) {
        self.outputs.lock().unwrap().insert(state.into(), outs.into());
    }
    fn inputs(&self, state: &str) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.0 == state)
            .map(|c| c.1.clone())
            .collect()
    }
    fn tool_inputs(&self, state: &str) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.0 == state)
            .filter_map(|c| c.2.clone())
            .collect()
    }
}

#[async_trait]
impl TaskExecutor for Scripted {
    async fn execute(&self, t: TaskExecution) -> Result<TaskOutput> {
        let tool_input = match &t.spec {
            TaskSpec::Tool { input, .. } => Some(input.clone()),
            _ => None,
        };
        self.calls
            .lock()
            .unwrap()
            .push((t.state.clone(), t.input.clone(), tool_input));
        let out = {
            let mut m = self.outputs.lock().unwrap();
            let q = m.entry(t.state.clone()).or_default();
            if q.len() > 1 {
                q.pop_front().unwrap()
            } else {
                q.front().cloned().unwrap_or(json!({"state": t.state}))
            }
        };
        Ok(TaskOutput {
            output: out,
            authors: self
                .author
                .lock()
                .unwrap()
                .clone()
                .map(|p| p.id)
                .into_iter()
                .collect(),
            provenance: Provenance {
                produced_by: "agent:test".into(),
                standards: vec!["std/error-format@v1".into()],
                ..Default::default()
            },
        })
    }
}

struct NoPlanner;
#[async_trait]
impl Planner for NoPlanner {
    async fn propose(&self, _ctx: PlanningContext) -> Result<Untrusted<PlanProposal>> {
        Ok(Untrusted::new(PlanProposal {
            steps: vec![],
            rationale: String::new(),
        }))
    }
}

/// Deterministic test validator: output needs a non-empty `title`, and every standard
/// supplied by the platform must be acknowledged in `output.compliance`.
struct TitleAndStandards;
#[async_trait]
impl Validator for TitleAndStandards {
    fn id(&self) -> &str {
        "test.design"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::Deterministic
    }
    async fn validate(&self, s: &ValidationSubject) -> ValidationReport {
        let out = &s.content["output"];
        let ctx = &s.content["context"];
        let mut findings = vec![];
        let mut f = |m: String| {
            findings.push(Finding {
                validator: "test.design".into(),
                severity: Severity::Error,
                message: m,
                location: None,
                evidence: Value::Null,
            })
        };
        if out["title"].as_str().unwrap_or("").is_empty() {
            f("title missing".into());
        }
        for st in ctx["standards"].as_array().cloned().unwrap_or_default() {
            if !out["compliance"].as_array().is_some_and(|a| a.contains(&st)) {
                f(format!("standard {st} not addressed"));
            }
        }
        if ctx
            .get("requirements")
            .is_some_and(|r| r["document"] != "requirements")
        {
            f("context.requirements is not the approved requirements".into());
        }
        ValidationReport {
            validator: "test.design".into(),
            version: "1.0.0".into(),
            kind: ValidatorKind::Deterministic,
            findings,
        }
    }
}

struct Fx {
    _d: tempfile::TempDir,
    db: Db,
    engine: WorkflowEngine,
    exec: Arc<Scripted>,
    scope: Scope,
    operator: Principal,
    approver: Principal,
}

async fn fx() -> Fx {
    let d = tempfile::tempdir().unwrap();
    let db = Db::open(&d.path().join("w.db")).await.unwrap();
    let exec = Arc::new(Scripted::default());
    let scope = Scope::new(TenantId::new(), ProjectId::new());
    *exec.author.lock().unwrap() = Some(Principal::agent(
        AgentInstanceId::new(),
        AgentDefinitionId::new("architect").unwrap(),
        "1.0.0".into(),
        AutonomyLevel::ReadOnly,
        scope,
    ));
    let authz = Arc::new(RbacAuthorizer);
    let hooks = Arc::new(HookRegistry::new());
    let validation = Arc::new(ValidationEngine::new());
    validation.register(Arc::new(TitleAndStandards)).unwrap();
    let engine = WorkflowEngine::new(
        db.clone(),
        authz.clone(),
        Arc::new(RuleEngine::with_baseline().unwrap()),
        ApprovalManager::new(db.clone(), authz),
        BudgetLedger::new(db.clone()),
        hooks.clone(),
        exec.clone(),
        Arc::new(NoPlanner),
        ArtifactStore::new(db.clone(), &d.path().join("artifacts"), hooks.clone()).unwrap(),
        WorkspaceManager::new(db.clone(), &d.path().join("ws"), hooks).unwrap(),
    )
    .with_quality_gates(
        validation,
        vec![QualityGate {
            id: "design".into(),
            validators: vec!["test.design".into()],
            block_on: Severity::Error,
            semantic_can_block: false,
        }],
    );
    let human = |id: &str, role: Role| {
        Principal::human(
            PrincipalId::new(id).unwrap(),
            scope.tenant,
            [role].into(),
            ProjectAccess::All,
        )
    };
    Fx {
        _d: d,
        db,
        engine,
        exec,
        scope,
        operator: human("olivia", Role::Operator),
        approver: human("arthur", Role::Approver),
    }
}

fn wf(id: &str, states: Value) -> WorkflowDefinition {
    serde_json::from_value(
        json!({"id": id, "version": "1.0.0", "description": "test", "initial": "draft", "states": states}),
    )
    .unwrap()
}

/// draft (agent, gated) → render (tool bundle) → review gate → done; reject → rejected.
fn design_wf(max_revisions: u32) -> WorkflowDefinition {
    wf(
        "design.test",
        json!([
            {"name": "draft", "type": "agent_task", "task_kind": "design", "capabilities": ["design"], "max_autonomy": "read_only",
             "artifact": "design/model.json",
             "inputs": {"requirements": {"$input": "/requirements"}, "brief": {"$input": "/brief"}},
             "quality_gate": {"gate": "design", "context": {"requirements": {"$input": "/requirements"}}},
             "transitions": {"succeeded": "render", "failed": "failed"}},
            {"name": "render", "type": "tool_step", "tool": "architecture.render",
             "input": {"model": {"$output": "draft"}, "title": {"$output": "draft", "pointer": "/title"}, "model_artifact": {"$artifact": "draft"}},
             "artifact": "design/bundle", "bundle": true,
             "transitions": {"succeeded": "review", "failed": "failed"}},
            {"name": "review", "type": "approval_gate", "required_role": "approver", "artifact_from": "render", "max_revisions": max_revisions,
             "transitions": {"approved": "done", "changes_requested": "draft", "rejected": "rejected"}},
            {"name": "done", "type": "terminal", "outcome": "succeeded"},
            {"name": "rejected", "type": "terminal", "outcome": "failed"},
            {"name": "failed", "type": "terminal", "outcome": "failed"}
        ]),
    )
}

fn model(title: &str) -> Value {
    json!({"title": title, "compliance": ["std/error-format@v1"]})
}

fn bundle(title: &str) -> Value {
    json!({"files": {"design.md": format!("# {title}\n"), "diagrams/context.mmd": "flowchart LR\n  a --> b\n"},
           "trace": [{"from_kind": "requirement", "from_id": "REQ-F-001", "relation": "allocated_to", "to_kind": "hld_component", "to_id": "order-service"},
                     {"from_kind": "brd_section", "from_id": "brd/x@v1#2", "relation": "derives", "to_kind": "requirement", "to_id": "REQ-F-001"}]})
}

async fn start(f: &Fx, def: &WorkflowDefinition) -> eaeh_core::WorkflowInstanceId {
    f.engine.register_definition(None, def).await.unwrap();
    f.engine
        .start(
            &f.operator,
            &def.id,
            f.scope,
            json!({"requirements": {"document": "requirements", "n": 3}, "brief": "orders"}),
            TraceContext::new_root(),
        )
        .await
        .unwrap()
}

async fn pending(f: &Fx) -> (eaeh_core::ApprovalId, String) {
    let a = f
        .engine
        .approvals()
        .list(f.scope, Some(ApprovalStatus::Pending), None, 5)
        .await
        .unwrap()
        .remove(0);
    (a.id, a.artifact_hash.clone().unwrap())
}

async fn decide(f: &Fx, d: ApprovalDecision, comments: &str) -> InstanceStatus {
    let (id, hash) = pending(f).await;
    f.engine
        .decide_approval(
            &f.approver,
            id,
            d,
            comments,
            Some(&hash),
            TraceContext::new_root(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn request_changes_revises_with_feedback_and_is_bounded() {
    let f = fx().await;
    f.exec
        .script("draft", vec![model("v1"), model("v2"), model("v3")]);
    f.exec
        .script("render", vec![bundle("v1"), bundle("v2"), bundle("v3")]);
    let id = start(&f, &design_wf(2)).await;
    assert_eq!(f.engine.drive(id).await.unwrap(), InstanceStatus::WaitingApproval);

    // Comments are mandatory for a change request.
    let (aid, hash) = pending(&f).await;
    let e = f
        .engine
        .decide_approval(
            &f.approver,
            aid,
            ApprovalDecision::RequestChanges,
            " ",
            Some(&hash),
            TraceContext::new_root(),
        )
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::InvalidInput);

    assert_eq!(
        decide(&f, ApprovalDecision::RequestChanges, "add a deployment view").await,
        InstanceStatus::WaitingApproval
    );
    let second = f.exec.inputs("draft")[1].clone();
    assert_eq!(second["revision_feedback"]["review"], "add a deployment view");
    assert_eq!(
        second["outputs"]["draft"]["title"], "v1",
        "previous version is available for revision"
    );
    assert_eq!(
        decide(&f, ApprovalDecision::RequestChanges, "name the queue").await,
        InstanceStatus::WaitingApproval
    );
    assert_eq!(f.exec.inputs("draft").len(), 3);
    // Third request exceeds max_revisions = 2 ⇒ follows the rejected transition.
    assert_eq!(
        decide(&f, ApprovalDecision::RequestChanges, "still not right").await,
        InstanceStatus::Failed
    );
    let inst = f.engine.get(id).await.unwrap();
    assert_eq!(inst.state, "rejected");
    assert_eq!(inst.data.revisions["review"], 2);
    assert!(inst.data.feedback["review"].contains("revision limit (2) reached"));
    let audit: (i64,) =
        eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM audit_log WHERE action = 'workflow.revision'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(audit.0, 3);
    let decisions: Vec<(String,)> =
        eaeh_store::sqlx::query_as("SELECT decision FROM approval_decisions ORDER BY decided_at")
            .fetch_all(f.db.pool())
            .await
            .unwrap();
    assert_eq!(
        decisions.iter().map(|d| d.0.as_str()).collect::<Vec<_>>(),
        vec!["request_changes"; 3]
    );
    // No trace links: nothing was approved.
    assert!(
        f.engine
            .trace(&f.operator, f.scope, None, 1)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn reject_is_final_and_gates_without_a_revision_transition_fall_back() {
    let f = fx().await;
    f.exec.script("draft", vec![model("v1")]);
    f.exec.script("render", vec![bundle("v1")]);
    let id = start(&f, &design_wf(3)).await;
    f.engine.drive(id).await.unwrap();
    assert_eq!(
        decide(&f, ApprovalDecision::Reject, "wrong problem").await,
        InstanceStatus::Failed
    );
    assert_eq!(f.engine.get(id).await.unwrap().state, "rejected");
    assert_eq!(f.exec.inputs("draft").len(), 1, "reject never re-runs the author");

    // A Phase 1/2 style gate (no changes_requested transition) treats a change request
    // like its rejected transition (revise loop) – backward compatible.
    let legacy = wf(
        "legacy.test",
        json!([
            {"name": "draft", "type": "agent_task", "task_kind": "design", "capabilities": ["design"], "max_autonomy": "read_only",
             "artifact": "legacy.json", "transitions": {"succeeded": "review"}},
            {"name": "review", "type": "approval_gate", "required_role": "approver", "artifact_from": "draft",
             "transitions": {"approved": "done", "rejected": "draft"}},
            {"name": "done", "type": "terminal", "outcome": "succeeded"}
        ]),
    );
    let id = start(&f, &legacy).await;
    f.engine.drive(id).await.unwrap();
    assert_eq!(
        decide(&f, ApprovalDecision::RequestChanges, "tweak").await,
        InstanceStatus::WaitingApproval
    );
    assert_eq!(f.engine.get(id).await.unwrap().data.revisions["review"], 1);
}

#[tokio::test]
async fn bindings_resolve_outputs_inputs_and_artifacts() {
    let f = fx().await;
    f.exec.script("draft", vec![model("Orders HLD")]);
    f.exec.script("render", vec![bundle("Orders HLD")]);
    let id = start(&f, &design_wf(3)).await;
    f.engine.drive(id).await.unwrap();
    let draft = &f.exec.inputs("draft")[0];
    assert_eq!(draft["inputs"]["brief"], "orders");
    assert_eq!(draft["inputs"]["requirements"]["n"], 3);
    let render = &f.exec.tool_inputs("render")[0];
    assert_eq!(render["model"]["title"], "Orders HLD");
    assert_eq!(render["title"], "Orders HLD");
    let art = &f.engine.get(id).await.unwrap().data.artifacts["draft"];
    assert_eq!(render["model_artifact"]["hash"], json!(art.hash));

    // Static validation: bindings may only reference work states; gates must exist.
    let mut bad = design_wf(3);
    bad.id = WorkflowDefinitionId::new("bad.binding").unwrap();
    bad.states[0].kind = serde_json::from_value(
        json!({"type": "agent_task", "task_kind": "d", "capabilities": ["d"], "max_autonomy": "read_only",
        "inputs": {"x": {"$output": "review"}}}),
    )
    .unwrap();
    assert!(
        f.engine
            .register_definition(None, &bad)
            .await
            .unwrap_err()
            .message
            .contains("not a work state")
    );
    let mut bad = design_wf(3);
    bad.id = WorkflowDefinitionId::new("bad.gate").unwrap();
    bad.states[0].quality_gate.as_mut().unwrap().gate = "nope".into();
    assert!(
        f.engine
            .register_definition(None, &bad)
            .await
            .unwrap_err()
            .message
            .contains("unknown quality gate 'nope'")
    );
}

#[tokio::test]
async fn quality_gate_fails_closed_before_anything_is_stored() {
    let f = fx().await;
    // Missing title and the platform-supplied standard is not addressed.
    f.exec
        .script("draft", vec![json!({"title": "", "compliance": []})]);
    let id = start(&f, &design_wf(3)).await;
    assert_eq!(f.engine.drive(id).await.unwrap(), InstanceStatus::Failed);
    let inst = f.engine.get(id).await.unwrap();
    assert_eq!(inst.state, "failed");
    assert!(
        inst.data.artifacts.is_empty(),
        "nothing stored when the gate fails"
    );
    let reason = inst.data.failure.unwrap_or_default();
    assert!(
        reason.contains("quality gate 'design' failed")
            && reason.contains("title missing")
            && reason.contains("std/error-format@v1"),
        "{reason}"
    );
    let ev: (i64,) =
        eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM outbox WHERE event_type = 'validation.failed'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(ev.0, 1);
    let au: (String,) =
        eaeh_store::sqlx::query_as("SELECT outcome FROM audit_log WHERE action = 'workflow.quality_gate'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(au.0, "failed");

    // A passing output records the gate result in artifact provenance.
    let f = fx().await;
    f.exec.script("draft", vec![model("ok")]);
    f.exec.script("render", vec![bundle("ok")]);
    let id = start(&f, &design_wf(3)).await;
    f.engine.drive(id).await.unwrap();
    let art = f.engine.get(id).await.unwrap().data.artifacts["draft"].clone();
    let (meta, _) = f.engine.artifacts().read(f.scope, art.id).await.unwrap();
    assert_eq!(
        meta.provenance.quality_gates,
        vec!["design:passed (0 blocking, 0 advisory)"]
    );
}

#[tokio::test]
async fn bundles_bind_all_files_to_one_approval_and_trace_is_recorded_on_approval() {
    let f = fx().await;
    f.exec.script("draft", vec![model("v1")]);
    f.exec.script("render", vec![bundle("v1")]);
    let id = start(&f, &design_wf(3)).await;
    f.engine.drive(id).await.unwrap();
    let inst = f.engine.get(id).await.unwrap();
    let manifest_ref = inst.data.artifacts["render"].clone();
    assert_eq!(manifest_ref.name, "design/bundle");
    let (_, bytes) = f.engine.artifacts().read(f.scope, manifest_ref.id).await.unwrap();
    let manifest: Value = serde_json::from_slice(&bytes).unwrap();
    let md = &manifest["files"]["design.md"];
    let (meta, content) = f
        .engine
        .artifacts()
        .read(f.scope, md["artifact_id"].as_str().unwrap().parse().unwrap())
        .await
        .unwrap();
    assert_eq!(meta.name, "design/bundle/design.md");
    assert_eq!(meta.content_hash, md["hash"].as_str().unwrap());
    assert_eq!(content, b"# v1\n");
    assert_eq!(
        manifest["files"]["diagrams/context.mmd"]["media_type"],
        "text/vnd.mermaid"
    );
    // The approval binds the manifest hash, which covers every file hash.
    let (aid, hash) = pending(&f).await;
    assert_eq!(hash, manifest_ref.hash);
    assert!(
        f.engine
            .trace(&f.operator, f.scope, None, 1)
            .await
            .unwrap()
            .is_empty()
    );
    let st = f
        .engine
        .decide_approval(
            &f.approver,
            aid,
            ApprovalDecision::Approve,
            "ok",
            Some(&hash),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    assert_eq!(st, InstanceStatus::Succeeded);

    let all = f.engine.trace(&f.operator, f.scope, None, 1).await.unwrap();
    assert_eq!(all.len(), 2);
    assert!(
        all.iter()
            .all(|l| l.artifact_hash == manifest_ref.hash && l.approval_id == aid.to_string())
    );
    // Transitive query: from the BRD section to the HLD component via the requirement.
    let from_brd = f
        .engine
        .trace(&f.operator, f.scope, Some("brd/x@v1#2"), 2)
        .await
        .unwrap();
    assert!(
        from_brd.iter().any(|l| l.to_id == "order-service"),
        "{from_brd:?}"
    );
    let one_hop = f
        .engine
        .trace(&f.operator, f.scope, Some("brd/x@v1#2"), 1)
        .await
        .unwrap();
    assert_eq!(one_hop.len(), 1);
    // Other projects see nothing.
    let other = Scope::new(f.scope.tenant, ProjectId::new());
    assert!(
        f.engine
            .trace(&f.operator, other, None, 1)
            .await
            .unwrap()
            .is_empty()
    );
    let ev: (i64,) =
        eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM outbox WHERE event_type = 'trace.links_recorded'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(ev.0, 1);
}
