//! Retrieval-quality baseline (AC2-21): recall@1 and recall@3 of lexical, vector
//! (hashed n-gram, non-semantic) and hybrid retrieval on a fixed evaluation set.
//! Results are printed (`--nocapture`) and recorded in TEST-STRATEGY / the completion
//! report; thresholds guard against regressions, set just below measured values.

use eaeh_core::{Classification, PrincipalId, TraceContext};
use eaeh_extensibility::HookRegistry;
use eaeh_governance::authz::RbacAuthorizer;
use eaeh_governance::{ApprovalManager, IdentityService};
use eaeh_intelligence::HashedNgramEmbedder;
use eaeh_knowledge::*;
use eaeh_store::Db;
use std::sync::Arc;

const CORPUS: &[(&str, &str)] = &[
    (
        "error-format",
        "# Error responses\nErrors use application/problem+json with type, title, status and trace_id.",
    ),
    (
        "resource-naming",
        "# Resource naming\nCollections are plural nouns such as /orders; path segments use kebab-case.",
    ),
    (
        "pagination",
        "# Pagination\nList endpoints use cursor pagination with limit and next_cursor.",
    ),
    (
        "versioning",
        "# API versioning\nBreaking changes require a new major version in the URL path, for example /v2.",
    ),
    (
        "authentication",
        "# Authentication\nServices authenticate with OAuth2 client credentials; never use basic auth.",
    ),
    (
        "idempotency",
        "# Idempotency\nPOST requests that create resources accept an Idempotency-Key header to make retries safe.",
    ),
    (
        "rate-limits",
        "# Rate limiting\nClients receive 429 Too Many Requests with a Retry-After header when throttled.",
    ),
    (
        "logging",
        "# Logging\nLogs are structured JSON and must never contain secrets, tokens or personal data.",
    ),
    (
        "timeouts",
        "# Timeouts\nOutbound HTTP calls set connect and read timeouts and use exponential backoff with jitter.",
    ),
    (
        "database-migrations",
        "# Database migrations\nSchema changes are applied with forward-only, versioned migrations reviewed in pull requests.",
    ),
    (
        "dependency-policy",
        "# Dependency policy\nThird-party libraries require licence review; copyleft licences need architecture approval.",
    ),
    (
        "data-retention",
        "# Data retention\nPersonal data is deleted after 30 days unless a legal hold applies.",
    ),
];

const QUERIES: &[(&str, &str)] = &[
    ("problem json error body fields", "error-format"),
    ("plural nouns for collection paths", "resource-naming"),
    ("next_cursor paging of list results", "pagination"),
    ("how do we version breaking API changes", "versioning"),
    ("oauth client credentials", "authentication"),
    ("safe retries for create requests", "idempotency"),
    ("throttled clients 429 retry-after", "rate-limits"),
    ("may logs include tokens or personal data", "logging"),
    ("exponential backoff and jitter for outbound calls", "timeouts"),
    ("versioned forward-only schema migrations", "database-migrations"),
    ("licence review for third party libraries", "dependency-policy"),
    ("how long do we keep personal data", "data-retention"),
    // Morphological / sub-word variants (where the n-gram embedder can help).
    ("paginated listing cursors", "pagination"),
    ("idempotent POST key header", "idempotency"),
    ("migration of database schemas", "database-migrations"),
];

/// Pure paraphrases with (almost) no word overlap with the target document. These
/// measure semantic retrieval, which the built-in hashed n-gram embedder does not
/// provide; results are recorded, not asserted (limitation R2-01).
const PARAPHRASES: &[(&str, &str)] = &[
    ("how should failures be reported to callers", "error-format"),
    ("which login approach do machines use", "authentication"),
    ("what happens when someone calls us too often", "rate-limits"),
    ("how long until customer information is purged", "data-retention"),
    ("can we adopt GPL code", "dependency-policy"),
];

#[tokio::test]
async fn recall_baseline() {
    let d = tempfile::tempdir().unwrap();
    let db = Db::open(&d.path().join("eval.db")).await.unwrap();
    let authz = Arc::new(RbacAuthorizer);
    let identity = IdentityService::new(db.clone(), authz.clone());
    let (admin, project, _) = identity
        .bootstrap("acme", "eval", PrincipalId::new("admin").unwrap(), "A", 30)
        .await
        .unwrap();
    let kb = KnowledgeService::new(
        db.clone(),
        authz.clone(),
        ApprovalManager::new(db.clone(), authz),
        Arc::new(HookRegistry::new()),
        Arc::new(HashedNgramEmbedder::new(384).unwrap()),
        KnowledgeConfig::default(),
    );
    kb.register_source(&admin, "eval-guides", SourceKind::Reference, "evaluation corpus")
        .await
        .unwrap();
    for (subject, content) in CORPUS {
        kb.ingest(
            &admin,
            IngestRequest {
                source: "eval-guides".into(),
                subject: (*subject).into(),
                title: (*subject).into(),
                format: Format::Markdown,
                content: (*content).into(),
                scope: KnowledgeScope::Enterprise,
                classification: Classification::Internal,
                tags: vec![],
                mandatory: false,
                review_by: None,
                governance_project: project,
            },
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    }
    let ctx = kb.context_for_human(&admin, project).await.unwrap();
    let mut results = vec![];
    for mode in [SearchMode::Lexical, SearchMode::Vector, SearchMode::Hybrid] {
        let (mut r1, mut r3) = (0usize, 0usize);
        for (q, expected) in QUERIES {
            let hits = kb
                .search(&ctx, &SearchQuery::new(q, mode, 3), TraceContext::new_root())
                .await
                .unwrap();
            let pos = hits.iter().position(|h| h.subject == *expected);
            if pos == Some(0) {
                r1 += 1;
            }
            if pos.is_some() {
                r3 += 1;
            }
        }
        let n = QUERIES.len() as f64;
        let (rec1, rec3) = (r1 as f64 / n, r3 as f64 / n);
        println!(
            "RETRIEVAL-EVAL mode={:<7} recall@1={rec1:.3} recall@3={rec3:.3} (n={})",
            mode.as_str(),
            QUERIES.len()
        );
        results.push((mode, rec1, rec3));
    }
    for mode in [SearchMode::Lexical, SearchMode::Vector, SearchMode::Hybrid] {
        let mut r3 = 0usize;
        for (q, expected) in PARAPHRASES {
            let hits = kb
                .search(&ctx, &SearchQuery::new(q, mode, 3), TraceContext::new_root())
                .await
                .unwrap();
            if hits.iter().any(|h| h.subject == *expected) {
                r3 += 1;
            }
        }
        println!(
            "RETRIEVAL-EVAL paraphrase mode={:<7} recall@3={:.3} (n={}) [recorded, not asserted]",
            mode.as_str(),
            r3 as f64 / PARAPHRASES.len() as f64,
            PARAPHRASES.len()
        );
    }
    let get = |m: SearchMode| results.iter().find(|r| r.0 == m).map(|r| (r.1, r.2)).unwrap();
    let (_, lex3) = get(SearchMode::Lexical);
    let (hyb1, hyb3) = get(SearchMode::Hybrid);
    assert!(hyb3 >= lex3, "hybrid must not be worse than lexical at k=3");
    assert!(hyb3 >= 0.9, "hybrid recall@3 regressed: {hyb3}");
    assert!(hyb1 >= 0.7, "hybrid recall@1 regressed: {hyb1}");
}
