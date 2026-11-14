//! Knowledge management: ingestion, governance, lifecycle, retrieval and isolation
//! (AC2-03…AC2-11, AC2-18).

use eaeh_core::{
    ApprovalId, Classification, ErrorKind, Principal, PrincipalId, PrincipalKind, ProjectAccess, ProjectId,
    Role, Scope, TenantId, TraceContext, WorkflowInstanceId,
};
use eaeh_extensibility::HookRegistry;
use eaeh_governance::authz::RbacAuthorizer;
use eaeh_governance::{ApprovalDecision, ApprovalManager, IdentityService, NewPrincipal};
use eaeh_intelligence::{HashedNgramEmbedder, TrustLevel};
use eaeh_knowledge::*;
use eaeh_store::Db;
use std::sync::Arc;

struct Fx {
    _d: tempfile::TempDir,
    db: Db,
    kb: KnowledgeService,
    approvals: ApprovalManager,
    identity: IdentityService,
    admin: Principal,
    steward: Principal,
    approver: Principal,
    viewer: Principal,
    p1: ProjectId,
    p2: ProjectId,
}

async fn fx() -> Fx {
    let d = tempfile::tempdir().unwrap();
    let db = Db::open(&d.path().join("k.db")).await.unwrap();
    let authz = Arc::new(RbacAuthorizer);
    let identity = IdentityService::new(db.clone(), authz.clone());
    let (admin, p1, _t) = identity
        .bootstrap(
            "acme",
            "payments",
            PrincipalId::new("admin").unwrap(),
            "Admin",
            30,
        )
        .await
        .unwrap();
    let p2 = identity
        .create_project(&admin, admin.tenant, "logistics")
        .await
        .unwrap();
    let mk = |id: &str, roles: &[Role], projects: ProjectAccess| NewPrincipal {
        id: PrincipalId::new(id).unwrap(),
        display_name: id.into(),
        kind: PrincipalKind::Human,
        roles: roles.iter().copied().collect(),
        projects,
    };
    let steward = identity
        .create_principal(&admin, mk("sam", &[Role::KnowledgeSteward], ProjectAccess::All))
        .await
        .unwrap();
    let approver = identity
        .create_principal(&admin, mk("arthur", &[Role::Approver], ProjectAccess::All))
        .await
        .unwrap();
    let viewer = identity
        .create_principal(
            &admin,
            mk("vera", &[Role::Viewer], ProjectAccess::Only([p1].into())),
        )
        .await
        .unwrap();
    let approvals = ApprovalManager::new(db.clone(), authz.clone());
    let kb = KnowledgeService::new(
        db.clone(),
        authz,
        approvals.clone(),
        Arc::new(HookRegistry::new()),
        Arc::new(HashedNgramEmbedder::new(256).unwrap()),
        KnowledgeConfig::default(),
    );
    Fx {
        _d: d,
        db,
        kb,
        approvals,
        identity,
        admin,
        steward,
        approver,
        viewer,
        p1,
        p2,
    }
}

fn req(f: &Fx, source: &str, subject: &str, content: &str, scope: KnowledgeScope) -> IngestRequest {
    IngestRequest {
        source: source.into(),
        subject: subject.into(),
        title: format!("{subject} title"),
        format: Format::Markdown,
        content: content.into(),
        scope,
        classification: Classification::Internal,
        tags: vec![],
        mandatory: false,
        review_by: None,
        governance_project: f.p1,
    }
}

const ERRORS_STD: &str = "# Error responses\nAll HTTP APIs must return errors as application/problem+json.\n\n## Fields\nEvery error includes type, title, status and a trace_id.\n";
const NAMING: &str = "# Resource naming\nUse plural nouns for collections, for example /orders and /customers.\n\n## Casing\nPath segments use kebab-case.\n";

async fn sources(f: &Fx) {
    f.kb.register_source(
        &f.steward,
        "api-standards",
        SourceKind::Standard,
        "Enterprise API standards",
    )
    .await
    .unwrap();
    f.kb.register_source(
        &f.steward,
        "api-guides",
        SourceKind::Reference,
        "API style guidance",
    )
    .await
    .unwrap();
}

async fn approve(f: &Fx, doc: &DocumentMeta) -> DocumentMeta {
    let aid: ApprovalId = doc.approval_id.clone().unwrap().parse().unwrap();
    f.approvals
        .decide(
            &f.approver,
            aid,
            ApprovalDecision::Approve,
            "ok",
            Some(&doc.content_hash),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    f.kb.apply_approval(aid, TraceContext::new_root())
        .await
        .unwrap()
        .unwrap()
}

async fn hctx(f: &Fx, who: &Principal, project: ProjectId) -> RetrievalContext {
    f.kb.context_for_human(who, project).await.unwrap()
}

#[tokio::test]
async fn reference_documents_are_indexed_and_searchable_with_citations() {
    let f = fx().await;
    sources(&f).await;
    let doc =
        f.kb.ingest(
            &f.steward,
            req(
                &f,
                "api-guides",
                "resource-naming",
                NAMING,
                KnowledgeScope::Enterprise,
            ),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    assert_eq!(doc.status, DocStatus::Active);
    assert_eq!(doc.version, 1);
    assert_eq!(doc.chunks, 2);
    let ctx = hctx(&f, &f.viewer, f.p1).await;
    for mode in [SearchMode::Lexical, SearchMode::Vector, SearchMode::Hybrid] {
        let hits =
            f.kb.search(
                &ctx,
                &SearchQuery::new("plural nouns for collections", mode, 3),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
        assert!(!hits.is_empty(), "{mode:?}");
        assert_eq!(hits[0].citation, "api-guides/resource-naming@v1#0", "{mode:?}");
        assert_eq!(hits[0].content_hash, doc.content_hash);
    }
    let audit: (i64,) =
        eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM audit_log WHERE action = 'knowledge.search'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(audit.0, 3);
    let ev: (i64,) = eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM outbox WHERE event_type IN ('knowledge.ingested','knowledge.activated','knowledge.retrieved')").fetch_one(f.db.pool()).await.unwrap();
    assert_eq!(ev.0, 5);
}

#[tokio::test]
async fn standards_activate_only_after_human_approval_bound_to_hash() {
    let f = fx().await;
    sources(&f).await;
    let mut r = req(
        &f,
        "api-standards",
        "error-format",
        ERRORS_STD,
        KnowledgeScope::Enterprise,
    );
    r.mandatory = true;
    let doc =
        f.kb.ingest(&f.steward, r, TraceContext::new_root())
            .await
            .unwrap();
    assert_eq!(doc.status, DocStatus::PendingApproval);
    let ctx = hctx(&f, &f.viewer, f.p1).await;
    assert!(
        f.kb.search(
            &ctx,
            &SearchQuery::new("problem json errors", SearchMode::Hybrid, 5),
            TraceContext::new_root()
        )
        .await
        .unwrap()
        .is_empty()
    );
    assert!(f.kb.standards_context(&ctx).await.unwrap().is_empty());

    let aid: ApprovalId = doc.approval_id.clone().unwrap().parse().unwrap();
    let t = TraceContext::new_root();
    // The steward (author, and not an approver) cannot approve; a wrong hash is rejected.
    assert_eq!(
        f.approvals
            .decide(
                &f.steward,
                aid,
                ApprovalDecision::Approve,
                "",
                Some(&doc.content_hash),
                t
            )
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
    assert_eq!(
        f.approvals
            .decide(&f.approver, aid, ApprovalDecision::Approve, "", Some("0000"), t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    let active = approve(&f, &doc).await;
    assert_eq!(active.status, DocStatus::Active);

    let items = f.kb.standards_context(&ctx).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].trust(), TrustLevel::EnterprisePolicy);
    assert!(items[0].key().contains("api-standards/error-format@v1"));

    // Mandatory flag only for enterprise/domain standard sources.
    let mut bad = req(&f, "api-guides", "x", NAMING, KnowledgeScope::Enterprise);
    bad.mandatory = true;
    assert_eq!(
        f.kb.ingest(&f.steward, bad, t).await.unwrap_err().kind,
        ErrorKind::InvalidInput
    );
}

#[tokio::test]
async fn rejected_standards_never_activate() {
    let f = fx().await;
    sources(&f).await;
    let doc =
        f.kb.ingest(
            &f.steward,
            req(
                &f,
                "api-standards",
                "error-format",
                ERRORS_STD,
                KnowledgeScope::Enterprise,
            ),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    let aid: ApprovalId = doc.approval_id.clone().unwrap().parse().unwrap();
    f.approvals
        .decide(
            &f.approver,
            aid,
            ApprovalDecision::Reject,
            "too vague",
            Some(&doc.content_hash),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    assert_eq!(
        f.kb.apply_approval(aid, TraceContext::new_root())
            .await
            .unwrap()
            .unwrap()
            .status,
        DocStatus::Rejected
    );
}

#[tokio::test]
async fn documents_with_credentials_are_quarantined_and_never_retrievable() {
    let f = fx().await;
    sources(&f).await;
    let secret_doc = "# Deploy notes\nUse this key:\n-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----\n";
    let doc =
        f.kb.ingest(
            &f.steward,
            req(
                &f,
                "api-guides",
                "deploy-notes",
                secret_doc,
                KnowledgeScope::Enterprise,
            ),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    assert_eq!(doc.status, DocStatus::Quarantined);
    assert_eq!(doc.chunks, 0, "quarantined documents are not indexed");
    let (content,): (Option<String>,) =
        eaeh_store::sqlx::query_as("SELECT content FROM knowledge_documents WHERE id = ?")
            .bind(&doc.id)
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert!(content.is_none(), "quarantined content is not stored");
    let ctx = hctx(&f, &f.admin, f.p1).await;
    assert!(
        f.kb.search(
            &ctx,
            &SearchQuery::new("deploy notes key", SearchMode::Hybrid, 5),
            TraceContext::new_root()
        )
        .await
        .unwrap()
        .is_empty()
    );
    let audit: Vec<(String,)> =
        eaeh_store::sqlx::query_as("SELECT details FROM audit_log WHERE action = 'knowledge.ingest'")
            .fetch_all(f.db.pool())
            .await
            .unwrap();
    assert!(audit.iter().all(|(d,)| !d.contains("MIIEvQ")));
}

#[tokio::test]
async fn scope_tenant_and_classification_isolation() {
    let f = fx().await;
    sources(&f).await;
    f.identity
        .set_project_domain(&f.admin, f.p1, Some("commerce"))
        .await
        .unwrap();
    let wf = WorkflowInstanceId::new();
    let t = TraceContext::new_root();
    let ingest = |subject: &str, scope: KnowledgeScope, class: Classification| {
        let mut r = req(
            &f,
            "api-guides",
            subject,
            &format!("# {subject}\nzebra {subject} guidance text\n"),
            scope,
        );
        r.classification = class;
        r
    };
    f.kb.ingest(
        &f.steward,
        ingest("p1-only", KnowledgeScope::Project(f.p1), Classification::Internal),
        t,
    )
    .await
    .unwrap();
    f.kb.ingest(
        &f.steward,
        ingest("p2-only", KnowledgeScope::Project(f.p2), Classification::Internal),
        t,
    )
    .await
    .unwrap();
    f.kb.ingest(
        &f.steward,
        ingest(
            "commerce-domain",
            KnowledgeScope::Domain("commerce".into()),
            Classification::Internal,
        ),
        t,
    )
    .await
    .unwrap();
    f.kb.ingest(
        &f.steward,
        ingest(
            "team-platform",
            KnowledgeScope::Team("platform".into()),
            Classification::Internal,
        ),
        t,
    )
    .await
    .unwrap();
    f.kb.ingest(
        &f.steward,
        ingest(
            "restricted-note",
            KnowledgeScope::Enterprise,
            Classification::Restricted,
        ),
        t,
    )
    .await
    .unwrap();
    // Workflow-scoped documents need an existing workflow instance; insert one directly.
    eaeh_store::sqlx::query("INSERT INTO workflow_definitions (id, version, document, digest, created_at) VALUES ('wf.x', '1.0.0', '{}', 'd', 'now')").execute(f.db.pool()).await.unwrap();
    eaeh_store::sqlx::query("INSERT INTO workflow_instances (id, definition_id, definition_version, tenant_id, project_id, state, status, version, input, data, started_by, correlation_id, created_at, updated_at) VALUES (?, 'wf.x', '1.0.0', ?, ?, 's', 'running', 1, '{}', '{}', 'admin', 'c', 'now', 'now')")
        .bind(wf.to_string()).bind(f.admin.tenant.to_string()).bind(f.p1.to_string()).execute(f.db.pool()).await.unwrap();
    f.kb.ingest(
        &f.steward,
        ingest("wf-only", KnowledgeScope::Workflow(wf), Classification::Internal),
        t,
    )
    .await
    .unwrap();

    let found = |ctx: RetrievalContext| {
        let kb = f.kb.clone();
        async move {
            kb.search(
                &ctx,
                &SearchQuery::new("zebra guidance", SearchMode::Lexical, 20),
                TraceContext::new_root(),
            )
            .await
            .unwrap()
            .into_iter()
            .map(|h| h.subject)
            .collect::<std::collections::BTreeSet<_>>()
        }
    };
    let viewer_p1 = found(hctx(&f, &f.viewer, f.p1).await).await;
    assert_eq!(
        viewer_p1,
        ["commerce-domain", "p1-only"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    );
    // Viewer has no access to p2 at all.
    assert_eq!(
        f.kb.context_for_human(&f.viewer, f.p2).await.unwrap_err().kind,
        ErrorKind::Forbidden
    );
    // Team membership reveals team knowledge; privileged readers see restricted.
    f.identity
        .set_principal_teams(&f.admin, &f.viewer.id, &["platform".into()])
        .await
        .unwrap();
    assert!(
        found(hctx(&f, &f.viewer, f.p1).await)
            .await
            .contains("team-platform")
    );
    let admin_p2 = found(hctx(&f, &f.admin, f.p2).await).await;
    assert_eq!(
        admin_p2,
        ["p2-only", "restricted-note"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    );
    // Workflow-scoped knowledge only within that workflow.
    let mut wctx = hctx(&f, &f.viewer, f.p1).await;
    assert!(!found(wctx.clone()).await.contains("wf-only"));
    wctx.workflow = Some(wf);
    assert!(found(wctx).await.contains("wf-only"));
    // Another tenant sees nothing.
    let other_tenant = TenantId::new();
    let foreign = RetrievalContext {
        tenant: other_tenant,
        project: Some(f.p1),
        domain: Some("commerce".into()),
        teams: vec!["platform".into()],
        workflow: Some(wf),
        max_classification: Classification::Restricted,
        principal_id: "x".into(),
    };
    assert!(found(foreign).await.is_empty());
    // Steward cannot ingest into a project they cannot access; viewer cannot ingest at all.
    let limited = f
        .identity
        .create_principal(
            &f.admin,
            NewPrincipal {
                id: PrincipalId::new("lim").unwrap(),
                display_name: "l".into(),
                kind: PrincipalKind::Human,
                roles: [Role::KnowledgeSteward].into(),
                projects: ProjectAccess::Only([f.p1].into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.kb.ingest(
            &limited,
            ingest("x", KnowledgeScope::Project(f.p2), Classification::Internal),
            t
        )
        .await
        .unwrap_err()
        .kind,
        ErrorKind::Forbidden
    );
    assert_eq!(
        f.kb.ingest(
            &f.viewer,
            ingest("y", KnowledgeScope::Enterprise, Classification::Internal),
            t
        )
        .await
        .unwrap_err()
        .kind,
        ErrorKind::Forbidden
    );
    let _ = Scope::new(f.admin.tenant, f.p1);
}

#[tokio::test]
async fn versions_supersede_and_retire_and_delete_purges() {
    let f = fx().await;
    sources(&f).await;
    let t = TraceContext::new_root();
    let v1 =
        f.kb.ingest(
            &f.steward,
            req(
                &f,
                "api-guides",
                "naming",
                "# Naming\nUse quokka style names.\n",
                KnowledgeScope::Enterprise,
            ),
            t,
        )
        .await
        .unwrap();
    let v2 =
        f.kb.ingest(
            &f.steward,
            req(
                &f,
                "api-guides",
                "naming",
                "# Naming\nUse wombat style names.\n",
                KnowledgeScope::Enterprise,
            ),
            t,
        )
        .await
        .unwrap();
    assert_eq!(v2.version, 2);
    assert_eq!(
        f.kb.get(&f.steward, &v1.id).await.unwrap().status,
        DocStatus::Superseded
    );
    let ctx = hctx(&f, &f.admin, f.p1).await;
    let s = |q: &str| {
        let kb = f.kb.clone();
        let ctx = ctx.clone();
        let q = q.to_string();
        async move {
            kb.search(
                &ctx,
                &SearchQuery::new(&q, SearchMode::Hybrid, 5),
                TraceContext::new_root(),
            )
            .await
            .unwrap()
        }
    };
    assert!(s("quokka").await.iter().all(|h| h.version == 2));
    assert_eq!(s("wombat").await[0].citation, "api-guides/naming@v2#0");
    f.kb.retire(&f.steward, &v2.id).await.unwrap();
    assert!(s("wombat").await.is_empty());
    let deleted = f.kb.delete(&f.steward, &v2.id).await.unwrap();
    assert_eq!(deleted.status, DocStatus::Deleted);
    let (content,): (Option<String>,) =
        eaeh_store::sqlx::query_as("SELECT content FROM knowledge_documents WHERE id = ?")
            .bind(&v2.id)
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert!(content.is_none());
    let (fts,): (i64,) = eaeh_store::sqlx::query_as(
        "SELECT COUNT(*) FROM knowledge_chunks_fts WHERE knowledge_chunks_fts MATCH 'wombat'",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(fts, 0, "index entries purged");
    let (emb,): (i64,) =
        eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM knowledge_embeddings WHERE chunk_id LIKE ?")
            .bind(format!("{}%", v2.id))
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(emb, 0);
    assert!(f.db.verify_audit_chain().await.unwrap().valid);
    assert_eq!(
        f.kb.delete(&f.steward, &v2.id).await.unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        f.kb.retire(&f.viewer, &v1.id).await.unwrap_err().kind,
        ErrorKind::Forbidden
    );
}

#[tokio::test]
async fn conflicts_freshness_and_standard_precedence() {
    let f = fx().await;
    sources(&f).await;
    f.identity
        .set_project_domain(&f.admin, f.p1, Some("commerce"))
        .await
        .unwrap();
    let t = TraceContext::new_root();
    // Same subject from two sources with different content => conflict, enterprise standard prevails.
    let mut ent = req(
        &f,
        "api-standards",
        "pagination",
        "# Pagination\nUse cursor pagination with a next_cursor field.\n",
        KnowledgeScope::Enterprise,
    );
    ent.mandatory = true;
    let ent = f.kb.ingest(&f.steward, ent, t).await.unwrap();
    approve(&f, &ent).await;
    f.kb.ingest(
        &f.steward,
        req(
            &f,
            "api-guides",
            "pagination",
            "# Pagination\nUse offset and limit parameters.\n",
            KnowledgeScope::Domain("commerce".into()),
        ),
        t,
    )
    .await
    .unwrap();
    let conflicts = f.kb.conflicts(&f.steward).await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert!(
        conflicts[0].resolution.contains(&ent.id),
        "{}",
        conflicts[0].resolution
    );
    let ctx = hctx(&f, &f.viewer, f.p1).await;
    let hits =
        f.kb.search(&ctx, &SearchQuery::new("pagination", SearchMode::Hybrid, 5), t)
            .await
            .unwrap();
    assert!(hits.iter().all(|h| h.conflict));
    assert_eq!(hits[0].source, "api-standards", "standards rank first");

    // Domain mandatory standard on the same subject does not override the enterprise one.
    f.kb.register_source(
        &f.steward,
        "commerce-standards",
        SourceKind::Standard,
        "Domain standards",
    )
    .await
    .unwrap();
    let mut dom = req(
        &f,
        "commerce-standards",
        "pagination",
        "# Pagination\nDomain variant: page numbers.\n",
        KnowledgeScope::Domain("commerce".into()),
    );
    dom.mandatory = true;
    let dom = f.kb.ingest(&f.steward, dom, t).await.unwrap();
    approve(&f, &dom).await;
    let std = f.kb.mandatory_standards(&ctx).await.unwrap();
    assert_eq!(std.len(), 1);
    assert_eq!(
        std[0].0, "api-standards/pagination@v1",
        "enterprise standard prevails over domain standard"
    );
    assert!(std[0].2.contains("cursor pagination"));

    // Freshness: past review-by date is flagged and excluded unless requested.
    let mut old = req(
        &f,
        "api-guides",
        "legacy-auth",
        "# Legacy auth\nUse basic auth with platypus tokens.\n",
        KnowledgeScope::Enterprise,
    );
    old.review_by = Some("2020-01-01T00:00:00Z".into());
    f.kb.ingest(&f.steward, old, t).await.unwrap();
    assert!(
        f.kb.search(&ctx, &SearchQuery::new("platypus", SearchMode::Lexical, 5), t)
            .await
            .unwrap()
            .is_empty()
    );
    let mut q = SearchQuery::new("platypus", SearchMode::Lexical, 5);
    q.include_stale = true;
    let stale = f.kb.search(&ctx, &q, t).await.unwrap();
    assert!(stale[0].stale);
}

#[tokio::test]
async fn fts_injection_cannot_break_queries_or_scope() {
    let f = fx().await;
    sources(&f).await;
    let t = TraceContext::new_root();
    f.kb.ingest(
        &f.steward,
        req(
            &f,
            "api-guides",
            "p2-secret-plan",
            "# Plan\nkangaroo roadmap\n",
            KnowledgeScope::Project(f.p2),
        ),
        t,
    )
    .await
    .unwrap();
    let ctx = hctx(&f, &f.viewer, f.p1).await;
    for q in [
        "kangaroo\" OR heading:*",
        "text:kangaroo",
        "kangaroo NEAR(roadmap)",
        "*",
        "\"\"\"",
        "kangaroo) OR (1=1",
    ] {
        let hits =
            f.kb.search(&ctx, &SearchQuery::new(q, SearchMode::Hybrid, 5), t)
                .await
                .unwrap();
        assert!(hits.is_empty(), "{q}");
    }
}

#[tokio::test]
async fn agent_retrieval_is_project_bound_and_ceiling_limited() {
    let f = fx().await;
    sources(&f).await;
    let t = TraceContext::new_root();
    let mut r = req(
        &f,
        "api-guides",
        "conf",
        "# Conf\nnarwhal details\n",
        KnowledgeScope::Enterprise,
    );
    r.classification = Classification::Restricted;
    f.kb.ingest(&f.steward, r, t).await.unwrap();
    f.kb.ingest(
        &f.steward,
        req(
            &f,
            "api-guides",
            "p2doc",
            "# P2\nnarwhal p2 notes\n",
            KnowledgeScope::Project(f.p2),
        ),
        t,
    )
    .await
    .unwrap();
    let scope = Scope::new(f.admin.tenant, f.p1);
    let agent = Principal::agent(
        eaeh_core::AgentInstanceId::new(),
        eaeh_core::AgentDefinitionId::new("a").unwrap(),
        "1.0.0".into(),
        eaeh_core::AutonomyLevel::ReadOnly,
        scope,
    );
    let ctx = f.kb.context_for_agent(&agent, scope, None).await.unwrap();
    assert_eq!(ctx.max_classification, Classification::Confidential);
    assert!(
        f.kb.search(&ctx, &SearchQuery::new("narwhal", SearchMode::Hybrid, 5), t)
            .await
            .unwrap()
            .is_empty()
    );
    // An agent cannot obtain a context for another project.
    assert_eq!(
        f.kb.context_for_agent(&agent, Scope::new(f.admin.tenant, f.p2), None)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
}

/// Records knowledge hook dispatches; optionally vetoes before retrieval.
struct KnowledgeHook {
    seen: std::sync::Mutex<Vec<(eaeh_extensibility::HookPoint, String)>>,
    veto: bool,
}

#[async_trait::async_trait]
impl eaeh_extensibility::Hook for KnowledgeHook {
    async fn on_event(
        &self,
        ctx: &eaeh_extensibility::HookContext,
    ) -> eaeh_core::Result<eaeh_extensibility::HookOutcome> {
        let q = match &ctx.payload {
            eaeh_extensibility::HookPayload::Knowledge { query } => query.clone(),
            _ => String::new(),
        };
        self.seen.lock().unwrap().push((ctx.point, q));
        Ok(
            if self.veto && ctx.point == eaeh_extensibility::HookPoint::BeforeKnowledgeRetrieval {
                eaeh_extensibility::HookOutcome::Veto("retrieval blocked by compliance hook".into())
            } else {
                eaeh_extensibility::HookOutcome::Proceed
            },
        )
    }
}

#[tokio::test]
async fn knowledge_hooks_observe_and_can_veto_retrieval() {
    use eaeh_extensibility::{FailurePolicy, HookPoint, HookRegistration};
    let f = fx().await;
    sources(&f).await;
    f.kb.ingest(
        &f.steward,
        req(
            &f,
            "api-guides",
            "resource-naming",
            NAMING,
            KnowledgeScope::Enterprise,
        ),
        TraceContext::new_root(),
    )
    .await
    .unwrap();
    let points = vec![
        HookPoint::BeforeKnowledgeRetrieval,
        HookPoint::AfterKnowledgeRetrieval,
    ];
    let service = |hook: Arc<KnowledgeHook>| {
        let hooks = Arc::new(HookRegistry::new());
        hooks
            .register(
                HookRegistration::new("kb-observer", points.clone(), FailurePolicy::Block),
                hook,
            )
            .unwrap();
        KnowledgeService::new(
            f.db.clone(),
            Arc::new(RbacAuthorizer),
            f.approvals.clone(),
            hooks,
            Arc::new(HashedNgramEmbedder::new(256).unwrap()),
            KnowledgeConfig::default(),
        )
    };
    let ctx = hctx(&f, &f.viewer, f.p1).await;
    let q = SearchQuery::new("plural nouns", SearchMode::Hybrid, 3);

    // Observing hook: both points fire, in order, with the query; results unaffected.
    let observer = Arc::new(KnowledgeHook {
        seen: Default::default(),
        veto: false,
    });
    let hits = service(observer.clone())
        .search(&ctx, &q, TraceContext::new_root())
        .await
        .unwrap();
    assert!(!hits.is_empty());
    let seen = observer.seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![
            (HookPoint::BeforeKnowledgeRetrieval, "plural nouns".to_string()),
            (HookPoint::AfterKnowledgeRetrieval, "plural nouns".to_string()),
        ]
    );

    // Vetoing hook: retrieval is blocked before any search or audit happens.
    let audits = || async {
        let n: (i64,) =
            eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM audit_log WHERE action = 'knowledge.search'")
                .fetch_one(f.db.pool())
                .await
                .unwrap();
        n.0
    };
    let before = audits().await;
    let vetoer = Arc::new(KnowledgeHook {
        seen: Default::default(),
        veto: true,
    });
    let err = service(vetoer.clone())
        .search(&ctx, &q, TraceContext::new_root())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::HookBlocked);
    assert_eq!(
        vetoer.seen.lock().unwrap().len(),
        1,
        "after-hook must not run when vetoed"
    );
    assert_eq!(
        audits().await,
        before,
        "a vetoed search must not be recorded as performed"
    );
}
