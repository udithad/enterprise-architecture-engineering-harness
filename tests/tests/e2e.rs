//! Full-stack end-to-end scenarios through the real composition root.
//!
//! The model provider is the labelled test double `ScriptedModelProvider` (D-04): it
//! returns pre-programmed responses; everything else (agents, gateway, policy, approvals,
//! budgets, workflow engine, isolation, audit, events) is production code.

use eaeh_agents::AgentTask;
use eaeh_core::secret::SecretsProvider;
use eaeh_core::{
    AgentDefinitionId, HarnessError, PrincipalId, PrincipalKind, ProjectAccess, Result, Role, Scope,
    SecretString, TaskId, TraceContext, WorkflowDefinitionId,
};
use eaeh_governance::{ApprovalDecision, ApprovalStatus, NewPrincipal};
use eaeh_runtime::{Harness, HarnessConfig};
use eaeh_testkit::{Script, ScriptedModelProvider};
use eaeh_workflow::InstanceStatus;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn config(name: &str, container: bool) -> HarnessConfig {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut cfg = HarnessConfig::load(&root.join("config/eaeh.toml")).unwrap();
    // Uses a scripted model provider: direct_provider mode (backward-compatible path).
    cfg.execution.mode = eaeh_agents::ExecutionMode::DirectProvider;
    // Under the target dir (inside $HOME) so container runtimes on macOS can mount workspaces.
    cfg.storage.data_dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("e2e-{name}-{}", uuid::Uuid::now_v7()));
    cfg.isolation.container_enabled = container;
    cfg.supervision.dispatch_interval_ms = 50;
    cfg
}

async fn people(h: &Harness) -> (eaeh_core::Principal, eaeh_core::Principal, Scope) {
    let (admin, project, _t) = h
        .identity
        .bootstrap(
            "acme",
            "platform",
            PrincipalId::new("admin").unwrap(),
            "Admin",
            30,
        )
        .await
        .unwrap();
    let mk = |id: &str, r: Role| NewPrincipal {
        id: PrincipalId::new(id).unwrap(),
        display_name: id.into(),
        kind: PrincipalKind::Human,
        roles: [r].into(),
        projects: ProjectAccess::Only([project].into()),
    };
    let op = h
        .identity
        .create_principal(&admin, mk("olivia", Role::Operator))
        .await
        .unwrap();
    let ap = h
        .identity
        .create_principal(&admin, mk("arthur", Role::Approver))
        .await
        .unwrap();
    (op, ap, Scope::new(admin.tenant, project))
}

#[tokio::test]
async fn agentic_workflow_with_planning_gate_and_full_traceability() {
    let provider = Arc::new(ScriptedModelProvider::new(
        "scripted",
        "scripted-model",
        vec![
            // planner agent (hybrid orchestration proposal)
            Script::json(
                json!({"steps": [{"task_kind": "analyze", "capabilities": ["analyze"], "input": {"focus": "requirements"}, "risk": "read_only"}], "rationale": "single analysis step"}),
            ),
            // analyst executes the plan step
            Script::tool("fs.list", json!({})),
            Script::json(json!({"summary": "workspace is empty; greenfield"})),
            // writer: design
            Script::tool(
                "fs.write",
                json!({"path": "design.md", "content": "# Design\nComponents: A, B"}),
            ),
            Script::json(json!({"summary": "design written"})),
            // writer: implement (after human approval)
            Script::tool(
                "fs.write",
                json!({"path": "impl.md", "content": "implemented A and B"}),
            ),
            Script::json(json!({"summary": "implementation complete"})),
        ],
    ));
    let h = Arc::new(
        Harness::builder(config("agentic", false))
            .model_provider(provider.clone())
            .build()
            .await
            .unwrap(),
    );
    h.start_background().await.unwrap();
    let (op, ap, scope) = people(&h).await;

    let id = h
        .engine
        .start(
            &op,
            &WorkflowDefinitionId::new("engineering.task").unwrap(),
            scope,
            json!({"brief": "build A and B"}),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    let st = h.engine.drive(id).await.unwrap();
    let inst = h.engine.get(id).await.unwrap();
    assert_eq!(
        st,
        InstanceStatus::WaitingApproval,
        "failure: {:?}",
        inst.data.failure
    );
    assert!(
        !inst.data.outputs.contains_key("implement"),
        "LLD/implementation must wait for approval"
    );
    assert_eq!(inst.data.plans.values().next().unwrap().steps.len(), 1);

    let approval = h
        .approvals
        .list(scope, Some(ApprovalStatus::Pending), None, 5)
        .await
        .unwrap()
        .remove(0);
    let design_art = &inst.data.artifacts["design"];
    assert_eq!(approval.artifact_hash.as_deref(), Some(design_art.hash.as_str()));
    // The authoring agent is recorded and excluded from approving.
    assert!(approval.authors.iter().any(|a| a.as_str().starts_with("agent:")));
    let st = h
        .engine
        .decide_approval(
            &ap,
            approval.id,
            ApprovalDecision::Approve,
            "design approved",
            Some(&design_art.hash),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    assert_eq!(st, InstanceStatus::Succeeded);
    assert_eq!(
        provider.remaining(),
        0,
        "every scripted model call was consumed exactly once"
    );

    // Traceability: artifact provenance links agent, model version, prompts, tools, context.
    let (meta, bytes) = h.artifacts.read(scope, design_art.id).await.unwrap();
    let prov = &meta.provenance;
    assert_eq!(prov.agent_definition.as_deref(), Some("writer@1.0.0"));
    assert_eq!(
        prov.models,
        vec!["scripted/scripted-model@scripted-model-scripted-1"]
    );
    assert!(prov.prompts.contains(&"system.agent-core@1.0.0".to_string()));
    assert!(prov.prompts.contains(&"agent.generic@1.0.0".to_string()));
    assert_eq!(prov.tool_invocations.len(), 1);
    assert!(prov.context_snapshot.is_some());
    assert_eq!(prov.workflow_instance.as_deref(), Some(id.to_string().as_str()));
    assert!(String::from_utf8(bytes).unwrap().contains("design written"));

    // One correlation id ties together the run's audit trail.
    let audit = h.db.audit_by_correlation(inst.trace().correlation).await.unwrap();
    let actions: std::collections::BTreeSet<&str> = audit.iter().map(|a| a.action.as_str()).collect();
    for a in [
        "workflow.start",
        "plan.accepted",
        "agent.instantiate",
        "tool.invoke",
        "agent.execute",
        "artifact.create",
        "approval.request",
        "approval.decide",
        "workflow.transition",
        "workflow.complete",
    ] {
        assert!(actions.contains(a), "missing audited action {a}: {actions:?}");
    }
    let usage: (i64,) =
        eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM model_usage WHERE correlation_id = ?")
            .bind(inst.trace().correlation.to_string())
            .fetch_one(h.db.pool())
            .await
            .unwrap();
    assert_eq!(usage.0, 7);
    assert!(h.db.verify_audit_chain().await.unwrap().valid);

    // Duplicate dispatch passes cause no duplicate side effects.
    h.dispatcher.drain(10).await.unwrap();
    let before = h.metrics.snapshot();
    h.dispatcher.drain(10).await.unwrap();
    assert_eq!(before, h.metrics.snapshot());
    h.shutdown().await;
}

#[tokio::test]
async fn agent_build_tool_runs_only_inside_container_isolation() {
    let probe = eaeh_isolation_probe().await;
    if !probe {
        return;
    }
    let provider = Arc::new(ScriptedModelProvider::new(
        "scripted",
        "scripted-model",
        vec![
            Script::tool(
                "exec.run",
                json!({"program": "/bin/echo", "args": ["hello-from-isolation"]}),
            ),
            Script::tool("exec.run", json!({"program": "/bin/sh", "args": ["-c", "id"]})),
            Script::json(json!({"summary": "built"})),
        ],
    ));
    let h = Arc::new(
        Harness::builder(config("container", true))
            .model_provider(provider.clone())
            .build()
            .await
            .unwrap(),
    );
    assert!(
        h.isolation_status
            .iter()
            .any(|(p, s)| p == "container" && s == "available"),
        "{:?}",
        h.isolation_status
    );
    let (op, _ap, scope) = people(&h).await;
    let ws = h.workspaces.create(scope, &op.id).await.unwrap();
    h.budgets
        .init("wf-build", &eaeh_governance::BudgetLimits::platform_default())
        .await
        .unwrap();
    let builder = h
        .agent_registry
        .latest(&AgentDefinitionId::new("builder").unwrap())
        .await
        .unwrap();
    let r = h
        .agents
        .run(
            &builder,
            AgentTask {
                task_id: TaskId::new(),
                kind: "build".into(),
                input: json!({}),
                scope,
                trace: TraceContext::new_root(),
                workflow_owner: Some("wf-build".into()),
                workspace: Some(ws),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        r.tool_invocations.len(),
        1,
        "allow-listed program ran; /bin/sh was refused"
    );
    let reqs = provider.requests();
    let data = &reqs[1].messages[1].content;
    assert!(data.contains("hello-from-isolation"), "{data}");
    assert!(
        data.contains("\\\"provider\\\":\\\"container\\\"") || data.contains("\"provider\":\"container\""),
        "{data}"
    );
    assert!(
        reqs[2].messages[1]
            .content
            .contains("not in the exec.run allowlist")
    );
}

/// Fail-closed container availability check shared with the isolation suite convention.
async fn eaeh_isolation_probe() -> bool {
    let ok = tokio::process::Command::new("podman")
        .args(["image", "inspect", "docker.io/library/alpine:3.20"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok && std::env::var("EAEH_SKIP_CONTAINER_TESTS").as_deref() == Ok("1") {
        eprintln!("SKIPPED (EAEH_SKIP_CONTAINER_TESTS=1): container runtime/image unavailable");
        return false;
    }
    assert!(
        ok,
        "container runtime required (set EAEH_SKIP_CONTAINER_TESTS=1 to skip explicitly)"
    );
    true
}

struct FixedSecrets;
impl SecretsProvider for FixedSecrets {
    fn name(&self) -> &str {
        "fixed-test"
    }
    fn get(&self, key: &str) -> Result<SecretString> {
        match key {
            "repo.token" => Ok(SecretString::new("ghp_SuperSecretRepoCredential42")),
            _ => Err(HarnessError::not_found("no secret")),
        }
    }
}

#[tokio::test]
/// Log redaction and log correlation are verified in `tests/tests/logging.rs` (own
/// process, global subscriber).
async fn secrets_never_reach_audit_events_or_agent_context() {
    const SECRET: &str = "ghp_SuperSecretRepoCredential42";
    let provider = Arc::new(ScriptedModelProvider::new(
        "scripted",
        "scripted-model",
        vec![
            Script::tool("fs.read", json!({"path": "config.txt"})),
            Script::json(json!({"summary": format!("found credential {SECRET}")})),
        ],
    ));
    let h = Arc::new(
        Harness::builder(config("secrets", false))
            .model_provider(provider.clone())
            .secrets(Arc::new(FixedSecrets))
            .build()
            .await
            .unwrap(),
    );
    let secret = h.secret("repo.token").unwrap();
    assert_eq!(format!("{secret:?}"), "[REDACTED]");
    let (op, _ap, scope) = people(&h).await;
    let ws = h.workspaces.create(scope, &op.id).await.unwrap();
    ws.acquire_lock("config.txt", "setup", chrono::Duration::minutes(1))
        .await
        .unwrap();
    ws.write_file(
        "setup",
        "config.txt",
        format!("token={SECRET}").as_bytes(),
        TraceContext::new_root(),
    )
    .await
    .unwrap();
    h.budgets
        .init("wf-s", &eaeh_governance::BudgetLimits::platform_default())
        .await
        .unwrap();
    let analyst = h
        .agent_registry
        .latest(&AgentDefinitionId::new("analyst").unwrap())
        .await
        .unwrap();
    let r = h
        .agents
        .run(
            &analyst,
            AgentTask {
                task_id: TaskId::new(),
                kind: "analyze".into(),
                input: json!({}),
                scope,
                trace: TraceContext::new_root(),
                workflow_owner: Some("wf-s".into()),
                workspace: Some(ws),
            },
        )
        .await
        .unwrap();

    // Agent context sent to the model.
    for req in provider.requests() {
        for m in req.messages {
            assert!(!m.content.contains(SECRET), "secret leaked into model context");
        }
    }
    // Model output returned to the harness is redacted too.
    assert!(!r.output.to_string().contains(SECRET));
    // Audit records.
    let audit = h.db.audit_records(None, 0, 10_000).await.unwrap();
    assert!(audit.iter().all(|a| !a.details.contains(SECRET)));
    // Task/approval/event tables.
    let dump: Vec<(String,)> = eaeh_store::sqlx::query_as("SELECT payload FROM outbox")
        .fetch_all(h.db.pool())
        .await
        .unwrap();
    assert!(dump.iter().all(|(p,)| !p.contains(SECRET)));
    let _ = tokio::time::timeout(Duration::from_secs(1), h.shutdown()).await;
}
