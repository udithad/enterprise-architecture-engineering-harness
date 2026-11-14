//! Skills registry: validation, approval lifecycle, dependencies, binding rules and
//! deterministic selection (AC2-12, AC2-13, AC2-14).

use eaeh_core::{
    ApprovalId, AutonomyLevel, ErrorKind, Principal, PrincipalId, PrincipalKind, ProjectAccess, ProjectId,
    RiskClass, Role, SkillId, ToolId, TraceContext,
};
use eaeh_governance::authz::RbacAuthorizer;
use eaeh_governance::{ApprovalDecision, ApprovalManager, IdentityService, NewPrincipal};
use eaeh_intelligence::TrustLevel;
use eaeh_skills::*;
use eaeh_store::Db;
use eaeh_tools::ToolRegistry;
use eaeh_tools::builtin::{ExecRun, FsList, FsRead, FsWrite, default_exec_limits};
use serde_json::json;
use std::sync::Arc;

struct Fx {
    _d: tempfile::TempDir,
    reg: SkillRegistry,
    approvals: ApprovalManager,
    steward: Principal,
    approver: Principal,
    project: ProjectId,
}

async fn fx() -> Fx {
    let d = tempfile::tempdir().unwrap();
    let db = Db::open(&d.path().join("s.db")).await.unwrap();
    let authz = Arc::new(RbacAuthorizer);
    let identity = IdentityService::new(db.clone(), authz.clone());
    let (admin, project, _) = identity
        .bootstrap("acme", "platform", PrincipalId::new("admin").unwrap(), "A", 30)
        .await
        .unwrap();
    let mk = |id: &str, r: Role| NewPrincipal {
        id: PrincipalId::new(id).unwrap(),
        display_name: id.into(),
        kind: PrincipalKind::Human,
        roles: [r].into(),
        projects: ProjectAccess::All,
    };
    let steward = identity
        .create_principal(&admin, mk("sam", Role::KnowledgeSteward))
        .await
        .unwrap();
    let approver = identity
        .create_principal(&admin, mk("arthur", Role::Approver))
        .await
        .unwrap();
    let tools = Arc::new(ToolRegistry::new());
    tools.register(Arc::new(FsRead::default()), "core").unwrap();
    tools.register(Arc::new(FsList::default()), "core").unwrap();
    tools.register(Arc::new(FsWrite::default()), "core").unwrap();
    tools
        .register(Arc::new(ExecRun::new(vec![], default_exec_limits())), "core")
        .unwrap();
    let approvals = ApprovalManager::new(db.clone(), authz.clone());
    Fx {
        _d: d,
        reg: SkillRegistry::new(db, authz, approvals.clone(), tools),
        approvals,
        steward,
        approver,
        project,
    }
}

fn skill(id: &str, version: &str) -> SkillDefinition {
    serde_json::from_value(json!({
        "id": id, "version": version,
        "description": format!("{id} skill"),
        "capabilities": ["review"],
        "task_kinds": [],
        "instructions": format!("Apply the {id} checklist."),
        "required_tools": ["fs.read"],
        "max_risk": "read_only"
    }))
    .unwrap()
}

async fn register_and_approve(f: &Fx, def: &SkillDefinition) -> SkillRecord {
    let rec = f
        .reg
        .register(&f.steward, def, f.project, TraceContext::new_root())
        .await
        .unwrap();
    let aid: ApprovalId = rec.approval_id.clone().unwrap().parse().unwrap();
    f.approvals
        .decide(
            &f.approver,
            aid,
            ApprovalDecision::Approve,
            "ok",
            Some(&rec.digest),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    f.reg
        .apply_approval(aid, TraceContext::new_root())
        .await
        .unwrap()
        .unwrap()
}

fn tools(ids: &[&str]) -> Vec<ToolId> {
    ids.iter().map(|t| ToolId::new(*t).unwrap()).collect()
}

#[tokio::test]
async fn skills_require_human_approval_bound_to_digest() {
    let f = fx().await;
    let def = skill("api-review", "1.0.0");
    let rec = f
        .reg
        .register(&f.steward, &def, f.project, TraceContext::new_root())
        .await
        .unwrap();
    assert_eq!(rec.status, SkillStatus::PendingApproval);
    let allowed = tools(&["fs.read"]);
    let explicit = vec!["api-review".to_string()];
    let b = Binding {
        explicit: &explicit,
        allowed_tools: &allowed,
        autonomy: AutonomyLevel::ReadOnly,
        task_kind: "review",
    };
    assert_eq!(
        f.reg.resolve(&b).await.unwrap_err().kind,
        ErrorKind::NotFound,
        "pending skills are unusable"
    );
    let aid: ApprovalId = rec.approval_id.clone().unwrap().parse().unwrap();
    let t = TraceContext::new_root();
    assert_eq!(
        f.approvals
            .decide(
                &f.steward,
                aid,
                ApprovalDecision::Approve,
                "",
                Some(&rec.digest),
                t
            )
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
    assert_eq!(
        f.approvals
            .decide(&f.approver, aid, ApprovalDecision::Approve, "", Some("bad"), t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    f.approvals
        .decide(
            &f.approver,
            aid,
            ApprovalDecision::Approve,
            "",
            Some(&rec.digest),
            t,
        )
        .await
        .unwrap();
    assert_eq!(
        f.reg.apply_approval(aid, t).await.unwrap().unwrap().status,
        SkillStatus::Approved
    );
    let res = f.reg.resolve(&b).await.unwrap();
    assert_eq!(res.references(), vec!["api-review@1.0.0"]);
    let items = res.instruction_items();
    assert_eq!(items[0].trust(), TrustLevel::Skill);
    // Idempotent re-registration; changed content under the same version conflicts.
    f.reg.register(&f.steward, &def, f.project, t).await.unwrap();
    let mut changed = def.clone();
    changed.instructions = "different".into();
    assert_eq!(
        f.reg
            .register(&f.steward, &changed, f.project, t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
}

#[tokio::test]
async fn rejected_skills_never_become_usable() {
    let f = fx().await;
    let rec = f
        .reg
        .register(
            &f.steward,
            &skill("x", "1.0.0"),
            f.project,
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    let aid: ApprovalId = rec.approval_id.clone().unwrap().parse().unwrap();
    f.approvals
        .decide(
            &f.approver,
            aid,
            ApprovalDecision::Reject,
            "no",
            Some(&rec.digest),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
    assert_eq!(
        f.reg
            .apply_approval(aid, TraceContext::new_root())
            .await
            .unwrap()
            .unwrap()
            .status,
        SkillStatus::Rejected
    );
}

#[tokio::test]
async fn registration_validation() {
    let f = fx().await;
    let t = TraceContext::new_root();
    let mut missing_tool = skill("a", "1.0.0");
    missing_tool.required_tools = tools(&["shell.anything"]);
    assert_eq!(
        f.reg
            .register(&f.steward, &missing_tool, f.project, t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::NotFound
    );
    let mut too_risky = skill("b", "1.0.0");
    too_risky.required_tools = tools(&["exec.run"]);
    assert_eq!(
        f.reg
            .register(&f.steward, &too_risky, f.project, t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::InvalidInput
    );
    let mut placeholder = skill("c", "1.0.0");
    placeholder.instructions = "Hello {{name}}".into();
    assert!(
        f.reg
            .register(&f.steward, &placeholder, f.project, t)
            .await
            .is_err()
    );
    let mut dep = skill("d", "1.0.0");
    dep.dependencies = vec![SkillDependency {
        id: SkillId::new("nope").unwrap(),
        version: semver::VersionReq::parse("^1").unwrap(),
    }];
    assert_eq!(
        f.reg
            .register(&f.steward, &dep, f.project, t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::InvalidInput
    );
    let mut api = skill("e", "1.0.0");
    api.eaeh_api = semver::VersionReq::parse("^2").unwrap();
    assert!(f.reg.register(&f.steward, &api, f.project, t).await.is_err());
    // Viewers/operators cannot register skills.
    let op = Principal::human(
        PrincipalId::new("op").unwrap(),
        f.steward.tenant,
        [Role::Operator].into(),
        ProjectAccess::All,
    );
    assert_eq!(
        f.reg
            .register(&op, &skill("f", "1.0.0"), f.project, t)
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
}

#[tokio::test]
async fn dependency_cycles_are_rejected() {
    let f = fx().await;
    register_and_approve(&f, &skill("a", "1.0.0")).await;
    let mut b = skill("b", "1.0.0");
    b.dependencies = vec![SkillDependency {
        id: SkillId::new("a").unwrap(),
        version: semver::VersionReq::parse("^1").unwrap(),
    }];
    register_and_approve(&f, &b).await;
    let mut a2 = skill("a", "1.1.0");
    a2.dependencies = vec![SkillDependency {
        id: SkillId::new("b").unwrap(),
        version: semver::VersionReq::parse("^1").unwrap(),
    }];
    let e = f
        .reg
        .register(&f.steward, &a2, f.project, TraceContext::new_root())
        .await
        .unwrap_err();
    assert!(e.message.contains("cycle"), "{e}");
}

#[tokio::test]
async fn skills_never_extend_agent_permissions() {
    let f = fx().await;
    let mut builder = skill("build-check", "1.0.0");
    builder.required_tools = tools(&["exec.run"]);
    builder.max_risk = RiskClass::BuildTest;
    builder.task_kinds = vec!["review".into()];
    register_and_approve(&f, &builder).await;
    let mut reviewer = skill("api-review", "1.0.0");
    reviewer.task_kinds = vec!["review".into()];
    register_and_approve(&f, &reviewer).await;

    let allowed = tools(&["fs.read", "fs.list"]);
    let explicit = vec!["build-check@^1".to_string()];
    let b = Binding {
        explicit: &explicit,
        allowed_tools: &allowed,
        autonomy: AutonomyLevel::ReadOnly,
        task_kind: "review",
    };
    assert_eq!(
        f.reg.resolve(&b).await.unwrap_err().kind,
        ErrorKind::Forbidden,
        "explicit incompatible skill fails the run"
    );

    let none: Vec<String> = vec![];
    let b = Binding {
        explicit: &none,
        allowed_tools: &allowed,
        autonomy: AutonomyLevel::ReadOnly,
        task_kind: "review",
    };
    let res = f.reg.resolve(&b).await.unwrap();
    assert_eq!(
        res.references(),
        vec!["api-review@1.0.0"],
        "dynamically matched incompatible skill is skipped"
    );
    assert_eq!(res.skipped.len(), 1);
    assert!(res.skipped[0].1.contains("exec.run"));

    // Even with the tool allowed, the autonomy ceiling must cover the skill's risk.
    let all = tools(&["fs.read", "exec.run"]);
    let b = Binding {
        explicit: &explicit,
        allowed_tools: &all,
        autonomy: AutonomyLevel::WorkspaceModify,
        task_kind: "x",
    };
    assert_eq!(f.reg.resolve(&b).await.unwrap_err().kind, ErrorKind::Forbidden);
    let b = Binding {
        explicit: &explicit,
        allowed_tools: &all,
        autonomy: AutonomyLevel::ExecuteAuthorized,
        task_kind: "x",
    };
    assert_eq!(
        f.reg.resolve(&b).await.unwrap().references(),
        vec!["build-check@1.0.0"]
    );
}

#[tokio::test]
async fn selection_is_deterministic_with_dependencies_first_and_lifecycle() {
    let f = fx().await;
    register_and_approve(&f, &skill("base", "1.0.0")).await;
    let mut v1 = skill("api-review", "1.0.0");
    v1.task_kinds = vec!["review".into()];
    v1.dependencies = vec![SkillDependency {
        id: SkillId::new("base").unwrap(),
        version: semver::VersionReq::parse("^1").unwrap(),
    }];
    register_and_approve(&f, &v1).await;
    let mut v2 = v1.clone();
    v2.version = semver::Version::new(1, 2, 0);
    register_and_approve(&f, &v2).await;
    let allowed = tools(&["fs.read"]);
    let none: Vec<String> = vec![];
    let b = Binding {
        explicit: &none,
        allowed_tools: &allowed,
        autonomy: AutonomyLevel::ReadOnly,
        task_kind: "review",
    };
    let first = f.reg.resolve(&b).await.unwrap();
    assert_eq!(first.references(), vec!["base@1.0.0", "api-review@1.2.0"]);
    assert_eq!(first.skills[0].selected_by, "dependency");
    for _ in 0..5 {
        assert_eq!(f.reg.resolve(&b).await.unwrap(), first);
    }
    // Deprecated stays usable (approved preferred); retired is not.
    let v12 = semver::Version::new(1, 2, 0);
    f.reg
        .set_status(
            &f.steward,
            &SkillId::new("api-review").unwrap(),
            &v12,
            SkillStatus::Deprecated,
        )
        .await
        .unwrap();
    let explicit = vec!["api-review".to_string()];
    let b2 = Binding {
        explicit: &explicit,
        allowed_tools: &allowed,
        autonomy: AutonomyLevel::ReadOnly,
        task_kind: "none",
    };
    assert_eq!(
        f.reg.resolve(&b2).await.unwrap().references(),
        vec!["base@1.0.0", "api-review@1.0.0"]
    );
    f.reg
        .set_status(
            &f.steward,
            &SkillId::new("base").unwrap(),
            &semver::Version::new(1, 0, 0),
            SkillStatus::Retired,
        )
        .await
        .unwrap();
    assert_eq!(
        f.reg.resolve(&b2).await.unwrap_err().kind,
        ErrorKind::NotFound,
        "retired dependency blocks use"
    );
}
