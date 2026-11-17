//! Derived runs: restart, retry and reinitiation (PORTAL-OPERATIONS-PLAN, W1).
//!
//! `engineering.design` on the shipped configuration; the model is the labelled test double
//! `ScriptedModelProvider`. Approvals are decided by the fixture's human approver.

mod common;
use common::*;
use eaeh_core::{TraceContext, WorkflowDefinitionId, WorkflowInstanceId};
use eaeh_governance::ApprovalDecision;
use eaeh_testkit::Script;
use eaeh_workflow::InstanceStatus;
use serde_json::json;

fn design_script(extra_hld_lld: usize) -> Vec<Script> {
    let good = example("requirements.json");
    let read_brd = || Script::tool("knowledge.read", json!({"document": BRD}));
    let mut v = vec![
        read_brd(),
        Script::json(good.clone()),
        read_brd(),
        Script::json(good),
        Script::json(example("hld.json")),
        Script::json(example("lld.json")),
    ];
    for _ in 0..extra_hld_lld {
        v.push(Script::json(example("hld.json")));
        v.push(Script::json(example("lld.json")));
    }
    v
}

async fn start(w: &World) -> WorkflowInstanceId {
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
        InstanceStatus::WaitingApproval
    );
    id
}

async fn count(w: &World, sql: &'static str, id: &str) -> i64 {
    let (n,): (i64,) = eaeh_store::sqlx::query_as(sql)
        .bind(id)
        .fetch_one(w.h.db.pool())
        .await
        .unwrap();
    n
}

/// Restart from the HLD stage after an architecture change: earlier stages and the valid
/// requirements approval are carried, HLD/LLD are regenerated as new versions and need new
/// approvals, the source run is superseded but untouched, and the new run completes with
/// the carried approval verified through lineage.
#[tokio::test]
async fn restart_from_hld_carries_upstream_regenerates_downstream_and_never_overwrites() {
    let w = world(design_script(1), "").await;
    let src = start(&w).await;
    approve_gate(&w).await; // requirements → HLD
    approve_gate(&w).await; // HLD → LLD
    let s = w.h.engine.get(src).await.unwrap();
    assert_eq!(s.state, "approve_lld");
    let old_hld = s.data.artifacts["render_hld"].clone();
    let req_approval = s.data.approved_gates["approve_requirements"];

    // Preview: what is carried, re-run, repeated.
    let plan =
        w.h.engine
            .restart_plan(&w.operator, src, Some("hld"), None)
            .await
            .unwrap();
    assert_eq!(
        plan.carried_states,
        vec![
            "brd",
            "analyse",
            "refine",
            "render_requirements",
            "approve_requirements"
        ]
    );
    assert_eq!(plan.carried_approvals, vec!["approve_requirements"]);
    assert_eq!(plan.repeat_approvals, vec!["approve_hld"]);
    let st = |n: &str| plan.stages.iter().find(|s| s.name == n).unwrap().clone();
    assert_eq!(st("hld").disposition, "restart_point");
    assert_eq!(st("render_hld").disposition, "rerun");
    assert_eq!(st("approve_hld").approval.as_deref(), Some("repeat"));
    assert_eq!(st("lld").disposition, "rerun");
    assert_eq!(st("approve_lld").approval.as_deref(), Some("required"));
    assert!(plan.source_will_be_cancelled);
    assert!(
        plan.allowed_stages.contains(&"approve_lld".to_string()),
        "{:?}",
        plan.allowed_stages
    );

    // A stale fingerprint is refused; the right one creates the derived run (idempotently).
    let e =
        w.h.engine
            .restart(
                &w.operator,
                src,
                "hld",
                None,
                "stale",
                "architecture changed",
                TraceContext::new_root(),
            )
            .await
            .unwrap_err();
    assert_eq!(e.kind, eaeh_core::ErrorKind::Conflict);
    let (new, _) =
        w.h.engine
            .restart(
                &w.operator,
                src,
                "hld",
                None,
                &plan.fingerprint,
                "architecture changed",
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    let (again, _) =
        w.h.engine
            .restart(
                &w.operator,
                src,
                "hld",
                None,
                &plan.fingerprint,
                "architecture changed",
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    assert_eq!(new, again, "a repeated confirmation does not create a second run");
    // Source superseded (cancelled), its data and approvals untouched.
    let s2 = w.h.engine.get(src).await.unwrap();
    assert_eq!(s2.status, InstanceStatus::Cancelled);
    assert_eq!(s2.data.artifacts["render_hld"], old_hld);
    assert_eq!(s2.data.approved_gates, s.data.approved_gates);

    // The new run starts at HLD with the carried data and the requirements approval.
    let n = w.h.engine.get(new).await.unwrap();
    assert_eq!(n.state, "hld");
    assert_eq!(n.data.restarted_from.as_ref().unwrap().source, src);
    assert_eq!(
        n.data.artifacts["render_requirements"],
        s.data.artifacts["render_requirements"]
    );
    assert!(!n.data.artifacts.contains_key("render_hld"));
    assert_eq!(
        n.data.carried_approvals["approve_requirements"].origin_instance,
        src
    );
    assert_eq!(n.data.approved_gates["approve_requirements"], req_approval);
    assert_eq!(
        w.h.engine.drive(new).await.unwrap(),
        InstanceStatus::WaitingApproval
    );
    let n = w.h.engine.get(new).await.unwrap();
    assert_eq!(n.state, "approve_hld");
    let new_hld = n.data.artifacts["render_hld"].clone();
    assert!(
        new_hld.version > old_hld.version,
        "a new version, the approved one is kept"
    );
    assert_ne!(new_hld.id, old_hld.id);
    // The old approved HLD is still readable and unchanged.
    let (_, bytes) = w.h.artifacts.read(w.scope, old_hld.id).await.unwrap();
    assert_eq!(eaeh_core::hash::sha256_hex(&bytes), old_hld.hash);
    // The HLD gate needs a new decision; then LLD; then the run completes, the carried
    // requirements approval verified through lineage by the completion guard.
    approve_gate(&w).await;
    approve_gate(&w).await;
    let n = w.h.engine.get(new).await.unwrap();
    assert_eq!(n.status, InstanceStatus::Succeeded, "{:?}", n.data.failure);
    let checks = w.h.engine.gate_checks(&w.operator, new).await.unwrap();
    assert!(checks.iter().all(|c| c.issues.is_empty()), "{checks:?}");
    // Lineage and audit.
    let l = w.h.engine.lineage(&w.operator, src).await.unwrap();
    assert_eq!(l["derived_runs"][0]["instance"], json!(new.to_string()));
    assert_eq!(w.h.engine.ancestors(new).await.unwrap(), vec![src]);
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'workflow.restart' AND resource_id = ?",
            &new.to_string()
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'workflow.superseded' AND resource_id = ?",
            &src.to_string()
        )
        .await,
        1
    );
}

/// Mandatory gates cannot be skipped, unknown stages cannot be injected, and changed input
/// restarts from the first stage only.
#[tokio::test]
async fn restart_points_never_skip_approvals_or_leave_the_definition() {
    let w = world(design_script(0), "").await;
    let src = start(&w).await;
    approve_gate(&w).await; // requirements approved; HLD now waiting (not approved)
    let plan =
        w.h.engine
            .restart_plan(&w.operator, src, None, None)
            .await
            .unwrap();
    assert!(
        !plan.allowed_stages.contains(&"lld".to_string()),
        "{:?}",
        plan.allowed_stages
    );
    assert_eq!(
        plan.allowed_stages.last().map(String::as_str),
        Some("approve_hld")
    );
    for (stage, kind) in [
        ("lld", eaeh_core::ErrorKind::Conflict),
        ("approve_lld", eaeh_core::ErrorKind::Conflict),
        ("deploy", eaeh_core::ErrorKind::InvalidInput),
        ("failed", eaeh_core::ErrorKind::InvalidInput),
    ] {
        let e =
            w.h.engine
                .restart_plan(&w.operator, src, Some(stage), None)
                .await
                .unwrap_err();
        assert_eq!(e.kind, kind, "{stage}: {}", e.message);
    }
    let e =
        w.h.engine
            .restart_plan(&w.operator, src, Some("lld"), None)
            .await
            .unwrap_err();
    assert!(e.message.contains("approve_hld"), "{}", e.message);
    // New input (e.g. a new BRD version) only from the first stage.
    let new_input = json!({"brd": "demo-brd/order-management@v2"});
    let e =
        w.h.engine
            .restart_plan(&w.operator, src, Some("hld"), Some(&new_input))
            .await
            .unwrap_err();
    assert!(e.message.contains("first stage"), "{}", e.message);
    let p =
        w.h.engine
            .restart_plan(&w.operator, src, Some("brd"), Some(&new_input))
            .await
            .unwrap();
    assert!(p.input_changed && p.carried_states.is_empty() && p.carried_approvals.is_empty());
    assert_eq!(p.repeat_approvals, vec!["approve_requirements"]);
    // Authorization: the approver cannot restart (workflow.start).
    let e =
        w.h.engine
            .restart_plan(&w.approver, src, None, None)
            .await
            .unwrap_err();
    assert_eq!(e.kind, eaeh_core::ErrorKind::Forbidden);
}

/// Retry a failed stage: the failed run stays as it is; a derived run re-executes the stage.
#[tokio::test]
async fn a_failed_stage_is_retried_in_a_derived_run() {
    // No model responses: the analysis stage fails.
    let w = world(vec![], "").await;
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
    assert_eq!(w.h.engine.drive(id).await.unwrap(), InstanceStatus::Failed);
    let plan =
        w.h.engine
            .restart_plan(&w.operator, id, None, None)
            .await
            .unwrap();
    assert_eq!(plan.stage, "analyse", "default restart point is the failed stage");
    assert_eq!(plan.carried_states, vec!["brd"]);
    assert!(!plan.source_will_be_cancelled);
    let (new, _) =
        w.h.engine
            .restart(
                &w.operator,
                id,
                "analyse",
                None,
                &plan.fingerprint,
                "retry",
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    assert_eq!(
        w.h.engine.get(id).await.unwrap().status,
        InstanceStatus::Failed,
        "history preserved"
    );
    w.h.engine.drive(new).await.unwrap();
    let tasks = w.h.engine.tasks_for(&w.operator, new).await.unwrap();
    assert!(
        tasks.iter().any(|t| t.state == "analyse"),
        "the stage was executed again: {tasks:?}"
    );
    assert!(
        !tasks.iter().any(|t| t.state == "brd"),
        "carried stages are not executed again"
    );
}

/// A carried approval is honoured only from an ancestor run.
#[tokio::test]
async fn a_carried_approval_from_an_unrelated_run_is_rejected() {
    let w = world(design_script(1), "").await;
    let src = start(&w).await;
    approve_gate(&w).await;
    let plan =
        w.h.engine
            .restart_plan(&w.operator, src, Some("hld"), None)
            .await
            .unwrap();
    let (new, _) =
        w.h.engine
            .restart(
                &w.operator,
                src,
                "hld",
                None,
                &plan.fingerprint,
                "x",
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    // Tamper: point the carried approval at an unrelated run.
    let other = eaeh_core::WorkflowInstanceId::new();
    eaeh_store::sqlx::query("UPDATE workflow_instances SET data = json_set(data, '$.carried_approvals.approve_requirements.origin_instance', ?) WHERE id = ?")
        .bind(other.to_string())
        .bind(new.to_string())
        .execute(w.h.db.pool())
        .await
        .unwrap();
    let checks = w.h.engine.gate_checks(&w.operator, new).await.unwrap();
    let c = checks.iter().find(|c| c.gate == "approve_requirements").unwrap();
    assert!(
        c.issues.iter().any(|i| i.contains("not from an ancestor")),
        "{c:?}"
    );
}

async fn approve_gate(w: &World) {
    decide(w, ApprovalDecision::Approve, "ok").await;
}
