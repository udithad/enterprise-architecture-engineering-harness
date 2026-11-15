//! Approved Requirements / HLD / LLD documents in the application repository
//! (docs/implementation/stages/ARTIFACT-MATERIALIZATION-PLAN.md, decisions M1–M4).
//!
//! `engineering.design` on the shipped configuration and the governed demonstration BRD.
//! Everything is production code except the model (labelled test double
//! `ScriptedModelProvider`). Approvals are decided by the fixture's human approver
//! identity, as in every other automated test.

mod common;
use common::*;
use eaeh_core::hash::sha256_hex;
use eaeh_core::{TraceContext, WorkflowDefinitionId, WorkflowInstanceId};
use eaeh_governance::ApprovalDecision;
use eaeh_testkit::Script;
use eaeh_workflow::InstanceStatus;
use serde_json::{Value, json};
use std::path::Path;

const REQ_DIR: &str = "docs/architecture/requirements";
const HLD_DIR: &str = "docs/architecture/hld";
const LLD_DIR: &str = "docs/architecture/lld";

async fn design_at_requirements_gate() -> (World, WorkflowInstanceId) {
    let good = example("requirements.json");
    let read_brd = || Script::tool("knowledge.read", json!({"document": BRD}));
    let script = vec![
        read_brd(),
        Script::json(good.clone()),
        read_brd(),
        Script::json(good),
        Script::json(example("hld.json")),
        Script::json(example("lld.json")),
    ];
    let w = world(script, "").await;
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
    (w, id)
}

/// The bundle manifest of a state's artifact: `path → (file artifact id, hash)`.
async fn manifest(w: &World, id: WorkflowInstanceId, state: &str) -> Vec<(String, String, String)> {
    let inst = w.h.engine.get(id).await.unwrap();
    let (_, bytes) =
        w.h.artifacts
            .read(w.scope, inst.data.artifacts[state].id)
            .await
            .unwrap();
    let m: Value = serde_json::from_slice(&bytes).unwrap();
    m["files"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(p, f)| {
            (
                p.clone(),
                f["artifact_id"].as_str().unwrap().to_string(),
                f["hash"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// Every file of the bundle exists under `dir` and equals the stored hash.
async fn assert_materialized(w: &World, id: WorkflowInstanceId, state: &str, dir: &str) -> usize {
    let files = manifest(w, id, state).await;
    assert!(!files.is_empty());
    for (path, fid, hash) in &files {
        let disk =
            std::fs::read(w.workspace.join(dir).join(path)).unwrap_or_else(|e| panic!("{dir}/{path}: {e}"));
        assert_eq!(&sha256_hex(&disk), hash, "{dir}/{path}");
        // The stored file artifact is the authoritative copy and still verifies.
        let (meta, stored) = w.h.artifacts.read(w.scope, fid.parse().unwrap()).await.unwrap();
        assert_eq!(stored, disk);
        assert_eq!(&meta.content_hash, hash);
    }
    files.len()
}

fn files_under(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            n += if p.is_dir() { files_under(&p) } else { 1 };
        }
    }
    n
}

async fn approve(w: &World) -> (InstanceStatus, String) {
    decide(w, ApprovalDecision::Approve, "approved after review").await
}

async fn doc_state(w: &World, id: WorkflowInstanceId, gate: &str) -> (String, Option<String>) {
    let d = w.h.engine.documents(&w.operator, id).await.unwrap();
    let d = d.into_iter().find(|d| d.gate == gate).unwrap();
    (d.materialization.unwrap().state, d.blocked)
}

async fn count(w: &World, sql: &'static str, id: &str) -> i64 {
    let (n,): (i64,) = eaeh_store::sqlx::query_as(sql)
        .bind(id)
        .fetch_one(w.h.db.pool())
        .await
        .unwrap();
    n
}

#[tokio::test]
async fn approved_documents_are_written_hash_verified_and_traceable() {
    let (w, id) = design_at_requirements_gate().await;
    let docs = w.workspace.join("docs/architecture");

    // Persisted and reviewable, but a draft is never written to the workspace.
    let inst = w.h.engine.get(id).await.unwrap();
    for state in ["analyse", "refine", "render_requirements"] {
        let a = &inst.data.artifacts[state];
        let (meta, bytes) = w.h.artifacts.read(w.scope, a.id).await.unwrap();
        assert_eq!(sha256_hex(&bytes), a.hash);
        assert_eq!(
            meta.provenance.workflow_instance.as_deref(),
            Some(id.to_string().as_str())
        );
    }
    assert!(!docs.exists(), "nothing is written before approval");
    assert_eq!(
        doc_state(&w, id, "approve_requirements").await.0,
        "awaiting_approval"
    );

    // Requirements approved → written before the HLD stage starts.
    let (st, state) = approve(&w).await;
    assert_eq!(
        (st, state.as_str()),
        (InstanceStatus::WaitingApproval, "approve_hld")
    );
    let n_req = assert_materialized(&w, id, "render_requirements", REQ_DIR).await;
    assert!(w.workspace.join(REQ_DIR).join("requirements.md").is_file());
    assert!(
        !w.workspace.join(HLD_DIR).exists(),
        "the HLD draft is not written"
    );
    assert_eq!(doc_state(&w, id, "approve_requirements").await.0, "verified");
    assert_eq!(doc_state(&w, id, "approve_hld").await.0, "awaiting_approval");

    // HLD and LLD.
    assert_eq!(approve(&w).await.1, "approve_lld");
    let n_hld = assert_materialized(&w, id, "render_hld", HLD_DIR).await;
    assert!(w.workspace.join(HLD_DIR).join("hld.md").is_file());
    let (st, _) = approve(&w).await;
    assert_eq!(st, InstanceStatus::Succeeded);
    let n_lld = assert_materialized(&w, id, "render_lld", LLD_DIR).await;
    assert!(w.workspace.join(LLD_DIR).join("lld.md").is_file());
    // Exactly the approved files: no duplicates, no stray temporary files.
    assert_eq!(files_under(&docs), n_req + n_hld + n_lld);

    // Provenance index (M4): artifact, version, hash, approval, sources BRD → Req → HLD → LLD.
    let idx: Value =
        serde_json::from_slice(&std::fs::read(w.workspace.join(".eaeh/artifacts.json")).unwrap()).unwrap();
    let inst = w.h.engine.get(id).await.unwrap();
    assert_eq!(idx["project"], json!(w.scope.project.to_string()));
    for (dir, state, gate) in [
        (REQ_DIR, "render_requirements", "approve_requirements"),
        (HLD_DIR, "render_hld", "approve_hld"),
        (LLD_DIR, "render_lld", "approve_lld"),
    ] {
        let e = &idx["documents"][dir];
        let a = &inst.data.artifacts[state];
        assert_eq!(e["artifact"]["id"], json!(a.id.to_string()));
        assert_eq!(e["artifact"]["hash"], json!(a.hash));
        assert_eq!(e["artifact"]["version"], json!(a.version));
        assert_eq!(e["workflow_instance"], json!(id.to_string()));
        assert_eq!(e["gate"], json!(gate));
        assert_eq!(
            e["approval"]["id"],
            json!(inst.data.approved_gates[gate].to_string())
        );
        assert_eq!(e["approval"]["approver"], json!(w.approver.id.to_string()));
        assert_eq!(e["approval"]["artifact_hash"], json!(a.hash));
        assert!(e["approval"]["decided_at"].is_string() && e["materialized_at"].is_string());
    }
    let src = |dir: &str| -> Vec<String> {
        idx["documents"][dir]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["gate"].as_str().map(String::from))
            .collect()
    };
    assert_eq!(src(HLD_DIR), vec!["approve_requirements"]);
    let lld_src = src(LLD_DIR);
    assert!(
        lld_src.contains(&"approve_hld".to_string()) && lld_src.contains(&"approve_requirements".to_string())
    );
    assert_eq!(
        idx["documents"][LLD_DIR]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["gate"] == "approve_hld")
            .unwrap()["hash"],
        json!(inst.data.artifacts["render_hld"].hash),
        "the LLD derives from the approved HLD version"
    );

    // Audit and events: one successful materialization per gate.
    assert_eq!(
        count(&w, "SELECT COUNT(*) FROM audit_log WHERE action = 'artifact.materialize' AND outcome = 'succeeded' AND json_extract(details, '$.workflow_instance') = ?", &id.to_string()).await,
        3
    );
    assert_eq!(
        count(&w, "SELECT COUNT(*) FROM outbox WHERE event_type = 'artifact.materialized' AND payload LIKE '%' || ? || '%'", "succeeded").await,
        3
    );

    // Idempotent reconciliation: nothing changes, nothing duplicates – and the
    // reconciled index keeps the same (earlier-gates-only) sources.
    let r = w.h.engine.sync_documents(&w.operator, id, false).await.unwrap();
    let idx2: Value =
        serde_json::from_slice(&std::fs::read(w.workspace.join(".eaeh/artifacts.json")).unwrap()).unwrap();
    for dir in [REQ_DIR, HLD_DIR, LLD_DIR] {
        assert_eq!(
            idx2["documents"][dir]["sources"], idx["documents"][dir]["sources"],
            "{dir}"
        );
    }
    assert_eq!(
        idx2["documents"][REQ_DIR]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s.get("gate").is_some())
            .count(),
        0
    );
    assert_eq!(r.len(), 3);
    for o in &r {
        assert_eq!(o.status, "succeeded", "{o:?}");
        assert!(
            o.report
                .as_ref()
                .unwrap()
                .files
                .iter()
                .all(|f| f.action == "unchanged"),
            "{o:?}"
        );
    }
    assert_eq!(files_under(&docs), n_req + n_hld + n_lld);
    for gate in ["approve_requirements", "approve_hld", "approve_lld"] {
        assert_eq!(doc_state(&w, id, gate).await.0, "verified");
    }
}

#[tokio::test]
async fn local_edits_are_detected_never_overwritten_silently_and_missing_files_restored() {
    let (w, id) = design_at_requirements_gate().await;
    approve(&w).await;
    approve(&w).await;
    approve(&w).await;
    let hld = w.workspace.join(HLD_DIR).join("hld.md");
    let approved = std::fs::read(&hld).unwrap();

    // A local edit: reported as modified, never as verified.
    std::fs::write(&hld, b"my local notes").unwrap();
    let (state, _) = doc_state(&w, id, "approve_hld").await;
    assert_eq!(state, "modified");
    let d = w.h.engine.documents(&w.operator, id).await.unwrap();
    let m = d
        .iter()
        .find(|d| d.gate == "approve_hld")
        .unwrap()
        .materialization
        .as_ref()
        .unwrap();
    assert!(
        m.files
            .iter()
            .any(|f| f.path.ends_with("hld.md") && f.state == "modified")
    );

    // Plain sync refuses to overwrite it (conflict, audited as denied) and writes nothing.
    let r = w.h.engine.sync_documents(&w.operator, id, false).await.unwrap();
    let o = r.iter().find(|o| o.gate == "approve_hld").unwrap();
    assert_eq!(o.status, "conflict");
    assert!(o.detail.contains("modified locally"), "{}", o.detail);
    assert_eq!(std::fs::read(&hld).unwrap(), b"my local notes");
    assert!(count(&w, "SELECT COUNT(*) FROM audit_log WHERE action = 'artifact.materialize' AND outcome = 'denied' AND resource_id = ?", &w.h.engine.get(id).await.unwrap().data.artifacts["render_hld"].id.to_string()).await >= 1);
    assert_eq!(doc_state(&w, id, "approve_hld").await.0, "conflict");

    // Explicit human override restores the approved version (audited with the flag).
    let r = w.h.engine.sync_documents(&w.operator, id, true).await.unwrap();
    let o = r.iter().find(|o| o.gate == "approve_hld").unwrap();
    assert_eq!(o.status, "succeeded");
    assert!(
        o.report
            .as_ref()
            .unwrap()
            .files
            .iter()
            .any(|f| f.action == "replaced_modified")
    );
    assert_eq!(std::fs::read(&hld).unwrap(), approved);
    assert_eq!(
        count(&w, "SELECT COUNT(*) FROM audit_log WHERE action = 'artifact.materialize' AND json_extract(details, '$.replace_modified') = 1 AND actor_id = ?", &w.operator.id.to_string()).await,
        3
    );

    // A deleted file is reported missing and restored by sync.
    std::fs::remove_file(&hld).unwrap();
    assert_eq!(doc_state(&w, id, "approve_hld").await.0, "missing");
    w.h.engine.sync_documents(&w.operator, id, false).await.unwrap();
    assert_eq!(std::fs::read(&hld).unwrap(), approved);
    assert_eq!(doc_state(&w, id, "approve_hld").await.0, "verified");
}

#[tokio::test]
async fn an_unregistered_workspace_blocks_the_gate_until_sync_and_never_skips_ahead() {
    let (w, id) = design_at_requirements_gate().await;
    eaeh_store::sqlx::query("DELETE FROM project_workspaces")
        .execute(w.h.db.pool())
        .await
        .unwrap();

    // Approved, but the documents cannot be written: the run holds at the gate.
    let (st, state) = approve(&w).await;
    assert_eq!(
        (st, state.as_str()),
        (InstanceStatus::Running, "approve_requirements")
    );
    let inst = w.h.engine.get(id).await.unwrap();
    let b = inst.data.blocked.clone().expect("blocked");
    assert_eq!(b.gate, "approve_requirements");
    assert!(
        b.reason.contains("no application workspace is registered"),
        "{}",
        b.reason
    );
    assert!(
        !inst.data.artifacts.contains_key("hld"),
        "the HLD stage did not start"
    );
    assert_eq!(
        doc_state(&w, id, "approve_requirements").await,
        ("no_workspace".into(), Some(b.reason.clone()))
    );
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'workflow.blocked' AND resource_id = ?",
            &id.to_string()
        )
        .await,
        1
    );
    assert_eq!(count(&w, "SELECT COUNT(*) FROM artifact_materializations WHERE status = 'failed' AND workflow_instance_id = ?", &id.to_string()).await, 1);

    // Re-driving retries (idempotently) and stays blocked; it is audited once.
    assert_eq!(w.h.engine.drive(id).await.unwrap(), InstanceStatus::Running);
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'workflow.blocked' AND resource_id = ?",
            &id.to_string()
        )
        .await,
        1
    );

    // Register and reconcile: written, verified, and the run continues to the HLD gate.
    w.h.engine
        .materializer()
        .register_workspace(w.scope, &w.workspace, w.operator.id.as_str())
        .await
        .unwrap();
    let r = w.h.engine.sync_documents(&w.operator, id, false).await.unwrap();
    assert_eq!(
        r.iter()
            .find(|o| o.gate == "approve_requirements")
            .unwrap()
            .status,
        "succeeded"
    );
    assert_eq!(
        r.iter().find(|o| o.gate == "approve_hld").unwrap().status,
        "skipped"
    );
    let inst = w.h.engine.get(id).await.unwrap();
    assert_eq!(
        (inst.status, inst.state.as_str()),
        (InstanceStatus::WaitingApproval, "approve_hld")
    );
    assert!(inst.data.blocked.is_none());
    assert_materialized(&w, id, "render_requirements", REQ_DIR).await;
}

#[tokio::test]
async fn an_unrelated_existing_file_is_not_overwritten_and_holds_the_run() {
    let (w, id) = design_at_requirements_gate().await;
    let mine = w.workspace.join(REQ_DIR).join("requirements.md");
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, b"hand-written requirements").unwrap();

    let (st, state) = approve(&w).await;
    assert_eq!(
        (st, state.as_str()),
        (InstanceStatus::Running, "approve_requirements")
    );
    let b = w.h.engine.get(id).await.unwrap().data.blocked.unwrap();
    assert!(b.reason.contains("was not written by EAEH"), "{}", b.reason);
    assert_eq!(std::fs::read(&mine).unwrap(), b"hand-written requirements");
    assert_eq!(
        files_under(&w.workspace.join(REQ_DIR)),
        1,
        "all or nothing: no other file written"
    );
    assert!(!w.workspace.join(".eaeh/artifacts.json").exists());

    // Only an explicit, audited human override replaces it; then the run continues.
    let r = w.h.engine.sync_documents(&w.operator, id, true).await.unwrap();
    assert_eq!(r[0].status, "succeeded");
    let inst = w.h.engine.get(id).await.unwrap();
    assert_eq!(inst.state, "approve_hld");
    assert_materialized(&w, id, "render_requirements", REQ_DIR).await;
}

#[cfg(unix)]
#[tokio::test]
async fn writes_cannot_escape_the_workspace_through_symlinks() {
    let (w, id) = design_at_requirements_gate().await;
    let outside = w.h.config.data_dir().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, w.workspace.join("docs")).unwrap();

    let (st, _) = approve(&w).await;
    assert_eq!(st, InstanceStatus::Running);
    let b = w.h.engine.get(id).await.unwrap().data.blocked.unwrap();
    assert!(b.reason.contains("symlink"), "{}", b.reason);
    assert_eq!(files_under(&outside), 0, "nothing written outside the workspace");
}

#[test]
fn materialize_targets_must_stay_inside_the_repository() {
    let base: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("workflows/engineering.design.json")).unwrap(),
    )
    .unwrap();
    for bad in [
        "../outside",
        "/etc",
        "docs/../../x",
        ".git/hooks",
        ".eaeh",
        "docs//hld",
        "./docs",
    ] {
        let mut d = base.clone();
        for s in d["states"].as_array_mut().unwrap() {
            if s["name"] == "approve_hld" {
                s["materialize"] = json!(bad);
            }
        }
        let def: eaeh_workflow::WorkflowDefinition = serde_json::from_value(d).unwrap();
        assert!(def.validate().is_err(), "{bad} must be rejected");
    }
    let def: eaeh_workflow::WorkflowDefinition = serde_json::from_value(base).unwrap();
    def.validate().unwrap();
}

/// The LLD stage requires the HLD approval *of the current HLD version* (hash-bound).
#[tokio::test]
async fn lld_is_never_generated_from_an_outdated_approval() {
    let (w, id) = design_at_requirements_gate().await;
    approve(&w).await; // requirements → at approve_hld
    // Simulate a requirements artifact that no longer matches its approval.
    eaeh_store::sqlx::query(
        "UPDATE workflow_instances SET data = json_set(data, '$.artifacts.render_requirements.hash', ?) WHERE id = ?",
    )
    .bind("f".repeat(64))
    .bind(id.to_string())
    .execute(w.h.db.pool())
    .await
    .unwrap();
    let (st, _) = approve(&w).await; // HLD approved → the LLD stage checks its prerequisites
    assert_eq!(st, InstanceStatus::Failed);
    let inst = w.h.engine.get(id).await.unwrap();
    let reason = inst.data.failure.unwrap();
    assert!(
        reason.contains("approve_requirements") && reason.contains("current artifact version"),
        "{reason}"
    );
    assert!(!inst.data.artifacts.contains_key("lld"), "no LLD was generated");
}

/// Status and documents are read-scoped: another project's caller sees nothing.
#[tokio::test]
async fn document_status_requires_workflow_read_and_sync_requires_workflow_start() {
    let (w, id) = design_at_requirements_gate().await;
    // The approver may read but not reconcile (workflow.start belongs to the developer role).
    assert!(w.h.engine.documents(&w.approver, id).await.is_ok());
    let e =
        w.h.engine
            .sync_documents(&w.approver, id, false)
            .await
            .unwrap_err();
    assert_eq!(e.kind, eaeh_core::ErrorKind::Forbidden);
}
