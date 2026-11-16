//! Markdown skills applied per layer (docs/implementation/stages/MARKDOWN-SKILLS-PLAN.md).
//!
//! `config/skills/architecture-diagrams.md` (task kinds `hld-design`, `lld-design`) is
//! registered from its Markdown file. While pending it is never applied; once a human
//! approves it, the engine adds its instructions to the HLD/LLD agent runs – and only
//! those – and records it in the artifacts' provenance. The model is the labelled test
//! double `ScriptedModelProvider`.

mod common;
use common::*;
use eaeh_core::{TraceContext, WorkflowDefinitionId};
use eaeh_governance::ApprovalDecision;
use eaeh_testkit::Script;
use eaeh_workflow::InstanceStatus;
use serde_json::json;

const MD: &str = "architecture-diagrams@1.0.0";

#[tokio::test]
async fn an_approved_markdown_skill_is_applied_at_its_layers_only() {
    let good = example("requirements.json");
    let read_brd = || Script::tool("knowledge.read", json!({"document": BRD}));
    let w = world(
        vec![
            read_brd(),
            Script::json(good.clone()),
            read_brd(),
            Script::json(good),
            Script::json(example("hld.json")),
            Script::json(example("lld.json")),
        ],
        "",
    )
    .await;
    // Registered from the Markdown file (same service as `eaeh skill sync` / the portal).
    let path = root().join("config/skills/architecture-diagrams.md");
    let def = eaeh_skills::SkillDefinition::from_file_text(&path, &std::fs::read_to_string(&path).unwrap())
        .unwrap();
    assert_eq!(def.reference(), MD);
    assert!(def.instructions.starts_with("# Architecture diagrams"));
    let rec =
        w.h.skills
            .register(&w.steward, &def, w.scope.project, TraceContext::new_root())
            .await
            .unwrap();

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
    // Requirements approved → HLD runs while the skill is still PENDING: not applied.
    assert_eq!(decide(&w, ApprovalDecision::Approve, "ok").await.1, "approve_hld");
    let marker = format!("APPROVED SKILL [{MD}]");
    let reqs = w.provider.requests();
    let hld_req = reqs
        .iter()
        .find(|r| {
            r.messages[0]
                .content
                .contains("APPROVED SKILL [hld-design@1.0.0]")
        })
        .expect("HLD request");
    assert!(
        !hld_req.messages[0].content.contains(&marker),
        "a pending skill is never applied"
    );
    let inst = w.h.engine.get(id).await.unwrap();
    let hld_meta =
        w.h.artifacts
            .meta(w.scope, inst.data.artifacts["hld"].id)
            .await
            .unwrap();
    assert!(!hld_meta.provenance.skills.iter().any(|s| s == MD));

    // A human approves the skill → the next layer (LLD) applies it, with its Markdown text.
    w.h.decide_approval(
        &w.approver,
        rec.approval_id.clone().unwrap().parse().unwrap(),
        ApprovalDecision::Approve,
        "diagram rules reviewed",
        Some(&rec.digest),
    )
    .await
    .unwrap();
    assert_eq!(decide(&w, ApprovalDecision::Approve, "ok").await.1, "approve_lld");
    let reqs = w.provider.requests();
    let lld_req = reqs
        .iter()
        .find(|r| {
            r.messages[0]
                .content
                .contains("APPROVED SKILL [lld-design@1.0.0]")
        })
        .expect("LLD request");
    let sys = &lld_req.messages[0].content;
    assert!(
        sys.contains(&marker),
        "approved layer skill is applied at the LLD layer"
    );
    assert!(
        sys.contains("Use the **component id**"),
        "the Markdown body is the instruction text"
    );
    // Never at layers it does not name (requirements analysis / review).
    for r in reqs.iter().filter(|r| {
        r.messages[0]
            .content
            .contains("APPROVED SKILL [requirements-analysis")
    }) {
        assert!(!r.messages[0].content.contains(&marker));
    }
    let inst = w.h.engine.get(id).await.unwrap();
    let lld_meta =
        w.h.artifacts
            .meta(w.scope, inst.data.artifacts["lld"].id)
            .await
            .unwrap();
    assert!(
        lld_meta.provenance.skills.iter().any(|s| s == MD),
        "{:?}",
        lld_meta.provenance.skills
    );
}

#[tokio::test]
async fn skill_files_load_as_json_and_markdown_and_duplicates_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    std::fs::copy(
        root().join("config/skills/hld-design.json"),
        d.join("hld-design.json"),
    )
    .unwrap();
    std::fs::copy(
        root().join("config/skills/architecture-diagrams.md"),
        d.join("architecture-diagrams.md"),
    )
    .unwrap();
    std::fs::write(d.join("README.md"), "# Skills\n\nDocumentation, not a skill.\n").unwrap();
    let dd = d.clone();
    let w = world_cfg(
        eaeh_runtime::ExecutionMode::DirectProvider,
        vec![],
        "",
        move |c| {
            c.definitions.skills_dir = dd;
        },
    )
    .await;
    let files = eaeh_runtime::project::repository_skills(&w.h).unwrap();
    let refs: Vec<String> = files.iter().map(|(_, s)| s.reference()).collect();
    assert_eq!(
        refs,
        vec!["architecture-diagrams@1.0.0", "hld-design@1.0.0"],
        "README.md is ignored"
    );
    // The design workflow's HLD/LLD layers pick up the Markdown skill as a layer skill.
    let layer = eaeh_runtime::project::layer_skills(&w.h, &["engineering.design".to_string()])
        .await
        .unwrap();
    assert_eq!(layer.len(), 1);
    assert_eq!(layer[0].reference, "architecture-diagrams@=1.0.0");
    assert!(layer[0].layers.contains("engineering.design/hld (hld-design)"));
    assert!(layer[0].layers.contains("engineering.design/lld (lld-design)"));
    // The same id and version twice is refused with both file names.
    let dup = std::fs::read_to_string(d.join("architecture-diagrams.md")).unwrap();
    std::fs::write(d.join("copy.md"), dup).unwrap();
    let e = eaeh_runtime::project::repository_skills(&w.h).unwrap_err();
    assert!(
        e.message.contains("defined twice") && e.message.contains("copy.md"),
        "{}",
        e.message
    );
}

/// Skill directories (SKILL-ONBOARDING-PLAN): automatic discovery of a copied directory,
/// nested references in the agent context (scripts never), approval and rejection through
/// the existing approval service, and a new version whenever any file changes.
#[tokio::test]
async fn skill_directories_are_discovered_versioned_on_change_and_only_approved_versions_apply() {
    let skills = tempfile::tempdir().unwrap();
    let sd = skills.path().to_path_buf();
    // An empty skills folder first; the directory is "copied in" later.
    let dd = sd.clone();
    let good = example("requirements.json");
    let read_brd = || Script::tool("knowledge.read", json!({"document": BRD}));
    let w = world_cfg(
        eaeh_runtime::ExecutionMode::DirectProvider,
        vec![
            read_brd(),
            Script::json(good.clone()),
            read_brd(),
            Script::json(good),
            Script::json(example("hld.json")),
            Script::json(example("lld.json")),
        ],
        "",
        move |c| c.definitions.skills_dir = dd,
    )
    .await;
    use eaeh_runtime::project::{SkillState, discover_skills, skill_state};
    assert!(discover_skills(&w.h, None).await.unwrap().is_empty());

    // Copy a skill directory with nested folders into the skills folder.
    let dir = sd.join("api-error-handling");
    copy_dir(&root().join("config/skills/api-error-handling"), &dir);
    let r = discover_skills(&w.h, None).await.unwrap();
    assert_eq!(
        (r.len(), r[0].action.as_str(), r[0].version.as_deref()),
        (1, "new", Some("1.0.0")),
        "{r:?}"
    );
    let rec =
        w.h.skills
            .get(
                &"api-error-handling".parse().unwrap(),
                &semver::Version::new(1, 0, 0),
            )
            .await
            .unwrap();
    assert_eq!(rec.registered_by, eaeh_skills::DISCOVERY_ACTOR);
    assert_eq!(rec.status, eaeh_skills::SkillStatus::PendingApproval);
    let pk = rec.definition.package.clone().unwrap();
    let paths: Vec<&str> = pk.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "SKILL.md",
            "examples/config.yaml",
            "examples/error-response.json",
            "references/error-catalog.md",
            "references/http/status-mapping.md",
            "scripts/validate_errors.py",
        ]
    );
    // Idempotent: unchanged content registers nothing.
    assert_eq!(discover_skills(&w.h, None).await.unwrap()[0].action, "unchanged");
    let (n,): (i64,) = eaeh_store::sqlx::query_as(
        "SELECT COUNT(*) FROM audit_log WHERE action = 'skill.register' AND resource_id = 'api-error-handling@1.0.0' AND actor_id = 'system:skill-discovery'",
    )
    .fetch_one(w.h.db.pool())
    .await
    .unwrap();
    assert_eq!(n, 1);
    // Pending ⇒ not available to agents.
    assert!(matches!(
        skill_state(&w.h, &w.approver, "api-error-handling@^1")
            .await
            .unwrap(),
        SkillState::PendingApproval { .. }
    ));

    // A human approves 1.0.0 (bound to the digest that covers every file).
    w.h.decide_approval(
        &w.approver,
        rec.approval_id.clone().unwrap().parse().unwrap(),
        ApprovalDecision::Approve,
        "reviewed",
        Some(&rec.digest),
    )
    .await
    .unwrap();
    // Design run: the LLD agent receives the SKILL.md body and the nested reference text, never
    // the script.
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
    decide(&w, ApprovalDecision::Approve, "ok").await;
    assert_eq!(decide(&w, ApprovalDecision::Approve, "ok").await.1, "approve_lld");
    let reqs = w.provider.requests();
    let lld = &reqs
        .iter()
        .find(|r| {
            r.messages[0]
                .content
                .contains("APPROVED SKILL [lld-design@1.0.0]")
        })
        .unwrap()
        .messages[0]
        .content;
    assert!(lld.contains("APPROVED SKILL [api-error-handling@1.0.0]"));
    assert!(
        lld.contains("REFERENCE FILE references/http/status-mapping.md"),
        "nested reference included"
    );
    assert!(
        lld.contains("REFERENCE FILE examples/config.yaml")
            && lld.contains("REFERENCE FILE examples/error-response.json")
    );
    assert!(
        lld.contains("scripts/validate_errors.py (code"),
        "scripts are listed only"
    );
    assert!(!lld.contains("def main"), "script content is never sent");
    for r in reqs.iter().filter(|r| {
        r.messages[0]
            .content
            .contains("APPROVED SKILL [hld-design@1.0.0]")
    }) {
        assert!(
            !r.messages[0].content.contains("api-error-handling"),
            "not applied at the HLD layer"
        );
    }

    // A nested file changes after approval ⇒ a new version 1.0.1 (revision 2), pending; 1.0.0
    // stays in force. Rejected with comments ⇒ history kept, 1.0.0 still in force.
    std::fs::write(
        dir.join("references/http/status-mapping.md"),
        "# HTTP status mapping\n\n| Invalid input | 422 |\n",
    )
    .unwrap();
    let r = discover_skills(&w.h, None).await.unwrap();
    assert_eq!(
        (r[0].action.as_str(), r[0].version.as_deref()),
        ("changed", Some("1.0.1")),
        "{r:?}"
    );
    let v101 =
        w.h.skills
            .get(
                &"api-error-handling".parse().unwrap(),
                &semver::Version::new(1, 0, 1),
            )
            .await
            .unwrap();
    assert_eq!(v101.definition.package.as_ref().unwrap().revision, 2);
    assert_eq!(
        v101.definition.package.as_ref().unwrap().source_version,
        semver::Version::new(1, 0, 0)
    );
    assert_ne!(v101.digest, rec.digest);
    assert_eq!(
        skill_state(&w.h, &w.approver, "api-error-handling@^1")
            .await
            .unwrap(),
        SkillState::Approved {
            version: "1.0.0".into()
        }
    );
    let a101: eaeh_core::ApprovalId = v101.approval_id.clone().unwrap().parse().unwrap();
    w.h.decide_approval(
        &w.approver,
        a101,
        ApprovalDecision::Reject,
        "422 contradicts the API standard",
        Some(&v101.digest),
    )
    .await
    .unwrap();
    let ap = w.h.approvals.get(a101).await.unwrap();
    assert_eq!(ap.status, eaeh_governance::ApprovalStatus::Rejected);
    assert_eq!(ap.decision.unwrap().comments, "422 contradicts the API standard");
    assert_eq!(
        w.h.skills
            .get(
                &"api-error-handling".parse().unwrap(),
                &semver::Version::new(1, 0, 1)
            )
            .await
            .unwrap()
            .status,
        eaeh_skills::SkillStatus::Rejected
    );
    assert_eq!(
        skill_state(&w.h, &w.approver, "api-error-handling@^1")
            .await
            .unwrap(),
        SkillState::Approved {
            version: "1.0.0".into()
        }
    );

    // Fixed again ⇒ 1.0.2 (revision 3); approved ⇒ it is the version in force.
    std::fs::write(
        dir.join("references/http/status-mapping.md"),
        "# HTTP status mapping\n\n| Invalid input | 400 | `VALIDATION_FAILED` |\n",
    )
    .unwrap();
    let r = discover_skills(&w.h, None).await.unwrap();
    assert_eq!(r[0].version.as_deref(), Some("1.0.2"), "{r:?}");
    let v102 =
        w.h.skills
            .get(
                &"api-error-handling".parse().unwrap(),
                &semver::Version::new(1, 0, 2),
            )
            .await
            .unwrap();
    assert_eq!(v102.definition.package.as_ref().unwrap().revision, 3);
    w.h.decide_approval(
        &w.approver,
        v102.approval_id.clone().unwrap().parse().unwrap(),
        ApprovalDecision::Approve,
        "ok",
        Some(&v102.digest),
    )
    .await
    .unwrap();
    assert_eq!(
        skill_state(&w.h, &w.approver, "api-error-handling@^1")
            .await
            .unwrap(),
        SkillState::Approved {
            version: "1.0.2".into()
        }
    );
    // A stored approved version cannot be altered: its files verify against the tree hash.
    eaeh_skills::package::validate(v102.definition.package.as_ref().unwrap()).unwrap();

    // Invalid directories are reported, not registered.
    std::fs::create_dir_all(sd.join("broken")).unwrap();
    std::fs::write(sd.join("broken/SKILL.md"), "no front matter").unwrap();
    let r = discover_skills(&w.h, None).await.unwrap();
    let b = r.iter().find(|o| o.dir == "broken").unwrap();
    assert_eq!(b.action, "invalid");
    assert!(b.detail.contains("+++"), "{}", b.detail);
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}
