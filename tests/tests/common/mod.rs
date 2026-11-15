//! Shared fixture of the Phase 3 and external-AI acceptance suites.
#![allow(dead_code)]

pub use eaeh_agents::ExecutionMode;
use eaeh_core::{
    ApprovalId, Classification, PrincipalId, PrincipalKind, ProjectAccess, Role, Scope, TraceContext,
};
use eaeh_governance::{ApprovalDecision, ApprovalStatus, NewPrincipal};
use eaeh_knowledge::{Format, IngestRequest, KnowledgeScope, SourceKind};
use eaeh_runtime::{Harness, HarnessConfig};
use eaeh_testkit::{Script, ScriptedModelProvider};
use eaeh_workflow::InstanceStatus;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

pub const BRD: &str = "demo-brd/order-management@v1";
pub const STD: &str = "demo-api-standards/error-format@v1";

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub fn example(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root().join("examples/design").join(name)).unwrap())
        .unwrap()
}

pub struct World {
    pub h: Arc<Harness>,
    /// Registered application repository (approved documents are materialized here).
    pub workspace: PathBuf,
    pub provider: Arc<ScriptedModelProvider>,
    pub scope: Scope,
    pub operator: eaeh_core::Principal,
    pub approver: eaeh_core::Principal,
    pub steward: eaeh_core::Principal,
    pub admin: eaeh_core::Principal,
}

/// Harness on the shipped configuration with people, the approved demonstration
/// standard, the governed BRD (project-scoped, confidential) and the approved Phase 3 skills.
pub async fn world(script: Vec<Script>, brd_extra: &str) -> World {
    world_in(ExecutionMode::DirectProvider, script, brd_extra).await
}

/// The fixture in a given execution mode (the scripted provider is offered to the builder in
/// every mode; only `direct_provider` registers it).
pub async fn world_in(mode: ExecutionMode, script: Vec<Script>, brd_extra: &str) -> World {
    world_cfg(mode, script, brd_extra, |_| {}).await
}

/// The fixture with a configuration adjustment (e.g. coordinator limits in tests).
pub async fn world_cfg(
    mode: ExecutionMode,
    script: Vec<Script>,
    brd_extra: &str,
    adjust: impl FnOnce(&mut HarnessConfig),
) -> World {
    let mut cfg = HarnessConfig::load(&root().join("config/eaeh.toml")).unwrap();
    cfg.execution.mode = mode;
    // Hermetic: never probe or run a real Claude Code installation from tests.
    cfg.execution.coordinator.claude_path = Some("/nonexistent/claude".into());
    cfg.execution.coordinator.auto_start = false;
    adjust(&mut cfg);
    cfg.storage.data_dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("design-{}", uuid::Uuid::now_v7()));
    cfg.isolation.container_enabled = false;
    let provider = Arc::new(ScriptedModelProvider::new("scripted", "scripted-model", script));
    let h = Arc::new(
        Harness::builder(cfg)
            .model_provider(provider.clone())
            .build()
            .await
            .unwrap(),
    );
    let t = TraceContext::new_root();
    let (admin, project, _) = h
        .identity
        .bootstrap("acme", "orders", PrincipalId::new("admin").unwrap(), "Admin", 30)
        .await
        .unwrap();
    let mk = |id: &str, r: Role| NewPrincipal {
        id: PrincipalId::new(id).unwrap(),
        display_name: id.into(),
        kind: PrincipalKind::Human,
        roles: [r].into(),
        projects: ProjectAccess::All,
    };
    let steward = h
        .identity
        .create_principal(&admin, mk("sam", Role::KnowledgeSteward))
        .await
        .unwrap();
    let approver = h
        .identity
        .create_principal(&admin, mk("arthur", Role::Approver))
        .await
        .unwrap();
    let operator = h
        .identity
        .create_principal(&admin, mk("olivia", Role::Operator))
        .await
        .unwrap();

    let kb = &h.knowledge;
    kb.register_source(
        &steward,
        "demo-api-standards",
        SourceKind::Standard,
        "Demonstration API standards",
    )
    .await
    .unwrap();
    kb.register_source(&steward, "demo-brd", SourceKind::Reference, "Demonstration BRDs")
        .await
        .unwrap();
    let std_doc = kb
        .ingest(
            &steward,
            IngestRequest {
                source: "demo-api-standards".into(),
                subject: "error-format".into(),
                title: "Error response format".into(),
                format: Format::Markdown,
                content: std::fs::read_to_string(root().join("examples/knowledge/standards/error-format.md"))
                    .unwrap(),
                scope: KnowledgeScope::Enterprise,
                classification: Classification::Internal,
                tags: vec![],
                mandatory: true,
                review_by: None,
                governance_project: project,
            },
            t,
        )
        .await
        .unwrap();
    h.decide_approval(
        &approver,
        std_doc.approval_id.clone().unwrap().parse().unwrap(),
        ApprovalDecision::Approve,
        "standard approved",
        Some(&std_doc.content_hash),
    )
    .await
    .unwrap();
    let brd = std::fs::read_to_string(root().join("examples/brd/order-management.md")).unwrap() + brd_extra;
    let doc = kb
        .ingest(
            &steward,
            IngestRequest {
                source: "demo-brd".into(),
                subject: "order-management".into(),
                title: "Order management BRD".into(),
                format: Format::Markdown,
                content: brd,
                scope: KnowledgeScope::Project(project),
                classification: Classification::Confidential,
                tags: vec!["brd".into()],
                mandatory: false,
                review_by: None,
                governance_project: project,
            },
            t,
        )
        .await
        .unwrap();
    assert_eq!(doc.chunks, if brd_extra.is_empty() { 8 } else { 9 });
    for s in [
        "requirements-analysis",
        "hld-design",
        "lld-design",
        "implementation-planning",
        "code-implementation",
    ] {
        let def: eaeh_skills::SkillDefinition = serde_json::from_str(
            &std::fs::read_to_string(root().join(format!("config/skills/{s}.json"))).unwrap(),
        )
        .unwrap();
        let rec = h.skills.register(&steward, &def, project, t).await.unwrap();
        // The steward who registered a skill can never approve it.
        let aid: ApprovalId = rec.approval_id.clone().unwrap().parse().unwrap();
        assert!(
            h.decide_approval(&steward, aid, ApprovalDecision::Approve, "", Some(&rec.digest))
                .await
                .is_err()
        );
        h.decide_approval(
            &approver,
            aid,
            ApprovalDecision::Approve,
            "skill approved",
            Some(&rec.digest),
        )
        .await
        .unwrap();
    }
    // The application repository approved documents are written to (ARTIFACT-MATERIALIZATION-PLAN).
    let workspace = h.config.data_dir().join("app-repo");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    h.engine
        .materializer()
        .register_workspace(
            Scope::new(admin.tenant, project),
            &workspace,
            operator.id.as_str(),
        )
        .await
        .unwrap();
    World {
        h,
        provider,
        workspace,
        scope: Scope::new(admin.tenant, project),
        operator,
        approver,
        steward,
        admin,
    }
}

pub async fn pending_gate(w: &World) -> (ApprovalId, String, String) {
    let a =
        w.h.approvals
            .list(w.scope, Some(ApprovalStatus::Pending), None, 5)
            .await
            .unwrap()
            .remove(0);
    (a.id, a.artifact_hash.clone().unwrap(), a.gate.clone().unwrap())
}

/// Decide the pending gate; returns the workflow status and the state it is now in.
pub async fn decide(w: &World, d: ApprovalDecision, comments: &str) -> (InstanceStatus, String) {
    let (id, hash, _gate) = pending_gate(w).await;
    let rec = w.h.approvals.get(id).await.unwrap();
    let st =
        w.h.engine
            .decide_approval(
                &w.approver,
                id,
                d,
                comments,
                Some(&hash),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    let inst =
        w.h.engine
            .get(rec.workflow_instance_id.unwrap().parse().unwrap())
            .await
            .unwrap();
    (st, inst.state)
}

pub async fn read_bundle_file(
    w: &World,
    inst: &eaeh_workflow::WorkflowInstance,
    state: &str,
    path: &str,
) -> String {
    let manifest_ref = &inst.data.artifacts[state];
    let (_, bytes) = w.h.artifacts.read(w.scope, manifest_ref.id).await.unwrap();
    let manifest: Value = serde_json::from_slice(&bytes).unwrap();
    let id = manifest["files"][path]["artifact_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{path} not in bundle"));
    let (_, content) = w.h.artifacts.read(w.scope, id.parse().unwrap()).await.unwrap();
    String::from_utf8(content).unwrap()
}
