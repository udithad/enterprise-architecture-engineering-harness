//! Phase 2 demonstration (AC2-20): an agent reviews an API spec using an **approved
//! skill** and **governed enterprise knowledge**, produces a cited artifact, and a human
//! approves it. Everything is production code except the model, which is the labelled
//! test double `ScriptedModelProvider` (no live API call; P2-D6).

use eaeh_core::{
    ApprovalId, PrincipalId, PrincipalKind, ProjectAccess, Role, Scope, TraceContext, WorkflowDefinitionId,
};
use eaeh_governance::{ApprovalDecision, ApprovalStatus, NewPrincipal};
use eaeh_knowledge::{DocStatus, Format, IngestRequest, KnowledgeScope, SearchMode, SearchQuery, SourceKind};
use eaeh_runtime::{Harness, HarnessConfig};
use eaeh_testkit::{Script, ScriptedModelProvider};
use eaeh_workflow::InstanceStatus;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[tokio::test]
async fn agent_reviews_api_spec_with_approved_skill_and_enterprise_knowledge() {
    let mut cfg = HarnessConfig::load(&root().join("config/eaeh.toml")).unwrap();
    // Uses a scripted model provider: direct_provider mode (backward-compatible path).
    cfg.execution.mode = eaeh_agents::ExecutionMode::DirectProvider;
    cfg.storage.data_dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("kdemo-{}", uuid::Uuid::now_v7()));
    cfg.isolation.container_enabled = false;
    let review = json!({
        "summary": "Orders API violates 3 rules",
        "findings": [
            {"rule": "error format: application/problem+json", "location": "GET /order 500 text/plain", "severity": "high"},
            {"rule": "plural collection nouns", "location": "/order", "severity": "medium"},
            {"rule": "cursor pagination", "location": "GET /order ?page", "severity": "low"}
        ],
        "citations": ["demo-api-standards/error-format@v1", "demo-api-guides/resource-naming@v1#0", "demo-api-guides/pagination@v1#0"]
    });
    let provider = Arc::new(ScriptedModelProvider::new(
        "scripted",
        "scripted-model",
        vec![
            Script::tool(
                "knowledge.search",
                json!({"query": "listOrders GET /order responses", "k": 3}),
            ),
            Script::json(review.clone()),
        ],
    ));
    let h = Arc::new(
        Harness::builder(cfg)
            .model_provider(provider.clone())
            .build()
            .await
            .unwrap(),
    );
    let t = TraceContext::new_root();

    // People: admin bootstraps; a knowledge steward curates; an approver approves; an operator runs.
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

    // Governed knowledge from the example corpus.
    let kb = &h.knowledge;
    kb.register_source(
        &steward,
        "demo-api-standards",
        SourceKind::Standard,
        "Demonstration API standards",
    )
    .await
    .unwrap();
    kb.register_source(
        &steward,
        "demo-api-guides",
        SourceKind::Reference,
        "Demonstration API guides",
    )
    .await
    .unwrap();
    kb.register_source(
        &steward,
        "demo-api-specs",
        SourceKind::ApiSpec,
        "Specifications under review",
    )
    .await
    .unwrap();
    let file = |p: &str| std::fs::read_to_string(root().join("examples/knowledge").join(p)).unwrap();
    let ingest =
        |source: &str, subject: &str, path: &str, format: Format, scope: KnowledgeScope, mandatory: bool| {
            IngestRequest {
                source: source.into(),
                subject: subject.into(),
                title: subject.into(),
                format,
                content: file(path),
                scope,
                classification: Default::default(),
                tags: vec![],
                mandatory,
                review_by: None,
                governance_project: project,
            }
        };
    let std_doc = kb
        .ingest(
            &steward,
            ingest(
                "demo-api-standards",
                "error-format",
                "standards/error-format.md",
                Format::Markdown,
                KnowledgeScope::Enterprise,
                true,
            ),
            t,
        )
        .await
        .unwrap();
    assert_eq!(std_doc.status, DocStatus::PendingApproval);
    kb.ingest(
        &steward,
        ingest(
            "demo-api-guides",
            "resource-naming",
            "guides/resource-naming.md",
            Format::Markdown,
            KnowledgeScope::Enterprise,
            false,
        ),
        t,
    )
    .await
    .unwrap();
    kb.ingest(
        &steward,
        ingest(
            "demo-api-guides",
            "pagination",
            "guides/pagination.md",
            Format::Markdown,
            KnowledgeScope::Enterprise,
            false,
        ),
        t,
    )
    .await
    .unwrap();
    kb.ingest(
        &steward,
        ingest(
            "demo-api-specs",
            "orders-api",
            "specs/orders-api.json",
            Format::Openapi,
            KnowledgeScope::Project(project),
            false,
        ),
        t,
    )
    .await
    .unwrap();
    let aid: ApprovalId = std_doc.approval_id.clone().unwrap().parse().unwrap();
    h.decide_approval(
        &approver,
        aid,
        ApprovalDecision::Approve,
        "standard approved",
        Some(&std_doc.content_hash),
    )
    .await
    .unwrap();
    assert_eq!(
        kb.get(&steward, &std_doc.id).await.unwrap().status,
        DocStatus::Active
    );

    // Approved skill from the example definition.
    let skill: eaeh_skills::SkillDefinition = serde_json::from_str(
        &std::fs::read_to_string(root().join("config/skills/api-design-review.json")).unwrap(),
    )
    .unwrap();
    let rec = h.skills.register(&steward, &skill, project, t).await.unwrap();
    let saved: ApprovalId = rec.approval_id.clone().unwrap().parse().unwrap();
    h.decide_approval(
        &approver,
        saved,
        ApprovalDecision::Approve,
        "skill approved",
        Some(&rec.digest),
    )
    .await
    .unwrap();

    // Humans can search too (cited, scoped).
    let ctx = kb.context_for_human(&operator, project).await.unwrap();
    let hits = kb
        .search(
            &ctx,
            &SearchQuery::new("plural nouns for collections", SearchMode::Hybrid, 3),
            t,
        )
        .await
        .unwrap();
    assert_eq!(hits[0].citation, "demo-api-guides/resource-naming@v1#0");

    // Run the review workflow.
    let scope = Scope::new(admin.tenant, project);
    let id = h
        .engine
        .start(
            &operator,
            &WorkflowDefinitionId::new("knowledge.review").unwrap(),
            scope,
            json!({"spec": "demo-api-specs/orders-api"}),
            t,
        )
        .await
        .unwrap();
    assert_eq!(h.engine.drive(id).await.unwrap(), InstanceStatus::WaitingApproval);
    let inst = h.engine.get(id).await.unwrap();
    let art = inst.data.artifacts["review"].clone();
    let (meta, bytes) = h.artifacts.read(scope, art.id).await.unwrap();
    let out: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(out["citations"], review["citations"]);
    assert_eq!(meta.provenance.skills, vec!["api-design-review@1.0.0"]);
    for c in [
        "demo-api-standards/error-format@v1",
        "demo-api-guides/resource-naming@v1#0",
        "demo-api-guides/pagination@v1#0",
    ] {
        assert!(
            meta.provenance.knowledge.iter().any(|k| k == c),
            "missing {c} in {:?}",
            meta.provenance.knowledge
        );
    }
    assert!(
        meta.provenance
            .knowledge
            .iter()
            .any(|k| k.starts_with("demo-api-specs/orders-api@v1#")),
        "spec retrieved via knowledge.search"
    );

    // What the model saw: mandatory standard + approved skill as instructions; knowledge as cited data.
    let first = &provider.requests()[0];
    assert!(
        first.messages[0]
            .content
            .contains("MANDATORY ENTERPRISE STANDARD [demo-api-standards/error-format@v1]")
    );
    assert!(
        first.messages[0]
            .content
            .contains("APPROVED SKILL [api-design-review@1.0.0]")
    );
    assert!(
        first.messages[1]
            .content
            .contains("<data key=\"kb:demo-api-guides/pagination@v1#0\"")
    );

    // Human approval of the cited review completes the workflow.
    let gate = h
        .approvals
        .list(scope, Some(ApprovalStatus::Pending), None, 5)
        .await
        .unwrap()
        .remove(0);
    let st = h
        .engine
        .decide_approval(
            &approver,
            gate.id,
            ApprovalDecision::Approve,
            "review accepted",
            Some(&art.hash),
            t,
        )
        .await
        .unwrap();
    assert_eq!(st, InstanceStatus::Succeeded);
    let (skills, knowledge): (String, String) = eaeh_store::sqlx::query_as(
        "SELECT skill_refs, knowledge_refs FROM model_usage ORDER BY occurred_at DESC LIMIT 1",
    )
    .fetch_one(h.db.pool())
    .await
    .unwrap();
    assert!(
        skills.contains("api-design-review@1.0.0")
            && knowledge.contains("demo-api-standards/error-format@v1")
    );
    assert!(h.db.verify_audit_chain().await.unwrap().valid);
}
