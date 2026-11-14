//! Tool gateway security & governance tests (AC-09, AC-10, AC-14, AC-18 partial, AC-25 partial).

use async_trait::async_trait;
use eaeh_core::{
    AgentDefinitionId, AgentInstanceId, AutonomyLevel, ErrorKind, Principal, PrincipalId, ProjectAccess,
    ProjectId, Redactor, Result, RiskClass, Role, Scope, SecretString, TenantId, ToolId, TraceContext,
};
use eaeh_extensibility::interceptor::RedactionInterceptor;
use eaeh_extensibility::{
    FailurePolicy, Hook, HookContext, HookOutcome, HookPoint, HookRegistration, HookRegistry,
};
use eaeh_governance::authz::RbacAuthorizer;
use eaeh_governance::policy::RuleEngine;
use eaeh_governance::{
    ApprovalDecision, ApprovalManager, ApprovalStatus, BudgetLedger, BudgetLimits, BudgetResource,
};
use eaeh_isolation::{ProviderRegistry, RestrictedProcessProvider};
use eaeh_store::Db;
use eaeh_tools::builtin::{ExecRun, FsRead, FsWrite, default_exec_limits};
use eaeh_tools::{Tool, ToolContext, ToolDescriptor, ToolGateway, ToolRegistry, ToolRequest};
use eaeh_workspace::{Workspace, WorkspaceManager};
use serde_json::{Value, json};
use std::sync::Arc;

struct Fx {
    _dir: tempfile::TempDir,
    db: Db,
    gw: ToolGateway,
    hooks: Arc<HookRegistry>,
    approvals: ApprovalManager,
    budgets: BudgetLedger,
    scope: Scope,
    ws: Workspace,
    secret: SecretString,
}

/// A test-only tool with configurable risk that echoes input and leaks a secret.
struct Probe {
    d: ToolDescriptor,
    secret: String,
}

#[async_trait]
impl Tool for Probe {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.d
    }
    async fn invoke(&self, _ctx: &ToolContext, input: Value) -> Result<Value> {
        Ok(json!({"echo": input, "note": format!("leaked {}", self.secret)}))
    }
}

fn probe(id: &str, risk: RiskClass, secret: &str) -> Arc<Probe> {
    Arc::new(Probe {
        d: ToolDescriptor {
            id: ToolId::new(id).unwrap(),
            version: semver::Version::new(1, 0, 0),
            description: "probe".into(),
            risk,
            input_schema: json!({"type": "object"}),
            output_schema: json!({"type": "object"}),
            isolation: Default::default(),
            needs_execution: false,
            needs_workspace: false,
            timeout_ms: 1000,
            rate_limit_per_minute: None,
            idempotent: false,
            max_retries: 0,
        },
        secret: secret.into(),
    })
}

async fn fx() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let redactor = Redactor::new();
    let db = Db::open_with(
        &dir.path().join("t.db"),
        eaeh_core::clock::system_clock(),
        redactor.clone(),
    )
    .await
    .unwrap();
    let secret = SecretString::new("sk-live-very-secret-123");
    redactor.register(&secret);
    let hooks = Arc::new(HookRegistry::new());
    let reg = Arc::new(ToolRegistry::new());
    reg.register(Arc::new(FsRead::default()), "core").unwrap();
    reg.register(Arc::new(FsWrite::default()), "core").unwrap();
    reg.register(
        Arc::new(ExecRun::new(vec!["/bin/echo".into()], default_exec_limits())),
        "core",
    )
    .unwrap();
    reg.register(
        probe("probe.deploy", RiskClass::Deployment, secret.expose()),
        "core",
    )
    .unwrap();
    reg.register(
        probe("probe.external", RiskClass::ExternalApi, secret.expose()),
        "core",
    )
    .unwrap();
    reg.register(probe("probe.read", RiskClass::ReadOnly, secret.expose()), "core")
        .unwrap();
    let authz = Arc::new(RbacAuthorizer);
    let approvals = ApprovalManager::new(db.clone(), authz.clone());
    let budgets = BudgetLedger::new(db.clone());
    let mut providers = ProviderRegistry::new();
    providers.add(Arc::new(RestrictedProcessProvider::new()));
    let gw = ToolGateway::new(
        db.clone(),
        reg,
        authz,
        Arc::new(RuleEngine::with_baseline().unwrap()),
        approvals.clone(),
        budgets.clone(),
        hooks.clone(),
        providers,
    );
    gw.output_interceptors()
        .add(Arc::new(RedactionInterceptor(redactor)))
        .unwrap();
    let scope = Scope::new(TenantId::new(), ProjectId::new());
    let wm = WorkspaceManager::new(db.clone(), &dir.path().join("ws"), hooks.clone()).unwrap();
    let ws = wm
        .create(scope, &PrincipalId::new("owner").unwrap())
        .await
        .unwrap();
    budgets
        .init(
            "wf-1",
            &BudgetLimits::default().with(BudgetResource::ToolCalls, 100),
        )
        .await
        .unwrap();
    Fx {
        _dir: dir,
        db,
        gw,
        hooks,
        approvals,
        budgets,
        scope,
        ws,
        secret,
    }
}

fn agent(scope: Scope, autonomy: AutonomyLevel) -> Principal {
    Principal::agent(
        AgentInstanceId::new(),
        AgentDefinitionId::new("dev").unwrap(),
        "1.0.0".into(),
        autonomy,
        scope,
    )
}

fn req(f: &Fx, p: &Principal, tool: &str, input: Value) -> ToolRequest {
    ToolRequest {
        principal: p.clone(),
        scope: f.scope,
        tool: ToolId::new(tool).unwrap(),
        version: None,
        input,
        workspace: Some(f.ws.clone()),
        budget_owners: vec!["wf-1".into()],
        approval: None,
        allowed_tools: Some(
            [
                "fs.read",
                "fs.write",
                "exec.run",
                "probe.deploy",
                "probe.external",
                "probe.read",
            ]
            .iter()
            .map(|t| ToolId::new(*t).unwrap())
            .collect(),
        ),
        trace: TraceContext::new_root(),
    }
}

#[tokio::test]
async fn authorized_write_then_read_is_audited_and_evented() {
    let f = fx().await;
    let a = agent(f.scope, AutonomyLevel::WorkspaceModify);
    let w =
        f.gw.invoke(req(
            &f,
            &a,
            "fs.write",
            json!({"path": "notes/a.txt", "content": "hello"}),
        ))
        .await
        .unwrap();
    assert!(w.policy_rules.iter().any(|r| r.contains("tool-low-risk-allow")));
    let r =
        f.gw.invoke(req(&f, &a, "fs.read", json!({"path": "notes/a.txt"})))
            .await
            .unwrap();
    assert_eq!(r.output.as_data()["content"], "hello");
    let audit = f.db.audit_records(None, 0, 100).await.unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|a| a.action == "tool.invoke" && a.outcome == "succeeded")
            .count(),
        2
    );
    let (n,): (i64,) =
        eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM outbox WHERE event_type = 'tool.invoked'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert_eq!(n, 2);
}

#[tokio::test]
async fn autonomy_and_allowlist_enforced() {
    let f = fx().await;
    let ro = agent(f.scope, AutonomyLevel::ReadOnly);
    let e =
        f.gw.invoke(req(&f, &ro, "fs.write", json!({"path": "x", "content": "y"})))
            .await
            .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    assert!(e.message.contains("autonomy"));

    let a = agent(f.scope, AutonomyLevel::WorkspaceModify);
    let mut r = req(&f, &a, "fs.write", json!({"path": "x", "content": "y"}));
    r.allowed_tools = Some(vec![ToolId::new("fs.read").unwrap()]);
    assert_eq!(f.gw.invoke(r).await.unwrap_err().kind, ErrorKind::Forbidden);

    let mut r = req(&f, &a, "fs.read", json!({"path": "x"}));
    r.allowed_tools = None;
    assert_eq!(
        f.gw.invoke(r).await.unwrap_err().kind,
        ErrorKind::Forbidden,
        "agents must present allowlist"
    );

    let denied = f.db.audit_records(None, 0, 100).await.unwrap();
    assert!(denied.iter().filter(|a| a.outcome == "denied").count() >= 3);
}

#[tokio::test]
async fn unregistered_or_contract_violating_calls_rejected() {
    let f = fx().await;
    let a = agent(f.scope, AutonomyLevel::WorkspaceModify);
    let mut r = req(&f, &a, "fs.read", json!({}));
    r.tool = ToolId::new("shell.anything").unwrap();
    r.allowed_tools = Some(vec![r.tool.clone()]);
    assert_eq!(f.gw.invoke(r).await.unwrap_err().kind, ErrorKind::NotFound);
    let e =
        f.gw.invoke(req(&f, &a, "fs.read", json!({"path": "x", "extra": true})))
            .await
            .unwrap_err();
    assert_eq!(e.kind, ErrorKind::InvalidInput);
}

#[tokio::test]
async fn isolation_requirements_refused_when_unavailable() {
    let f = fx().await;
    let a = agent(f.scope, AutonomyLevel::ExecuteAuthorized);
    let e =
        f.gw.invoke(req(
            &f,
            &a,
            "exec.run",
            json!({"program": "/bin/echo", "args": ["hi"]}),
        ))
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Isolation, "{e}");
    assert!(e.message.contains("network_isolation"));
}

#[tokio::test]
async fn external_api_denied_by_default_policy() {
    let f = fx().await;
    let a = agent(f.scope, AutonomyLevel::ExternalSideEffects);
    assert_eq!(
        f.gw.invoke(req(&f, &a, "probe.external", json!({})))
            .await
            .unwrap_err()
            .kind,
        ErrorKind::PolicyDenied
    );
}

#[tokio::test]
async fn high_risk_tool_requires_bound_human_approval() {
    let f = fx().await;
    let a = agent(f.scope, AutonomyLevel::ExternalSideEffects);
    let e =
        f.gw.invoke(req(&f, &a, "probe.deploy", json!({"env": "prod"})))
            .await
            .unwrap_err();
    assert_eq!(e.kind, ErrorKind::ApprovalRequired);
    let pending = f
        .approvals
        .list(f.scope, Some(ApprovalStatus::Pending), None, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    let approval_id = pending[0].id;

    // The agent itself cannot approve.
    assert_eq!(
        f.approvals
            .decide(
                &a,
                approval_id,
                ApprovalDecision::Approve,
                "",
                None,
                TraceContext::new_root()
            )
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
    // A pending (not yet approved) approval id does not unlock the call.
    let mut r = req(&f, &a, "probe.deploy", json!({"env": "prod"}));
    r.approval = Some(approval_id);
    assert_eq!(
        f.gw.invoke(r).await.unwrap_err().kind,
        ErrorKind::ApprovalRequired
    );
    let approval_id = f
        .approvals
        .list(f.scope, Some(ApprovalStatus::Pending), None, 10)
        .await
        .unwrap()[0]
        .id;

    let human = Principal::human(
        PrincipalId::new("carol").unwrap(),
        f.scope.tenant,
        [Role::Approver].into(),
        ProjectAccess::All,
    );
    f.approvals
        .decide(
            &human,
            approval_id,
            ApprovalDecision::Approve,
            "ok",
            None,
            TraceContext::new_root(),
        )
        .await
        .unwrap();

    let mut r = req(&f, &a, "probe.deploy", json!({"env": "prod"}));
    r.approval = Some(approval_id);
    f.gw.invoke(r).await.unwrap();

    // Approval is bound to the exact input: a different input is not covered.
    let mut r = req(&f, &a, "probe.deploy", json!({"env": "prod", "force": true}));
    r.approval = Some(approval_id);
    assert_eq!(
        f.gw.invoke(r).await.unwrap_err().kind,
        ErrorKind::ApprovalRequired
    );
}

#[tokio::test]
async fn budgets_enforced_and_fail_closed() {
    let f = fx().await;
    f.budgets
        .init(
            "tiny",
            &BudgetLimits::default().with(BudgetResource::ToolCalls, 2),
        )
        .await
        .unwrap();
    let a = agent(f.scope, AutonomyLevel::ReadOnly);
    for _ in 0..2 {
        let mut r = req(&f, &a, "probe.read", json!({}));
        r.budget_owners = vec!["tiny".into()];
        f.gw.invoke(r).await.unwrap();
    }
    let mut r = req(&f, &a, "probe.read", json!({}));
    r.budget_owners = vec!["tiny".into()];
    assert_eq!(f.gw.invoke(r).await.unwrap_err().kind, ErrorKind::BudgetExceeded);
    let mut r = req(&f, &a, "probe.read", json!({}));
    r.budget_owners = vec![];
    assert_eq!(f.gw.invoke(r).await.unwrap_err().kind, ErrorKind::BudgetExceeded);
}

struct AlwaysFail;
#[async_trait]
impl Hook for AlwaysFail {
    async fn on_event(&self, _: &HookContext) -> Result<HookOutcome> {
        Err(eaeh_core::HarnessError::internal("hook crashed"))
    }
}

struct Veto;
#[async_trait]
impl Hook for Veto {
    async fn on_event(&self, _: &HookContext) -> Result<HookOutcome> {
        Ok(HookOutcome::Veto("compliance check failed".into()))
    }
}

#[tokio::test]
async fn mandatory_controls_hold_when_hooks_missing_disabled_or_failing() {
    let f = fx().await;
    let ro = agent(f.scope, AutonomyLevel::ReadOnly);
    let call = || req(&f, &ro, "fs.write", json!({"path": "x", "content": "y"}));
    // No hooks registered.
    assert_eq!(f.gw.invoke(call()).await.unwrap_err().kind, ErrorKind::Forbidden);
    // Failing hook with Continue policy.
    f.hooks
        .register(
            HookRegistration::new(
                "crashy",
                vec![HookPoint::BeforeToolInvocation],
                FailurePolicy::Continue,
            ),
            Arc::new(AlwaysFail),
        )
        .unwrap();
    assert_eq!(f.gw.invoke(call()).await.unwrap_err().kind, ErrorKind::Forbidden);
    // Disabled hook.
    f.hooks.set_enabled("crashy", false).unwrap();
    assert_eq!(f.gw.invoke(call()).await.unwrap_err().kind, ErrorKind::Forbidden);

    // A blocking hook can still make an *allowed* call more restrictive.
    let rw = agent(f.scope, AutonomyLevel::WorkspaceModify);
    f.hooks
        .register(
            HookRegistration::new(
                "veto",
                vec![HookPoint::BeforeToolInvocation],
                FailurePolicy::Block,
            ),
            Arc::new(Veto),
        )
        .unwrap();
    let e =
        f.gw.invoke(req(&f, &rw, "fs.write", json!({"path": "x", "content": "y"})))
            .await
            .unwrap_err();
    assert_eq!(e.kind, ErrorKind::HookBlocked);
}

#[tokio::test]
async fn tool_output_secrets_are_redacted_and_untrusted() {
    let f = fx().await;
    let a = agent(f.scope, AutonomyLevel::ReadOnly);
    let r =
        f.gw.invoke(req(&f, &a, "probe.read", json!({"q": 1})))
            .await
            .unwrap();
    let text = r.output.as_data().to_string();
    assert!(!text.contains(f.secret.expose()), "{text}");
    assert!(text.contains("[REDACTED]"));
}

#[tokio::test]
async fn cross_project_access_denied() {
    let f = fx().await;
    let other = Scope::new(f.scope.tenant, ProjectId::new());
    let a = agent(other, AutonomyLevel::WorkspaceModify);
    // Agent bound to another project tries to use this project's scope + workspace.
    let e =
        f.gw.invoke(req(&f, &a, "fs.read", json!({"path": "x"})))
            .await
            .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Forbidden);
    // Agent in its own scope but presenting a foreign workspace.
    let mut r = req(&f, &a, "fs.read", json!({"path": "x"}));
    r.scope = other;
    assert_eq!(f.gw.invoke(r).await.unwrap_err().kind, ErrorKind::Forbidden);
}
