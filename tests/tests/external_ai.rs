//! External AI execution (ADR-0025): BRD → requirements → HLD → LLD with every agent task
//! handed off to an external AI client and validated by EAEH. These tests cover:
//!
//! * the hand-off package contains what a provider would have received;
//! * governed task-tool calls run as the agent;
//! * invalid submissions are fed back and cost a correction attempt;
//! * the submitter is a co-author and cannot approve;
//! * authorization and project isolation;
//! * **no model provider is ever invoked**, and a direct-provider runtime will not complete a
//!   handed-off task.
//!
//! The "client" here is the test itself, submitting the labelled example outputs
//! (`examples/design/*.json`). Everything else is production code.

mod common;
use common::*;
use eaeh_core::{
    PrincipalId, PrincipalKind, ProjectAccess, Role, Scope, TaskId, TraceContext, WorkflowDefinitionId,
};
use eaeh_governance::{ApprovalDecision, NewPrincipal};
use eaeh_testkit::Script;
use eaeh_workflow::InstanceStatus;
use serde_json::{Value, json};

const CLIENT: &str = "claude-code";

async fn open_task(w: &World) -> (TaskId, Value) {
    let open =
        w.h.agents
            .external_tasks(&w.operator, w.scope, true)
            .await
            .unwrap();
    assert_eq!(open.len(), 1, "{open:#?}");
    let id: TaskId = open[0].task_id.parse().unwrap();
    let view = w.h.agents.external_task(&w.operator, id).await.unwrap();
    (id, view.package.unwrap())
}

async fn submit(w: &World, id: TaskId, out: &Value) -> (bool, InstanceStatus, Vec<String>) {
    let (sub, st) = w.h.submit_external(&w.operator, id, out, CLIENT).await.unwrap();
    (sub.accepted, st, sub.feedback)
}

async fn read_brd(w: &World, id: TaskId) {
    let out =
        w.h.agents
            .external_tool(&w.operator, id, "knowledge.read", json!({"document": BRD}))
            .await
            .unwrap();
    assert_eq!(out["hits"].as_array().unwrap().len(), 8);
}

async fn model_usage_rows(w: &World) -> i64 {
    eaeh_store::sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM model_usage")
        .fetch_one(w.h.db.pool())
        .await
        .unwrap()
        .0
}

#[tokio::test]
async fn external_ai_lifecycle_brd_to_lld_without_any_model_call() {
    // A provider is offered to the builder, as if one were enabled; external_ai mode must ignore it.
    let w = world_in(
        ExecutionMode::ExternalAi,
        vec![Script::json(json!({"must": "never be used"}))],
        "",
    )
    .await;
    assert_eq!(w.h.execution_mode, ExecutionMode::ExternalAi);
    assert!(w.h.models.models().is_empty(), "no provider registered");
    assert_eq!(w.h.ignored_providers, vec!["scripted"]);

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
    assert_eq!(
        w.h.engine.drive(id).await.unwrap(),
        InstanceStatus::WaitingExternal
    );

    // ---- business analyst: the package holds what a provider would have received.
    let (t1, pkg) = open_task(&w).await;
    assert_eq!(pkg["agent"]["reference"], "business-analyst@1.0.0");
    assert_eq!(pkg["execution_mode"], "external_ai");
    let sys = pkg["instructions"].as_str().unwrap();
    assert!(sys.contains(&format!("MANDATORY ENTERPRISE STANDARD [{STD}]")));
    assert!(sys.contains("APPROVED SKILL [requirements-analysis@1.0.0]"));
    assert!(
        pkg["data"]
            .as_str()
            .unwrap()
            .contains("must be one JSON object valid against")
    );
    assert_eq!(pkg["context"]["classification"], "confidential");
    let tools: Vec<&str> = pkg["allowed_tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(tools, vec!["knowledge.read", "knowledge.search", "design.check"]);

    // Approvers may see that work is pending, never the package (project data) or act on it.
    assert_eq!(
        w.h.agents
            .external_tasks(&w.approver, w.scope, true)
            .await
            .unwrap()
            .len(),
        1
    );
    let e = w.h.agents.external_task(&w.approver, t1).await.unwrap_err();
    assert!(e.message.contains("task.execute_external"), "{e}");
    assert!(
        w.h.submit_external(&w.approver, t1, &example("requirements.json"), CLIENT)
            .await
            .is_err()
    );
    // The agent's allow-list binds the client too: fs.write is not this agent's tool.
    let e =
        w.h.agents
            .external_tool(&w.operator, t1, "fs.write", json!({"path": "x", "content": "y"}))
            .await
            .unwrap_err();
    assert!(e.message.contains("fs.write"), "{e}");

    // An incomplete draft is rejected by the deterministic validator and fed back.
    read_brd(&w, t1).await;
    let good = example("requirements.json");
    let mut first = good.clone();
    first["out_of_scope"] = json!([good["out_of_scope"][0]]);
    let (ok, st, fb) = submit(&w, t1, &first).await;
    assert!(!ok);
    assert_eq!(st, InstanceStatus::WaitingExternal);
    assert!(
        fb.last().unwrap().contains("[requirements.set]") && fb.last().unwrap().contains(&format!("{BRD}#7")),
        "{fb:?}"
    );
    let (ok, st, _) = submit(&w, t1, &good).await;
    assert!(ok);
    assert_eq!(
        st,
        InstanceStatus::WaitingExternal,
        "next hand-off: requirements engineer"
    );

    // ---- requirements engineer
    let (t2, pkg) = open_task(&w).await;
    assert_ne!(t1, t2);
    assert_eq!(pkg["agent"]["reference"], "requirements-engineer@1.0.0");
    read_brd(&w, t2).await;
    // Submitted by someone who is both operator and approver …
    let pat =
        w.h.identity
            .create_principal(
                &w.admin,
                NewPrincipal {
                    id: PrincipalId::new("pat").unwrap(),
                    display_name: "pat".into(),
                    kind: PrincipalKind::Human,
                    roles: [Role::Operator, Role::Approver].into(),
                    projects: ProjectAccess::All,
                },
            )
            .await
            .unwrap();
    let (sub, st) = w.h.submit_external(&pat, t2, &good, CLIENT).await.unwrap();
    assert!(sub.accepted);
    assert_eq!(st, InstanceStatus::WaitingApproval);
    // … who therefore cannot approve the result.
    let (gid, hash, _) = pending_gate(&w).await;
    let e =
        w.h.engine
            .decide_approval(
                &pat,
                gid,
                ApprovalDecision::Approve,
                "self",
                Some(&hash),
                TraceContext::new_root(),
            )
            .await
            .unwrap_err();
    assert_eq!(e.kind, eaeh_core::ErrorKind::Forbidden, "{e}");

    // The submitter is a co-author of the gated artifact (cannot approve it).
    let (gate_id, _, gate) = pending_gate(&w).await;
    assert_eq!(gate, "approve_requirements");
    let rec = w.h.approvals.get(gate_id).await.unwrap();
    for p in ["olivia", "pat"] {
        assert!(rec.authors.iter().any(|a| a.as_str() == p), "{:?}", rec.authors);
    }
    let inst = w.h.engine.get(id).await.unwrap();
    let md = read_bundle_file(&w, &inst, "render_requirements", "requirements.md").await;
    assert!(md.contains("BRD sections: 8 · covered: 6 · excluded: 2 · uncovered: 0"));
    let (meta, _) =
        w.h.artifacts
            .read(w.scope, inst.data.artifacts["refine"].id)
            .await
            .unwrap();
    assert_eq!(meta.provenance.models, vec![format!("external:{CLIENT}")]);
    assert!(
        meta.provenance
            .inputs
            .iter()
            .any(|i| i.starts_with("external-task-package:sha256:"))
    );
    assert!(meta.provenance.inputs.contains(&"submitted-by:pat".to_string()));

    // ---- HLD and LLD
    let (st, gate) = decide(&w, ApprovalDecision::Approve, "requirements approved").await;
    assert_eq!((st, gate.as_str()), (InstanceStatus::WaitingExternal, "hld"));
    let (t3, pkg) = open_task(&w).await;
    assert_eq!(pkg["agent"]["reference"], "solution-architect@1.0.0");
    assert!(
        pkg["data"].as_str().unwrap().contains("REQ-F-006"),
        "approved requirements are bound"
    );
    let (ok, st, _) = submit(&w, t3, &example("hld.json")).await;
    assert!(ok);
    assert_eq!(st, InstanceStatus::WaitingApproval);
    let (st, _) = decide(&w, ApprovalDecision::Approve, "HLD approved").await;
    assert_eq!(st, InstanceStatus::WaitingExternal);
    let (t4, _) = open_task(&w).await;
    let (ok, st, _) = submit(&w, t4, &example("lld.json")).await;
    assert!(ok);
    assert_eq!(st, InstanceStatus::WaitingApproval);
    let (st, _) = decide(&w, ApprovalDecision::Approve, "LLD approved").await;
    assert_eq!(st, InstanceStatus::Succeeded);

    // Traceability works exactly as in direct mode.
    let links =
        w.h.engine
            .trace(&w.operator, w.scope, Some(&format!("{BRD}#5")), 3)
            .await
            .unwrap();
    assert!(
        links
            .iter()
            .any(|l| l.relation == "realised_by" && l.from_id == "REQ-F-006" && l.to_id == "op-place-order")
    );

    // ---- No EAEH-managed model inference happened.
    assert!(
        w.provider.requests().is_empty(),
        "the offered provider was never called"
    );
    assert_eq!(model_usage_rows(&w).await, 0);
    let audit = |action: &'static str| {
        let db = w.h.db.clone();
        async move {
            eaeh_store::sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM audit_log WHERE action = ?")
                .bind(action)
                .fetch_one(db.pool())
                .await
                .unwrap()
                .0
        }
    };
    assert_eq!(audit("external_task.handoff").await, 4);
    assert_eq!(audit("external_task.submit").await, 5);
    assert_eq!(audit("external_task.accept").await, 4);
    assert_eq!(audit("external_task.reject").await, 1);
    assert_eq!(audit("model.invoke").await, 0);
    assert!(w.h.db.verify_audit_chain().await.unwrap().valid);
}

#[tokio::test]
async fn external_tasks_are_project_bound_and_correction_bounded() {
    let w = world_in(ExecutionMode::ExternalAi, vec![], "").await;
    let id =
        w.h.engine
            .start(
                &w.operator,
                &WorkflowDefinitionId::new("requirements.analysis").unwrap(),
                w.scope,
                json!({"brd": BRD}),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    assert_eq!(
        w.h.engine.drive(id).await.unwrap(),
        InstanceStatus::WaitingExternal
    );
    let (t, _) = open_task(&w).await;

    // An operator of another project (same tenant) cannot see or act on it: "not found".
    let other =
        w.h.identity
            .create_project(&w.admin, w.scope.tenant, "billing")
            .await
            .unwrap();
    let outsider =
        w.h.identity
            .create_principal(
                &w.admin,
                NewPrincipal {
                    id: PrincipalId::new("oscar").unwrap(),
                    display_name: "oscar".into(),
                    kind: PrincipalKind::Human,
                    roles: [Role::Operator].into(),
                    projects: ProjectAccess::Only([other].into()),
                },
            )
            .await
            .unwrap();
    for e in [
        w.h.agents.external_task(&outsider, t).await.unwrap_err(),
        w.h.agents
            .external_tool(&outsider, t, "knowledge.read", json!({"document": BRD}))
            .await
            .unwrap_err(),
        w.h.submit_external(&outsider, t, &json!({}), CLIENT)
            .await
            .unwrap_err(),
    ] {
        assert_eq!(e.kind, eaeh_core::ErrorKind::NotFound, "{e}");
    }
    assert!(w.h.agents.external_tasks(&outsider, w.scope, true).await.is_err());
    assert!(
        w.h.agents
            .external_tasks(&outsider, Scope::new(w.scope.tenant, other), true)
            .await
            .unwrap()
            .is_empty()
    );
    // Malformed client labels and non-object outputs are refused before anything is stored.
    assert!(
        w.h.submit_external(&w.operator, t, &json!({}), "bad client!")
            .await
            .is_err()
    );
    assert!(
        w.h.submit_external(&w.operator, t, &json!([1]), CLIENT)
            .await
            .is_err()
    );

    // The business analyst allows 3 correction attempts: the 4th invalid result fails the run.
    let bad = json!({"document": "requirements", "title": "x", "requirements": [], "citations": []});
    for _ in 0..3 {
        let (ok, st, _) = submit(&w, t, &bad).await;
        assert!(!ok);
        assert_eq!(st, InstanceStatus::WaitingExternal);
    }
    let (ok, st, fb) = submit(&w, t, &bad).await;
    assert!(!ok);
    assert!(
        matches!(st, InstanceStatus::Failed | InstanceStatus::Escalated),
        "{st:?}"
    );
    assert_eq!(fb.len(), 4);
    // A finished task accepts nothing more.
    assert!(w.h.submit_external(&w.operator, t, &bad, CLIENT).await.is_err());
    assert_eq!(model_usage_rows(&w).await, 0);
}

#[tokio::test]
async fn a_direct_provider_runtime_never_completes_a_handed_off_task() {
    let w = world_in(ExecutionMode::ExternalAi, vec![], "").await;
    let id =
        w.h.engine
            .start(
                &w.operator,
                &WorkflowDefinitionId::new("requirements.analysis").unwrap(),
                w.scope,
                json!({"brd": BRD}),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    assert_eq!(
        w.h.engine.drive(id).await.unwrap(),
        InstanceStatus::WaitingExternal
    );
    // Same database, reopened by a runtime in direct_provider mode with a willing provider.
    let mut cfg = w.h.config.clone();
    cfg.execution.mode = ExecutionMode::DirectProvider;
    let provider = std::sync::Arc::new(eaeh_testkit::ScriptedModelProvider::new(
        "scripted",
        "scripted-model",
        vec![Script::json(example("requirements.json"))],
    ));
    let direct = eaeh_runtime::Harness::builder(cfg)
        .model_provider(provider.clone())
        .build()
        .await
        .unwrap();
    let st = direct.engine.drive(id).await.unwrap();
    assert_eq!(st, InstanceStatus::Failed);
    let failure = direct.engine.get(id).await.unwrap().data.failure.unwrap();
    assert!(failure.contains("no fallback"), "{failure}");
    assert!(provider.requests().is_empty(), "the provider was never called");
}

#[tokio::test]
async fn deterministic_mode_registers_only_the_replay_provider() {
    let mut cfg = eaeh_runtime::HarnessConfig::load(&root().join("config/eaeh.toml")).unwrap();
    cfg.storage.data_dir =
        std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("det-{}", uuid::Uuid::now_v7()));
    cfg.isolation.container_enabled = false;
    cfg.execution.mode = ExecutionMode::Deterministic;
    // Deterministic mode without a cassette is a configuration error, not a silent no-op.
    cfg.providers.replay.enabled = false;
    assert!(eaeh_runtime::Harness::builder(cfg.clone()).build().await.is_err());
    cfg.providers.replay.enabled = true;
    cfg.providers.replay.cassette = root().join("examples/simple-springboot-api/replay-cassette.json");
    cfg.providers.anthropic.enabled = true; // would need a key in direct mode; ignored here
    let offered = std::sync::Arc::new(eaeh_testkit::ScriptedModelProvider::new("scripted", "m", vec![]));
    let h = eaeh_runtime::Harness::builder(cfg)
        .model_provider(offered)
        .build()
        .await
        .unwrap();
    assert_eq!(h.model_provider_ids, vec!["replay"]);
    assert_eq!(h.ignored_providers, vec!["scripted", "anthropic"]);
}

// ------------------------------------------------------------------ software delivery

fn delivery_input(build: Value) -> Value {
    json!({
        "design_run": "test",
        "requirements": example("requirements.json"),
        "hld": example("hld.json"),
        "lld": example("lld.json"),
        "project": {"name": "orders", "language": "Java 21", "framework": "Spring Boot 3.x", "build_tool": "Maven", "constraints": []},
        "build": build,
    })
}

/// One task per requirement; `skip` leaves one out (to exercise the coverage gate).
fn plan(skip: Option<&str>) -> Value {
    let reqs = example("requirements.json");
    let tasks: Vec<Value> = reqs["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| Some(r["id"].as_str().unwrap()) != skip)
        .enumerate()
        .map(|(i, r)| {
            json!({"id": format!("T-{:03}", i + 1), "title": format!("Implement {}", r["id"].as_str().unwrap()),
                   "requirements": [r["id"]], "files": ["src/main/java/orders/OrderService.java"],
                   "tests": ["src/test/java/orders/OrderServiceTest.java"]})
        })
        .collect();
    json!({"document": "implementation_plan", "title": "Orders implementation plan", "tasks": tasks, "citations": []})
}

fn implementation() -> Value {
    let ids: Vec<Value> = example("requirements.json")["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    json!({"document": "implementation", "summary": "Order service with tests.",
           "files": [{"path": "src/main/java/orders/OrderService.java", "requirements": ids}],
           "tests": [{"path": "src/test/java/orders/OrderServiceTest.java", "kind": "unit", "requirements": ids}],
           "citations": []})
}

/// Drive a delivery workflow through plan (with one rejected draft), plan approval and the
/// implementation hand-off; returns the instance and the status after implementing.
async fn deliver(w: &World, workflow: &str, build: Value) -> (eaeh_core::WorkflowInstanceId, InstanceStatus) {
    let id =
        w.h.engine
            .start(
                &w.operator,
                &WorkflowDefinitionId::new(workflow).unwrap(),
                w.scope,
                delivery_input(build),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    assert_eq!(
        w.h.engine.drive(id).await.unwrap(),
        InstanceStatus::WaitingExternal
    );
    let (t, pkg) = open_task(w).await;
    assert_eq!(pkg["agent"]["reference"], "delivery-planner@1.0.0");
    let (ok, _, fb) = submit(w, t, &plan(Some("REQ-F-001"))).await;
    assert!(!ok);
    assert!(fb[0].contains("REQ-F-001 is not covered"), "{fb:?}");
    let (ok, st, _) = submit(w, t, &plan(None)).await;
    assert!(ok);
    assert_eq!(st, InstanceStatus::WaitingApproval);
    let (st, gate) = decide(w, ApprovalDecision::Approve, "plan approved").await;
    assert_eq!(
        (st, gate.as_str()),
        (InstanceStatus::WaitingExternal, "implement")
    );

    let (t, pkg) = open_task(w).await;
    assert_eq!(pkg["agent"]["reference"], "software-engineer@1.0.0");
    assert_eq!(pkg["agent"]["autonomy"], "workspace_modify");
    // The workspace boundary and the agent's allow-list bind the client.
    let e =
        w.h.agents
            .external_tool(
                &w.operator,
                t,
                "fs.write",
                json!({"path": "../escape.txt", "content": "x"}),
            )
            .await
            .unwrap_err();
    assert!(!e.message.is_empty());
    assert!(
        w.h.agents
            .external_tool(&w.operator, t, "exec.run", json!({"program": "/bin/echo"}))
            .await
            .is_err()
    );
    // Listing a file that was never written is rejected at submission (with feedback).
    let (ok, _, fb) = submit(w, t, &implementation()).await;
    assert!(!ok);
    assert!(
        fb.last().unwrap().contains("do not exist in the task workspace"),
        "{fb:?}"
    );
    for (p, c) in [
        (
            "src/main/java/orders/OrderService.java",
            "package orders;\npublic class OrderService {}\n",
        ),
        (
            "src/test/java/orders/OrderServiceTest.java",
            "package orders;\nclass OrderServiceTest {}\n",
        ),
    ] {
        w.h.agents
            .external_tool(&w.operator, t, "fs.write", json!({"path": p, "content": c}))
            .await
            .unwrap();
    }
    let (ok, st, fb) = submit(w, t, &implementation()).await;
    assert!(ok, "{fb:?}");
    (id, st)
}

#[tokio::test]
async fn delivery_with_deferred_build_produces_an_approved_traceable_source_bundle() {
    let w = world_in(ExecutionMode::ExternalAi, vec![], "").await;
    let (id, st) = deliver(
        &w,
        "engineering.delivery-unverified",
        json!({"program": "", "args": []}),
    )
    .await;
    assert_eq!(st, InstanceStatus::WaitingApproval);
    let inst = w.h.engine.get(id).await.unwrap();
    assert_eq!(inst.state, "approve_delivery");
    let src = read_bundle_file(&w, &inst, "snapshot", "src/main/java/orders/OrderService.java").await;
    assert!(src.contains("class OrderService"));
    let summary = read_bundle_file(&w, &inst, "snapshot", "EAEH-DELIVERY.md").await;
    assert!(summary.contains("NOT EXECUTED BY EAEH"), "{summary}");
    let (st, _) = decide(&w, ApprovalDecision::Approve, "delivery approved").await;
    assert_eq!(st, InstanceStatus::Succeeded);
    let links =
        w.h.engine
            .trace(&w.operator, w.scope, Some("REQ-F-001"), 1)
            .await
            .unwrap();
    for (rel, to) in [
        ("implemented_by", "src/main/java/orders/OrderService.java"),
        ("verified_by", "src/test/java/orders/OrderServiceTest.java"),
    ] {
        assert!(
            links.iter().any(|l| l.relation == rel && l.to_id == to),
            "{links:#?}"
        );
    }
    assert_eq!(model_usage_rows(&w).await, 0);
}

#[tokio::test]
async fn delivery_with_required_build_never_skips_the_build_step() {
    // No container isolation in tests: the build step must fail the run, not be skipped.
    let w = world_in(ExecutionMode::ExternalAi, vec![], "").await;
    let (id, st) = deliver(
        &w,
        "engineering.delivery",
        json!({"program": "/bin/echo", "args": ["build"]}),
    )
    .await;
    assert_eq!(st, InstanceStatus::Failed);
    let inst = w.h.engine.get(id).await.unwrap();
    let failure = inst.data.failure.unwrap();
    assert!(failure.starts_with("build_test:"), "{failure}");
    assert!(!inst.data.current_approval.contains_key("approve_delivery"));
    assert!(
        w.h.approvals
            .list(w.scope, None, None, 50)
            .await
            .unwrap()
            .iter()
            .all(|a| a.gate.as_deref() != Some("approve_delivery"))
    );
}

/// The HLD failure of 2026-11-13: the client must be told the exact structure and must be
/// able to check a draft without spending its few corrections.
#[tokio::test]
async fn hld_hand_off_states_the_exact_contract_and_drafts_can_be_checked_for_free() {
    let w = world_in(ExecutionMode::ExternalAi, vec![], "").await;
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
    w.h.engine.drive(id).await.unwrap();
    for _ in 0..2 {
        let (t, _) = open_task(&w).await;
        read_brd(&w, t).await;
        assert!(submit(&w, t, &example("requirements.json")).await.0);
    }
    decide(&w, ApprovalDecision::Approve, "requirements approved").await;
    let (t, pkg) = open_task(&w).await;
    assert_eq!(pkg["agent"]["reference"], "solution-architect@1.0.0");

    // 1. The package states the contract explicitly, generated from the enforced schemas.
    let schema = eaeh_validation::schema::CompiledSchema::compile(&pkg["output_schema"]).unwrap();
    let ex = &pkg["output_example"];
    assert!(
        schema.errors(ex).is_empty(),
        "output_example is valid: {:?}",
        schema.errors(ex)
    );
    for (r, s) in pkg["skill_output_schemas"].as_object().unwrap() {
        let c = eaeh_validation::schema::CompiledSchema::compile(s).unwrap();
        assert!(c.errors(ex).is_empty(), "example meets skill contract {r}");
    }
    assert!(ex["context"].is_object());
    let outline: Vec<&str> = pkg["output_outline"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        outline
            .iter()
            .any(|l| l.starts_with("context: object (required); required fields: system_name, description"))
    );
    assert!(outline.iter().any(|l| l.starts_with(
        "decisions[]: object (required); required fields: id, title, status, context, decision, alternatives"
    )));
    let rules = pkg["rules"].to_string();
    assert!(rules.contains("eaeh_task_validate") && rules.contains("no other field is added"));

    // 2. The failed shape: every problem at once, and no correction attempt used.
    let mut bad = example("hld.json");
    bad["context"] = json!("A local-only demonstration REST API.");
    bad["decisions"][0] =
        json!({"id": "AD-001", "title": "t", "decision": "d", "rationale": "r", "citations": []});
    let r = w.h.agents.external_validate(&w.operator, t, &bad).await.unwrap();
    assert_eq!(r["valid"], false);
    let problems = r["problems"].to_string();
    for p in [
        "/context",
        "is not of type",
        "\\\"status\\\" is a required property",
        "\\\"alternatives\\\" is a required property",
        "'citations', 'rationale' were unexpected",
        "all shown",
    ] {
        assert!(problems.contains(p), "{p} missing in {problems}");
    }
    let view = w.h.agents.external_task(&w.operator, t).await.unwrap();
    assert_eq!(
        (view.status.as_str(), view.submissions),
        ("awaiting", 0),
        "validation is free"
    );
    // Approvers cannot use it (same rule as reading the package).
    assert!(w.h.agents.external_validate(&w.approver, t, &bad).await.is_err());

    // 3. A real submission of the bad shape gets the same complete feedback.
    let (ok, _, fb) = submit(&w, t, &bad).await;
    assert!(!ok);
    assert!(
        fb.last().unwrap().contains("all shown") && fb.last().unwrap().contains("/decisions/0"),
        "{fb:?}"
    );

    // 4. The correct document passes the free check (including the design rules) and is accepted.
    let good = example("hld.json");
    let r = w.h.agents.external_validate(&w.operator, t, &good).await.unwrap();
    assert_eq!(r["valid"], true, "{r}");
    assert!(r["checked"].to_string().contains("design_rules"));
    let (ok, st, _) = submit(&w, t, &good).await;
    assert!(ok);
    assert_eq!(st, InstanceStatus::WaitingApproval);
}
