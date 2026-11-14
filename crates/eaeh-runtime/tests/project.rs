//! Developer-experience runtime: project manifest, required-skill discovery and sync,
//! readiness (missing provider, pending/missing skills, roles, BRD visibility), activity
//! planning and isolation.

use eaeh_core::{
    Classification, ErrorKind, Principal, PrincipalId, PrincipalKind, ProjectAccess, ProjectId, Role,
    TraceContext,
};
use eaeh_governance::{ApprovalDecision, NewPrincipal};
use eaeh_knowledge::{Format, IngestRequest, KnowledgeScope, SourceKind};
use eaeh_runtime::project::{self as proj, CheckStatus, ProjectManifest, SkillState};
use eaeh_runtime::{Harness, HarnessConfig};
use eaeh_testkit::{Script, ScriptedModelProvider};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct W {
    _d: tempfile::TempDir,
    root: PathBuf,
    h: Arc<Harness>,
    admin: Principal,
    dev: Principal,
    approver: Principal,
    m: ProjectManifest,
}

async fn world(with_model: bool) -> W {
    let d = tempfile::tempdir().unwrap();
    let mut cfg = HarnessConfig::load(&repo().join("config/eaeh.toml")).unwrap();
    cfg.storage.data_dir = d.path().join("data");
    cfg.isolation.container_enabled = false;
    // These readiness tests exercise provider handling, i.e. direct_provider mode.
    cfg.execution.mode = eaeh_agents::ExecutionMode::DirectProvider;
    let mut b = Harness::builder(cfg);
    if with_model {
        b = b.model_provider(Arc::new(ScriptedModelProvider::new(
            "scripted",
            "scripted-model",
            vec![Script::json(serde_json::json!({}))],
        )));
    }
    let h = Arc::new(b.build().await.unwrap());
    let (admin, _, _) = h
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
    let project = h
        .identity
        .create_project(&admin, admin.tenant, "springboot")
        .await
        .unwrap();
    let mk = |id: &str, r: Role| NewPrincipal {
        id: PrincipalId::new(id).unwrap(),
        display_name: id.into(),
        kind: PrincipalKind::Human,
        roles: [r].into(),
        projects: ProjectAccess::Only([project].into()),
    };
    let dev = h
        .identity
        .create_principal(&admin, mk("dev", Role::Operator))
        .await
        .unwrap();
    let approver = h
        .identity
        .create_principal(&admin, mk("approver", Role::Approver))
        .await
        .unwrap();
    h.identity
        .create_principal(&admin, mk("steward", Role::KnowledgeSteward))
        .await
        .unwrap();
    h.knowledge
        .register_source(&admin, "sb-brd", SourceKind::Reference, "BRDs")
        .await
        .unwrap();
    let doc = h
        .knowledge
        .ingest(
            &admin,
            IngestRequest {
                source: "sb-brd".into(),
                subject: "brd".into(),
                title: "BRD".into(),
                format: Format::Markdown,
                content: std::fs::read_to_string(
                    repo().join("examples/simple-springboot-api/BRD-Simple-Spring-Boot-API.md"),
                )
                .unwrap(),
                scope: KnowledgeScope::Project(project),
                classification: Classification::Confidential,
                tags: vec!["brd".into()],
                mandatory: false,
                review_by: None,
                governance_project: project,
            },
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    let root = std::fs::canonicalize(d.path()).unwrap();
    let m = ProjectManifest {
        project: proj::ProjectSection {
            id: project,
            tenant: admin.tenant,
            name: "springboot".into(),
            description: String::new(),
        },
        workspace: proj::WorkspaceSection {
            root: root.display().to_string(),
            git_remote: None,
            git_head: None,
        },
        stack: Default::default(),
        brd: Some(proj::BrdSection {
            path: "BRD.md".into(),
            citation: doc.citation_base(),
        }),
        knowledge: vec![],
        workflows: vec!["requirements.analysis".into(), "engineering.design".into()],
        roles: proj::RolesSection {
            developer: "dev".into(),
            steward: "steward".into(),
            approvers: vec!["approver".into()],
        },
        provider: proj::ProviderSection {
            mode: "replay".into(),
        },
        execution: None,
        delivery: None,
    };
    m.save(&root).unwrap();
    W {
        _d: d,
        root,
        h,
        admin,
        dev,
        approver,
        m,
    }
}

fn blocking(r: &proj::Readiness) -> Vec<String> {
    r.checks
        .iter()
        .filter(|c| c.status == CheckStatus::Blocking)
        .map(|c| {
            format!(
                "[{}] {} → {}",
                c.area,
                c.message,
                c.fix.clone().unwrap_or_default()
            )
        })
        .collect()
}

#[tokio::test]
async fn manifest_roundtrip_and_discovery_from_subdirectories() {
    let w = world(false).await;
    let sub = w.root.join("src/main/java");
    std::fs::create_dir_all(&sub).unwrap();
    assert_eq!(proj::find_root(&sub).unwrap(), w.root);
    assert_eq!(ProjectManifest::load(&w.root).unwrap(), w.m);
    let text = std::fs::read_to_string(w.root.join(".eaeh/project.toml")).unwrap();
    assert!(text.contains("contains no secrets") && !text.contains("eaeh_"));
    assert!(proj::find_root(Path::new("/")).is_none());
}

#[tokio::test]
async fn required_skills_are_discovered_and_synced_idempotently() {
    let w = world(false).await;
    let req = proj::required_skills(&w.h, &w.m.workflows).await.unwrap();
    let refs: Vec<&str> = req.iter().map(|r| r.reference.as_str()).collect();
    assert_eq!(
        refs,
        vec!["hld-design@^1", "lld-design@^1", "requirements-analysis@^1"]
    );
    assert!(req[2].agents.contains("business-analyst") && req[2].agents.contains("requirements-engineer"));
    let refs: Vec<String> = req.iter().map(|r| r.reference.clone()).collect();
    match proj::skill_state(&w.h, &w.admin, "hld-design@^1").await.unwrap() {
        SkillState::NotRegistered {
            definition_file: Some(f),
        } => assert!(f.ends_with("config/skills/hld-design.json")),
        other => panic!("{other:?}"),
    }
    let first = proj::sync_skills(&w.h, &w.admin, w.m.project.id, &refs)
        .await
        .unwrap();
    assert!(
        first
            .iter()
            .all(|o| o.action == "registered" && o.approval_id.is_some()),
        "{first:?}"
    );
    let again = proj::sync_skills(&w.h, &w.admin, w.m.project.id, &refs)
        .await
        .unwrap();
    assert!(
        again.iter().all(|o| o.action == "already_registered"),
        "{again:?}"
    );
    // A developer (operator) may not register skills.
    let e = proj::sync_skills(&w.h, &w.dev, w.m.project.id, &["no-such@^1".to_string()])
        .await
        .unwrap();
    assert_eq!(e[0].action, "missing_definition");
    let e =
        w.h.skills
            .register(
                &w.dev,
                &proj::repository_skills(&w.h).unwrap()[0].1,
                w.m.project.id,
                TraceContext::new_root(),
            )
            .await
            .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert!(
        e.message
            .contains("roles that grant it: admin, knowledge_steward"),
        "{}",
        e.message
    );
}

#[tokio::test]
async fn readiness_reports_missing_provider_and_unapproved_skills_with_fixes() {
    let w = world(false).await;
    let r = proj::readiness(&w.h, &w.dev, &w.m, &w.root, None).await.unwrap();
    assert!(!r.ready);
    let b = blocking(&r).join("\n");
    assert!(
        b.contains("[provider] mode direct_provider: no model provider is registered")
            && b.contains("[providers.replay]")
            && b.contains("external_ai"),
        "{b}"
    );
    assert!(
        b.contains("[skills] requirements-analysis@^1 is not registered") && b.contains("eaeh skill sync"),
        "{b}"
    );
    assert!(
        r.checks
            .iter()
            .any(|c| c.area == "knowledge" && c.message.contains("readable by the developer (20 sections)"))
    );
    assert!(r.checks.iter().any(|c| c.area == "knowledge"
        && c.status == CheckStatus::Warning
        && c.message.contains("no approved mandatory")));

    // Registered but not approved: blocking with the approver action.
    let refs: Vec<String> = proj::required_skills(&w.h, &w.m.workflows)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.reference)
        .collect();
    proj::sync_skills(&w.h, &w.admin, w.m.project.id, &refs)
        .await
        .unwrap();
    let r = proj::readiness(
        &w.h,
        &w.dev,
        &w.m,
        &w.root,
        Some(&["requirements.analysis".to_string()]),
    )
    .await
    .unwrap();
    let b = blocking(&r).join("\n");
    assert!(
        b.contains("requirements-analysis@^1 v1.0.0 awaits human approval")
            && b.contains("eaeh approval review --as"),
        "{b}"
    );
    assert!(
        !b.contains("hld-design"),
        "only the selected workflow's skills are checked: {b}"
    );
}

#[tokio::test]
async fn readiness_is_ready_after_human_approvals_with_a_model() {
    let w = world(true).await;
    let refs: Vec<String> = proj::required_skills(&w.h, &w.m.workflows)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.reference)
        .collect();
    let synced = proj::sync_skills(&w.h, &w.admin, w.m.project.id, &refs)
        .await
        .unwrap();
    for o in synced {
        let aid = o.approval_id.unwrap().parse().unwrap();
        let st = proj::skill_state(&w.h, &w.admin, &o.reference).await.unwrap();
        let SkillState::PendingApproval { digest, .. } = st else {
            panic!()
        };
        // The developer cannot approve; the approver can.
        assert!(
            w.h.decide_approval(&w.dev, aid, ApprovalDecision::Approve, "", Some(&digest))
                .await
                .is_err()
        );
        w.h.decide_approval(&w.approver, aid, ApprovalDecision::Approve, "ok", Some(&digest))
            .await
            .unwrap();
    }
    let r = proj::readiness(&w.h, &w.dev, &w.m, &w.root, None).await.unwrap();
    assert!(r.ready, "{:#?}", blocking(&r));
    // Activity planning resolves the BRD input and the approvals; Phase 4 activities are refused.
    let plan = proj::plan_activity(&w.h, &w.dev, &w.m, &w.root, "requirements")
        .await
        .unwrap();
    assert_eq!(plan.workflow, "requirements.analysis");
    assert_eq!(plan.input["brd"], w.m.brd.as_ref().unwrap().citation);
    assert_eq!(plan.approvals.len(), 1);
    assert!(
        plan.stages
            .iter()
            .any(|s| s.agent.as_deref() == Some("business-analyst@1.0.0")
                && s.skills == vec!["requirements-analysis@^1"])
    );
    // Planning as an identity that cannot start workflows is blocked up front.
    let plan = proj::plan_activity(&w.h, &w.approver, &w.m, &w.root, "requirements")
        .await
        .unwrap();
    assert!(
        !plan.readiness.ready
            && blocking(&plan.readiness)
                .iter()
                .any(|b| b.contains("cannot start workflows"))
    );
    // plan / implement / test select the governed delivery activity, which needs an
    // approved design and (build verification required) a build command and isolation.
    for a in ["plan", "implement", "test"] {
        let p = proj::plan_activity(&w.h, &w.dev, &w.m, &w.root, a).await.unwrap();
        assert_eq!(p.workflow, "engineering.delivery");
        assert!(!p.readiness.ready);
        let b = blocking(&p.readiness).join("\n");
        assert!(
            b.contains("no approved design") && b.contains("no build/test command"),
            "{b}"
        );
    }
    let e = proj::plan_activity(&w.h, &w.dev, &w.m, &w.root, "deploy")
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotSupported);
    assert!(e.message.contains("Phase 7"), "{}", e.message);
    let e = proj::plan_activity(&w.h, &w.dev, &w.m, &w.root, "paint")
        .await
        .unwrap_err();
    assert!(e.message.contains("available: requirements"), "{}", e.message);
}

#[tokio::test]
async fn readiness_enforces_responsibilities_and_isolation() {
    let w = world(false).await;
    // Developer listed as approver: separation of duties.
    let mut m = w.m.clone();
    m.roles.approvers = vec!["dev".into()];
    let b = blocking(&proj::readiness(&w.h, &w.dev, &m, &w.root, None).await.unwrap()).join("\n");
    assert!(
        b.contains("approver: principal 'dev' lacks role approver") || b.contains("also the developer"),
        "{b}"
    );
    assert!(b.contains("no approver can decide"), "{b}");

    // Another project of the same tenant: its members cannot read this project's BRD, and a
    // developer of this project gets no access to the other project.
    let other: ProjectId =
        w.h.identity
            .create_project(&w.admin, w.admin.tenant, "other")
            .await
            .unwrap();
    let outsider =
        w.h.identity
            .create_principal(
                &w.admin,
                NewPrincipal {
                    id: PrincipalId::new("outsider").unwrap(),
                    display_name: "o".into(),
                    kind: PrincipalKind::Human,
                    roles: [Role::Operator].into(),
                    projects: ProjectAccess::Only([other].into()),
                },
            )
            .await
            .unwrap();
    let r = proj::readiness(&w.h, &outsider, &w.m, &w.root, None)
        .await
        .unwrap();
    assert!(
        blocking(&r)
            .iter()
            .any(|b| b.contains("[identity] 'outsider' has no access to this project"))
    );
    let ctx = w.h.knowledge.context_for_human(&outsider, other).await.unwrap();
    assert!(
        w.h.knowledge
            .read_document(
                &ctx,
                &w.m.brd.as_ref().unwrap().citation,
                TraceContext::new_root()
            )
            .await
            .is_err()
    );
    // Granting access is explicit, audited and idempotent; roles are unchanged.
    assert!(
        w.h.identity
            .grant_project_access(&w.admin, &outsider.id, w.m.project.id)
            .await
            .unwrap()
    );
    assert!(
        !w.h.identity
            .grant_project_access(&w.admin, &outsider.id, w.m.project.id)
            .await
            .unwrap()
    );
    let (p, _) = w.h.identity.get_principal(&outsider.id).await.unwrap();
    assert!(p.can_access(&w.m.scope()) && !p.has_role(Role::Approver));
    // Only principal.manage may grant (a developer cannot grant themselves access).
    assert_eq!(
        w.h.identity
            .grant_project_access(&w.dev, &w.dev.id, other)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );

    // A manifest of a project in another tenant is reported, not trusted.
    let mut foreign = w.m.clone();
    foreign.project.tenant = eaeh_core::TenantId::new();
    let r = proj::readiness(&w.h, &w.dev, &foreign, &w.root, None)
        .await
        .unwrap();
    assert!(!r.ready && blocking(&r)[0].contains("is not registered in your tenant"));
}
