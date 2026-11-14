//! Automated external AI execution (ADR-0026): the External Execution Coordinator runs the
//! design workflow's AI tasks without a person prompting a client; people only decide the
//! three approval gates.
//!
//! The AI client here is a **test double** (`FakeClient`) implementing the provider-neutral
//! `ExternalExecutor` interface: it submits the labelled example outputs
//! (`examples/design/*.json`) through the same governed calls the task-scoped MCP server
//! uses. Everything else (coordinator, leases, retries, recovery, validation, workflow
//! engine, approvals, audit) is production code. The Claude Code adapter itself is tested
//! against fake `claude` programs in `eaeh-runtime` and against the real CLI in the
//! acceptance run (see the implementation report).

mod common;
use async_trait::async_trait;
use common::*;
use eaeh_core::{
    Principal, PrincipalId, Scope, TaskId, TraceContext, WorkflowDefinitionId, WorkflowInstanceId,
};
use eaeh_governance::{ApprovalDecision, ApprovalStatus};
use eaeh_runtime::Harness;
use eaeh_runtime::coordinator::{
    self as co, AuthState, CancellationToken, ClientReport, Coordinator, ExecFailure, ExecResult,
    ExecutionRequest, ExternalExecutor, FailureClass, McpLaunch, NoNotifier, Preflight, Until,
};
use eaeh_workflow::InstanceStatus;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
enum Act {
    /// Read the task, call its tools, submit the valid example output.
    Good,
    /// Submit an output the deterministic validator rejects, then end the session.
    Invalid,
    /// End with a classified failure.
    Fail(FailureClass),
    /// Never finish (until cancelled).
    Hang,
}

struct FakeClient {
    h: Arc<Harness>,
    who: Principal,
    acts: Mutex<VecDeque<Act>>,
    auth: AuthState,
    seen: Mutex<Vec<String>>,
}

impl FakeClient {
    fn new(w: &World, acts: Vec<Act>) -> Arc<Self> {
        Arc::new(Self {
            h: w.h.clone(),
            who: w.operator.clone(),
            acts: Mutex::new(acts.into()),
            auth: AuthState::Subscription {
                plan: Some("pro".into()),
            },
            seen: Mutex::new(vec![]),
        })
    }

    fn with_auth(w: &World, auth: AuthState) -> Arc<Self> {
        Arc::new(Self {
            h: w.h.clone(),
            who: w.operator.clone(),
            acts: Mutex::new(VecDeque::new()),
            auth,
            seen: Mutex::new(vec![]),
        })
    }
}

fn output_for(state: &str) -> Value {
    match state {
        "analyse" | "refine" => example("requirements.json"),
        "hld" => example("hld.json"),
        "lld" => example("lld.json"),
        other => panic!("unexpected stage {other}"),
    }
}

#[async_trait]
impl ExternalExecutor for FakeClient {
    fn provider(&self) -> &str {
        "fake_client"
    }
    fn client_label(&self) -> &str {
        "fake-client"
    }
    async fn preflight(&self) -> Preflight {
        Preflight {
            provider: "fake_client".into(),
            available: true,
            program: Some("fake".into()),
            version: Some("1.0.0".into()),
            auth: self.auth.clone(),
            detail: "test double".into(),
        }
    }
    async fn execute(&self, req: &ExecutionRequest, cancel: CancellationToken) -> ExecResult {
        let act = self.acts.lock().unwrap().pop_front().unwrap_or(Act::Good);
        self.seen.lock().unwrap().push(req.task_id.clone());
        assert!(req.prompt.contains(&req.task_id));
        assert!(
            req.workdir.is_dir(),
            "a private working directory exists during the attempt"
        );
        let id: TaskId = req.task_id.parse().unwrap();
        let report = ClientReport {
            num_turns: Some(3),
            duration_ms: Some(10),
            notional_cost_usd: Some(0.0),
            auth_source: Some("none".into()),
            final_text: "EAEH_RESULT".into(),
        };
        let fail = |class| ExecFailure {
            class,
            detail: format!("{class:?} (test double)"),
            report: None,
        };
        match act {
            Act::Fail(c) => Err(fail(c)),
            Act::Hang => {
                cancel.cancelled().await;
                Err(fail(FailureClass::Cancelled))
            }
            Act::Good | Act::Invalid => {
                let view = self.h.agents.external_task(&self.who, id).await.unwrap();
                if matches!(view.state.as_str(), "analyse" | "refine") {
                    self.h
                        .agents
                        .external_tool(&self.who, id, "knowledge.read", json!({"document": BRD}))
                        .await
                        .unwrap();
                }
                let mut out = output_for(&view.state);
                if matches!(act, Act::Invalid) {
                    out["out_of_scope"] = json!([out["out_of_scope"][0]]);
                }
                self.h
                    .submit_external(&self.who, id, &out, self.client_label())
                    .await
                    .unwrap();
                Ok(report)
            }
        }
    }
}

fn quick(cfg: &mut eaeh_runtime::HarnessConfig) {
    let c = &mut cfg.execution.coordinator;
    c.poll_interval_ms = 200;
    c.backoff_secs = 1;
    c.lease_secs = 30;
    c.max_attempts = 3;
    c.notify = false;
}

async fn ext_world() -> World {
    world_cfg(ExecutionMode::ExternalAi, vec![], "", quick).await
}

fn launch() -> McpLaunch {
    McpLaunch {
        program: PathBuf::from("/nonexistent/eaeh"),
        args: vec!["mcp".into(), "--task".into(), "{task}".into()],
        env: vec![],
    }
}

async fn coordinator(w: &World, client: Arc<FakeClient>) -> eaeh_core::Result<Coordinator> {
    coordinator_as(w, w.operator.clone(), w.scope, client).await
}

async fn coordinator_as(
    w: &World,
    who: Principal,
    scope: Scope,
    client: Arc<FakeClient>,
) -> eaeh_core::Result<Coordinator> {
    Coordinator::register(
        w.h.clone(),
        who,
        scope,
        client,
        launch(),
        Arc::new(NoNotifier),
        Arc::new(|l: &str| eprintln!("[coordinator] {l}")),
    )
    .await
}

/// One automatic stretch, like a background worker started after a person's action.
async fn auto(w: &World, client: Arc<FakeClient>) -> co::RunSummary {
    let c = coordinator(w, client).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(60),
        c.run(CancellationToken::new(), Until::Idle),
    )
    .await
    .expect("the coordinator reached a stop")
    .unwrap()
}

async fn start_design(w: &World) -> WorkflowInstanceId {
    let id =
        w.h.engine
            .start(
                &w.operator,
                &WorkflowDefinitionId::new("engineering.design").unwrap(),
                w.scope,
                json!({"brd": BRD}),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    let st = w.h.engine.drive(id).await.unwrap();
    let inst = w.h.engine.get(id).await.unwrap();
    assert_eq!(st, InstanceStatus::WaitingExternal, "{:?}", inst.data.failure);
    id
}

async fn status(w: &World, id: WorkflowInstanceId) -> (InstanceStatus, String) {
    let i = w.h.engine.get(id).await.unwrap();
    (i.status, i.state)
}

async fn count(w: &World, sql: &'static str) -> i64 {
    eaeh_store::sqlx::query_as::<_, (i64,)>(sql)
        .fetch_one(w.h.db.pool())
        .await
        .unwrap()
        .0
}

async fn executions(w: &World) -> Vec<co::ExecutionInfo> {
    let mut v = co::executions(&w.h, &w.operator, w.scope, None, 500)
        .await
        .unwrap();
    v.reverse();
    v
}

async fn dispatch(w: &World, task: &str) -> (String, i64) {
    eaeh_store::sqlx::query_as::<_, (String, i64)>(
        "SELECT dispatch_state, dispatch_attempts FROM external_tasks WHERE id = ?",
    )
    .bind(task)
    .fetch_one(w.h.db.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn brd_to_lld_runs_automatically_and_people_only_decide_the_three_gates() {
    let w = ext_world().await;
    let id = start_design(&w).await;

    // BRD analysis and refinement run back to back without anyone prompting a client.
    let s = auto(&w, FakeClient::new(&w, vec![])).await;
    assert_eq!((s.attempts, s.succeeded), (2, 2), "{s:?}");
    assert_eq!(
        status(&w, id).await,
        (InstanceStatus::WaitingApproval, "approve_requirements".into())
    );
    let inst = w.h.engine.get(id).await.unwrap();
    let md = read_bundle_file(&w, &inst, "render_requirements", "requirements.md").await;
    assert!(md.contains("BRD sections: 8"), "requirements document rendered");

    // Nothing happens at a gate: the coordinator has no way to decide it.
    let s = auto(&w, FakeClient::new(&w, vec![])).await;
    assert_eq!(s.attempts, 0);
    assert_eq!(status(&w, id).await.0, InstanceStatus::WaitingApproval);

    // Gate 1 (human) → HLD automatically → gate 2.
    let (st, _) = decide(&w, ApprovalDecision::Approve, "requirements approved").await;
    assert_eq!(st, InstanceStatus::WaitingExternal);
    assert_eq!(auto(&w, FakeClient::new(&w, vec![])).await.succeeded, 1);
    assert_eq!(
        status(&w, id).await,
        (InstanceStatus::WaitingApproval, "approve_hld".into())
    );

    // Gate 2 → LLD automatically → gate 3 → done.
    decide(&w, ApprovalDecision::Approve, "HLD approved").await;
    assert_eq!(auto(&w, FakeClient::new(&w, vec![])).await.succeeded, 1);
    assert_eq!(
        status(&w, id).await,
        (InstanceStatus::WaitingApproval, "approve_lld".into())
    );
    let (st, _) = decide(&w, ApprovalDecision::Approve, "LLD approved").await;
    assert_eq!(st, InstanceStatus::Succeeded);

    // Every gate was decided by the human approver; the coordinator decided nothing.
    let gates: Vec<_> =
        w.h.approvals
            .list(w.scope, Some(ApprovalStatus::Approved), None, 50)
            .await
            .unwrap()
            .into_iter()
            .filter(|a| a.workflow_instance_id.is_some())
            .collect();
    assert_eq!(gates.len(), 3);
    for g in &gates {
        let g = w.h.approvals.get(g.id).await.unwrap();
        assert_eq!(g.decision.as_ref().unwrap().approver_id, "arthur");
        assert!(
            g.authors.iter().any(|a| a.as_str() == "olivia"),
            "the worker's operator is a co-author"
        );
    }
    // Execution transparency: 4 attempts, all succeeded, provider and auth recorded.
    let ex = executions(&w).await;
    assert_eq!(ex.len(), 4);
    assert!(ex.iter().all(|e| e.outcome == "succeeded"
        && e.provider == "fake_client"
        && e.execution_mode == "external_ai"
        && e.auth_status == "subscription login (pro)"));
    assert_eq!(
        ex.iter().map(|e| e.stage.as_str()).collect::<Vec<_>>(),
        vec!["analyse", "refine", "hld", "lld"]
    );
    // No EAEH-managed model inference.
    assert!(w.h.models.models().is_empty());
    assert_eq!(count(&w, "SELECT COUNT(*) FROM model_usage").await, 0);
    assert_eq!(
        count(&w, "SELECT COUNT(*) FROM audit_log WHERE action = 'model.invoke'").await,
        0
    );
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'external_execution.finish'"
        )
        .await,
        4
    );
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'external_client.preflight'"
        )
        .await,
        4 + 4,
        "one check per worker start and per attempt"
    );
    // Provenance marks the client.
    let inst = w.h.engine.get(id).await.unwrap();
    let (meta, _) =
        w.h.artifacts
            .read(w.scope, inst.data.artifacts["lld"].id)
            .await
            .unwrap();
    assert_eq!(meta.provenance.models, vec!["external:fake-client".to_string()]);
    assert!(w.h.db.verify_audit_chain().await.unwrap().valid);
}

#[tokio::test]
async fn requested_changes_are_corrected_automatically_and_the_old_version_is_kept() {
    let w = ext_world().await;
    let id = start_design(&w).await;
    auto(&w, FakeClient::new(&w, vec![])).await;
    decide(&w, ApprovalDecision::Approve, "ok").await;
    auto(&w, FakeClient::new(&w, vec![])).await;
    let (first_gate, first_hash, _) = pending_gate(&w).await;
    let first_artifact = w.h.engine.get(id).await.unwrap().data.artifacts["hld"].clone();

    let (st, state) = decide(&w, ApprovalDecision::RequestChanges, "Add a caching decision").await;
    assert_eq!((st, state.as_str()), (InstanceStatus::WaitingExternal, "hld"));
    // The correction task carries the reviewer's comments.
    let open =
        w.h.agents
            .external_tasks(&w.operator, w.scope, true)
            .await
            .unwrap();
    let pkg =
        w.h.agents
            .external_task(&w.operator, open[0].task_id.parse().unwrap())
            .await
            .unwrap()
            .package
            .unwrap();
    assert!(
        pkg.to_string().contains("Add a caching decision"),
        "comments reach the correction task"
    );

    assert_eq!(auto(&w, FakeClient::new(&w, vec![])).await.succeeded, 1);
    assert_eq!(
        status(&w, id).await,
        (InstanceStatus::WaitingApproval, "approve_hld".into())
    );
    let (second_gate, second_hash, _) = pending_gate(&w).await;
    assert_ne!(first_gate, second_gate, "a new approval for the revised version");
    let second_artifact = w.h.engine.get(id).await.unwrap().data.artifacts["hld"].clone();
    assert!(second_artifact.version > first_artifact.version);
    // The earlier version and the reviewer's decision are preserved.
    assert!(w.h.artifacts.read(w.scope, first_artifact.id).await.is_ok());
    let old = w.h.approvals.get(first_gate).await.unwrap();
    assert_eq!(old.status, ApprovalStatus::ChangesRequested);
    assert_eq!(old.decision.unwrap().comments, "Add a caching decision");
    let _ = (first_hash, second_hash);
}

#[tokio::test]
async fn failed_attempts_retry_with_backoff_then_stop_and_can_be_retried_by_a_person() {
    let w = world_cfg(ExecutionMode::ExternalAi, vec![], "", |c| {
        quick(c);
        c.execution.coordinator.max_attempts = 2;
    })
    .await;
    let id = start_design(&w).await;
    let client = FakeClient::new(&w, vec![Act::Fail(FailureClass::Timeout), Act::Invalid]);
    let t0 = std::time::Instant::now();
    let s = auto(&w, client).await;
    assert_eq!((s.attempts, s.succeeded), (2, 0));
    assert!(
        t0.elapsed() >= Duration::from_millis(900),
        "the retry waited for its backoff"
    );
    let ex = executions(&w).await;
    assert_eq!(
        ex.iter()
            .map(|e| (e.outcome.as_str(), e.failure_class.as_deref()))
            .collect::<Vec<_>>(),
        vec![("retry", Some("timeout")), ("exhausted", Some("invalid_output"))]
    );
    assert!(
        ex[1]
            .detail
            .as_deref()
            .unwrap()
            .contains("last submission rejected")
    );
    let task = ex[0].task_id.clone();
    assert_eq!(dispatch(&w, &task).await, ("exhausted".into(), 2));
    // The workflow is not failed; it waits, and nothing retries on its own.
    assert_eq!(status(&w, id).await.0, InstanceStatus::WaitingExternal);
    assert_eq!(auto(&w, FakeClient::new(&w, vec![])).await.attempts, 0);

    // A person makes it eligible again; it then completes.
    co::retry_task(&w.h, &w.operator, task.parse().unwrap())
        .await
        .unwrap();
    let s = auto(&w, FakeClient::new(&w, vec![])).await;
    assert_eq!(s.succeeded, 2);
    assert_eq!(status(&w, id).await.0, InstanceStatus::WaitingApproval);
}

#[tokio::test]
async fn subscription_limits_and_sign_in_problems_block_instead_of_retrying() {
    let w = ext_world().await;
    let id = start_design(&w).await;
    let s = auto(&w, FakeClient::new(&w, vec![Act::Fail(FailureClass::UsageLimit)])).await;
    assert_eq!(s.attempts, 1, "never retried automatically");
    let ex = executions(&w).await;
    assert_eq!(
        (ex[0].outcome.as_str(), ex[0].failure_class.as_deref()),
        ("blocked", Some("usage_limit"))
    );
    assert_eq!(dispatch(&w, &ex[0].task_id).await.0, "blocked");
    assert_eq!(status(&w, id).await.0, InstanceStatus::WaitingExternal);

    // A client that is not signed in with a subscription is refused before any work.
    for (auth, class) in [
        (AuthState::NotLoggedIn, "auth_unavailable"),
        (
            AuthState::ApiKeyBilling {
                reason: "authentication method 'api_key'".into(),
            },
            "billing_unsafe",
        ),
    ] {
        let e = coordinator(&w, FakeClient::with_auth(&w, auth))
            .await
            .unwrap_err();
        assert!(e.message.contains(class), "{e}");
    }
    let refused = count(
        &w,
        "SELECT COUNT(*) FROM audit_log WHERE action = 'external_client.preflight' AND outcome = 'denied'",
    )
    .await;
    assert_eq!(refused, 2);
}

#[tokio::test]
async fn only_external_ai_mode_and_operators_can_run_a_coordinator() {
    let w = world_cfg(ExecutionMode::DirectProvider, vec![], "", quick).await;
    let e = coordinator(&w, FakeClient::new(&w, vec![])).await.unwrap_err();
    assert!(e.message.contains("only in external_ai mode"), "{e}");

    let w = ext_world().await;
    // Approvers (and stewards) cannot execute hand-offs: no coordinator as an approver.
    let e = coordinator_as(&w, w.approver.clone(), w.scope, FakeClient::new(&w, vec![]))
        .await
        .unwrap_err();
    assert_eq!(e.kind, eaeh_core::ErrorKind::Forbidden, "{e}");
    // Unsupported providers are refused, never substituted.
    let mut cfg = w.h.config.execution.coordinator.clone();
    cfg.provider = "github_copilot".into();
    assert!(co::executor_from_config(&cfg).is_err());
}

#[tokio::test]
async fn one_worker_per_project_and_projects_are_isolated() {
    let w = ext_world().await;
    let a = start_design(&w).await;
    let b = start_design(&w).await;
    let first = coordinator(&w, FakeClient::new(&w, vec![])).await.unwrap();
    let e = coordinator(&w, FakeClient::new(&w, vec![])).await.unwrap_err();
    assert_eq!(e.kind, eaeh_core::ErrorKind::Conflict, "{e}");

    // A worker for another project sees none of this project's tasks.
    let other =
        w.h.identity
            .create_project(&w.admin, w.scope.tenant, "other")
            .await
            .unwrap();
    let other_scope = Scope::new(w.scope.tenant, other);
    let client = FakeClient::new(&w, vec![]);
    let c = coordinator_as(&w, w.operator.clone(), other_scope, client.clone())
        .await
        .unwrap();
    let s = c.run(CancellationToken::new(), Until::Idle).await.unwrap();
    assert_eq!(s.attempts, 0);
    assert!(client.seen.lock().unwrap().is_empty());

    // Two runs of the same project are executed one after the other without interference.
    let client = FakeClient::new(&w, vec![]);
    let s = first.run(CancellationToken::new(), Until::Idle).await.unwrap();
    assert_eq!(s.succeeded, 4);
    for id in [a, b] {
        assert_eq!(status(&w, id).await.0, InstanceStatus::WaitingApproval);
    }
    let ex = executions(&w).await;
    let mut tasks: Vec<_> = ex.iter().map(|e| e.task_id.clone()).collect();
    tasks.sort();
    tasks.dedup();
    assert_eq!(tasks.len(), 4, "each task executed exactly once");
    drop(client);

    // A duplicate submission after acceptance is refused.
    let e =
        w.h.submit_external(
            &w.operator,
            ex[0].task_id.parse().unwrap(),
            &example("requirements.json"),
            "x",
        )
        .await
        .unwrap_err();
    assert_eq!(e.kind, eaeh_core::ErrorKind::Conflict);
}

#[tokio::test]
async fn a_stop_request_cancels_the_session_and_gives_the_attempt_back() {
    let w = ext_world().await;
    start_design(&w).await;
    let c = Arc::new(
        coordinator(&w, FakeClient::new(&w, vec![Act::Hang]))
            .await
            .unwrap(),
    );
    let c2 = c.clone();
    let run = tokio::spawn(async move { c2.run(CancellationToken::new(), Until::Stopped).await });
    for _ in 0..50 {
        if executions(&w).await.iter().any(|e| e.outcome == "running") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(co::request_stop(&w.h, &w.operator, w.scope).await.unwrap(), 1);
    let s = tokio::time::timeout(Duration::from_secs(20), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(s.stopped_because.contains("stop requested"), "{s:?}");
    let ex = executions(&w).await;
    assert_eq!(
        (ex[0].outcome.as_str(), ex[0].failure_class.as_deref()),
        ("interrupted", Some("interrupted"))
    );
    assert_eq!(dispatch(&w, &ex[0].task_id).await, ("pending".into(), 0));
    let workers = co::workers(&w.h, &w.operator, w.scope).await.unwrap();
    assert_eq!(workers[0].status, "stopped");
    assert!(!workers[0].live);
}

#[tokio::test]
async fn a_crashed_worker_is_recovered_by_the_next_one() {
    let w = ext_world().await;
    let id = start_design(&w).await;
    let crashed = Arc::new(
        coordinator(&w, FakeClient::new(&w, vec![Act::Hang]))
            .await
            .unwrap(),
    );
    let dead_id = crashed.worker_id().to_string();
    let c2 = crashed.clone();
    let run = tokio::spawn(async move { c2.run(CancellationToken::new(), Until::Stopped).await });
    for _ in 0..50 {
        if executions(&w).await.iter().any(|e| e.outcome == "running") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Simulate a process crash / machine restart: the task is aborted without any cleanup
    // and its heartbeat and lease age out.
    run.abort();
    let _ = run.await;
    eaeh_store::sqlx::query(
        "UPDATE external_workers SET heartbeat_at = '2000-01-01T00:00:00.000000Z' WHERE id = ?",
    )
    .bind(&dead_id)
    .execute(w.h.db.pool())
    .await
    .unwrap();
    eaeh_store::sqlx::query("UPDATE external_tasks SET lease_expires_at = '2000-01-01T00:00:00.000000Z'")
        .execute(w.h.db.pool())
        .await
        .unwrap();
    let s = auto(&w, FakeClient::new(&w, vec![])).await;
    assert_eq!(s.succeeded, 2);
    assert_eq!(status(&w, id).await.0, InstanceStatus::WaitingApproval);
    let ex = executions(&w).await;
    assert_eq!(ex[0].outcome, "interrupted");
    assert_eq!(ex[0].worker, dead_id);
    let dead = co::workers(&w.h, &w.operator, w.scope)
        .await
        .unwrap()
        .into_iter()
        .find(|x| x.id == dead_id)
        .unwrap();
    assert_eq!(dead.status, "failed");
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'external_worker.recover'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn execution_records_hold_no_credentials() {
    let w = ext_world().await;
    start_design(&w).await;
    auto(
        &w,
        FakeClient::new(&w, vec![Act::Fail(FailureClass::AuthUnavailable)]),
    )
    .await;
    // Tokens of the people involved never appear in execution records or the audit log.
    let tokens: Vec<String> = eaeh_store::sqlx::query_as::<_, (String,)>("SELECT token_hash FROM tokens")
        .fetch_all(w.h.db.pool())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.0)
        .collect();
    let dump: Vec<(String,)> = eaeh_store::sqlx::query_as(
        "SELECT COALESCE(detail,'') || auth_status FROM external_executions UNION ALL SELECT details FROM audit_log WHERE action LIKE 'external_%'",
    )
    .fetch_all(w.h.db.pool())
    .await
    .unwrap();
    let text: String = dump.into_iter().map(|d| d.0).collect::<Vec<_>>().join("\n");
    assert!(!text.contains("eaeh_"), "no EAEH token material");
    assert!(
        !text.to_ascii_lowercase().contains("sk-ant"),
        "no API key material"
    );
    for t in tokens {
        assert!(!text.contains(&t));
    }
    let _ = PrincipalId::new("x");
}
