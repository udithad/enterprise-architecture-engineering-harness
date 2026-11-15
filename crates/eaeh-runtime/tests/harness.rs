//! Composition-root tests: shipped definitions load, demo workflow end to end, plugin
//! governance (AC-17), supervision escalation (AC-18), health/metrics (AC-27).

use async_trait::async_trait;
use eaeh_core::clock::ManualClock;
use eaeh_core::{
    AgentDefinitionId, AgentInstanceId, AutonomyLevel, ErrorKind, PluginId, Principal, PrincipalId,
    PrincipalKind, ProjectAccess, Result, RiskClass, Role, Scope, ToolId, TraceContext, WorkflowDefinitionId,
};
use eaeh_extensibility::plugin::ResourceLimitsSpec;
use eaeh_extensibility::{
    FailurePolicy, Hook, HookContext, HookOutcome, HookPoint, HookRegistration, PluginCapability, PluginKind,
    PluginManifest,
};
use eaeh_governance::{ApprovalDecision, ApprovalStatus, NewPrincipal};
use eaeh_runtime::{ExtensionContext, Harness, HarnessConfig, InProcessExtension};
use eaeh_tools::{Tool, ToolContext, ToolDescriptor, ToolRequest};
use eaeh_workflow::{InstanceStatus, TaskExecution, TaskExecutor, TaskOutput};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn repo_config(data: &std::path::Path) -> HarnessConfig {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/eaeh.toml");
    let mut cfg = HarnessConfig::load(&path).expect("shipped config must load");
    // Uses a scripted model provider: direct_provider mode (backward-compatible path).
    cfg.execution.mode = eaeh_agents::ExecutionMode::DirectProvider;
    cfg.storage.data_dir = data.to_path_buf();
    cfg.supervision.dispatch_interval_ms = 50;
    cfg
}

async fn people(h: &Harness) -> (Principal, Principal, Principal, Scope) {
    let (admin, project, _tok) = h
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
    let scope = Scope::new(admin.tenant, project);
    let mk = |id: &str, role: Role| NewPrincipal {
        id: PrincipalId::new(id).unwrap(),
        display_name: id.into(),
        kind: PrincipalKind::Human,
        roles: [role].into(),
        projects: ProjectAccess::Only([project].into()),
    };
    let op = h
        .identity
        .create_principal(&admin, mk("olivia", Role::Operator))
        .await
        .unwrap();
    let ap = h
        .identity
        .create_principal(&admin, mk("arthur", Role::Approver))
        .await
        .unwrap();
    (admin, op, ap, scope)
}

async fn wait_for(h: &Harness, id: eaeh_core::WorkflowInstanceId, want: InstanceStatus) -> InstanceStatus {
    for _ in 0..200 {
        let s = h.engine.get(id).await.unwrap().status;
        if s == want || (s.is_terminal() && want != s) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    h.engine.get(id).await.unwrap().status
}

#[tokio::test]
async fn shipped_definitions_and_demo_workflow_end_to_end() {
    let d = tempfile::tempdir().unwrap();
    let h = Arc::new(Harness::builder(repo_config(d.path())).build().await.unwrap());
    h.start_background().await.unwrap();
    let agents: Vec<String> = h
        .agent_registry
        .active()
        .await
        .unwrap()
        .into_iter()
        .map(|a| a.id.to_string())
        .collect();
    assert_eq!(
        agents,
        vec![
            "analyst",
            "api-reviewer",
            "builder",
            "business-analyst",
            "delivery-planner",
            "planner",
            "requirements-engineer",
            "software-designer",
            "software-engineer",
            "solution-architect",
            "writer"
        ]
    );
    let wfs: Vec<String> = h
        .engine
        .definitions()
        .await
        .unwrap()
        .into_iter()
        .map(|w| w.id.to_string())
        .collect();
    assert_eq!(
        wfs,
        vec![
            "demo.approval",
            "engineering.delivery",
            "engineering.delivery-unverified",
            "engineering.design",
            "engineering.task",
            "knowledge.review",
            "requirements.analysis"
        ]
    );
    assert!(
        h.prompts
            .active(&eaeh_core::PromptId::new("system.agent-core").unwrap())
            .await
            .is_ok()
    );

    let (_admin, op, ap, scope) = people(&h).await;
    let id = h
        .start_workflow(
            &op,
            &WorkflowDefinitionId::new("demo.approval").unwrap(),
            scope,
            json!({}),
        )
        .await
        .unwrap();
    assert_eq!(
        wait_for(&h, id, InstanceStatus::WaitingApproval).await,
        InstanceStatus::WaitingApproval
    );
    let pending = h
        .approvals
        .list(scope, Some(ApprovalStatus::Pending), None, 10)
        .await
        .unwrap();
    let a = &pending[0];
    // Operator who started it is not an approver.
    assert_eq!(
        h.decide_approval(
            &op,
            a.id,
            ApprovalDecision::Approve,
            "",
            a.artifact_hash.as_deref()
        )
        .await
        .unwrap_err()
        .kind,
        ErrorKind::Forbidden
    );
    h.decide_approval(
        &ap,
        a.id,
        ApprovalDecision::Approve,
        "ship it",
        a.artifact_hash.as_deref(),
    )
    .await
    .unwrap();
    assert_eq!(
        wait_for(&h, id, InstanceStatus::Succeeded).await,
        InstanceStatus::Succeeded
    );
    let inst = h.engine.get(id).await.unwrap();
    assert!(
        inst.data.outputs["publish"]["content"]
            .as_str()
            .unwrap()
            .contains("Adopt EAEH")
    );
    assert_eq!(inst.data.artifacts["draft"].version, 1);

    // Events dispatched to the metrics subscriber; health reports components.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let m = h.metrics.snapshot();
    assert!(m.keys().any(|k| k.contains("workflow.completed")), "{m:?}");
    let health = h.health().await;
    assert_eq!(health.status, "ok");
    assert_eq!(health.migrations, vec![1, 2, 3, 4, 5, 6]);
    assert!(
        health
            .isolation
            .iter()
            .any(|(p, s)| p == "restricted-process" && s == "available")
    );
    assert_eq!(
        health.model_providers, 0,
        "no production model provider in Phase 1"
    );
    assert!(h.db.verify_audit_chain().await.unwrap().valid);
    h.shutdown().await;
}

struct Noop;
#[async_trait]
impl Hook for Noop {
    async fn on_event(&self, _: &HookContext) -> Result<HookOutcome> {
        Ok(HookOutcome::Proceed)
    }
}

struct DeployTool(ToolDescriptor);
#[async_trait]
impl Tool for DeployTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.0
    }
    async fn invoke(&self, _: &ToolContext, _: Value) -> Result<Value> {
        Ok(json!({"deployed": true}))
    }
}

struct TestExtension {
    id: &'static str,
    caps: Vec<PluginCapability>,
    add_tool: bool,
}

impl InProcessExtension for TestExtension {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: PluginId::new(self.id).unwrap(),
            version: semver::Version::new(1, 0, 0),
            kind: PluginKind::InProcess,
            description: "test extension".into(),
            eaeh_api: semver::VersionReq::parse("^1").unwrap(),
            dependencies: vec![],
            capabilities: self.caps.iter().copied().collect(),
            config_schema: None,
            resource_limits: ResourceLimitsSpec {
                timeout_ms: 1000,
                memory_mb: None,
            },
            integrity: None,
        }
    }
    fn register(&self, ctx: &mut ExtensionContext<'_>) -> Result<()> {
        ctx.add_hook(
            HookRegistration::new("audit", vec![HookPoint::AfterToolInvocation], FailurePolicy::Warn),
            Arc::new(Noop),
        )?;
        if self.add_tool {
            ctx.add_tool(Arc::new(DeployTool(ToolDescriptor {
                id: ToolId::new(format!("{}.deploy", self.id)).unwrap(),
                version: semver::Version::new(1, 0, 0),
                description: "plugin deploy tool".into(),
                risk: RiskClass::Deployment,
                input_schema: json!({"type": "object"}),
                output_schema: json!({"type": "object"}),
                isolation: Default::default(),
                needs_execution: false,
                needs_workspace: false,
                timeout_ms: 1000,
                rate_limit_per_minute: None,
                idempotent: false,
                max_retries: 0,
            })))?;
        }
        Ok(())
    }
}

#[tokio::test]
async fn plugins_require_admin_enablement_grants_and_cannot_bypass_policy() {
    let d = tempfile::tempdir().unwrap();
    let mut cfg = repo_config(d.path());
    cfg.isolation.container_enabled = false;
    cfg.plugins.require_signature = false;
    cfg.plugins.grantable = [PluginCapability::RegisterHooks, PluginCapability::RegisterTools].into();
    let exts = || -> Vec<Box<dyn InProcessExtension>> {
        vec![
            Box::new(TestExtension {
                id: "acme.hooks",
                caps: vec![PluginCapability::RegisterHooks],
                add_tool: true,
            }),
            Box::new(TestExtension {
                id: "acme.deploy",
                caps: vec![PluginCapability::RegisterHooks, PluginCapability::RegisterTools],
                add_tool: true,
            }),
        ]
    };
    let build = |cfg: HarnessConfig| {
        let mut b = Harness::builder(cfg);
        for e in exts() {
            b = b.extension(e);
        }
        b.build()
    };
    // First start: manifests registered but nothing active.
    let h = build(cfg.clone()).await.unwrap();
    assert!(h.active_plugins.is_empty());
    assert!(h.hooks.registrations().is_empty());
    let (admin, _op, ap, scope) = people(&h).await;
    for id in ["acme.hooks", "acme.deploy"] {
        h.plugins
            .enable(
                &admin,
                &PluginId::new(id).unwrap(),
                &semver::Version::new(1, 0, 0),
            )
            .await
            .unwrap();
    }
    h.db.close().await;

    // Restart: acme.hooks tries to add a tool it was not granted → whole plugin fails;
    // acme.deploy activates with both capabilities.
    let h = Arc::new(build(cfg).await.unwrap());
    assert_eq!(h.active_plugins, vec!["acme.deploy@1.0.0"]);
    let failed = h
        .plugins
        .get(
            &PluginId::new("acme.hooks").unwrap(),
            &semver::Version::new(1, 0, 0),
        )
        .await
        .unwrap();
    assert_eq!(failed.state, eaeh_extensibility::PluginState::Failed);
    assert!(h.hooks.registrations().iter().all(|r| r.source == "acme.deploy"));

    // The plugin's deployment tool is still governed: agents need approval.
    let agent = Principal::agent(
        AgentInstanceId::new(),
        AgentDefinitionId::new("deployer").unwrap(),
        "1.0.0".into(),
        AutonomyLevel::ExternalSideEffects,
        scope,
    );
    h.budgets
        .init("wf-x", &eaeh_governance::BudgetLimits::platform_default())
        .await
        .unwrap();
    let e = h
        .gateway
        .invoke(ToolRequest {
            principal: agent,
            scope,
            tool: ToolId::new("acme.deploy.deploy").unwrap(),
            version: None,
            input: json!({}),
            workspace: None,
            budget_owners: vec!["wf-x".into()],
            approval: None,
            allowed_tools: Some(vec![ToolId::new("acme.deploy.deploy").unwrap()]),
            trace: TraceContext::new_root(),
        })
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::ApprovalRequired);
    let _ = ap;
    // Non-admins cannot manage plugins.
    let op2 = Principal::human(
        PrincipalId::new("olivia").unwrap(),
        scope.tenant,
        [Role::Operator].into(),
        ProjectAccess::All,
    );
    assert_eq!(
        h.plugins
            .disable(
                &op2,
                &PluginId::new("acme.deploy").unwrap(),
                &semver::Version::new(1, 0, 0)
            )
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Forbidden
    );
}

struct Blocking(tokio::sync::Notify);
#[async_trait]
impl TaskExecutor for Blocking {
    async fn execute(&self, _t: TaskExecution) -> Result<TaskOutput> {
        self.0.notify_one();
        tokio::time::sleep(Duration::from_secs(3600)).await;
        unreachable!()
    }
}

#[tokio::test]
async fn supervision_escalates_stalled_work() {
    let d = tempfile::tempdir().unwrap();
    let mut cfg = repo_config(d.path());
    cfg.isolation.container_enabled = false;
    let clock = ManualClock::new(chrono::Utc::now());
    let exec = Arc::new(Blocking(tokio::sync::Notify::new()));
    let h = Arc::new(
        Harness::builder(cfg)
            .clock(Arc::new(clock.clone()))
            .executor(exec.clone())
            .build()
            .await
            .unwrap(),
    );
    let (_admin, op, _ap, scope) = people(&h).await;
    let id = h
        .start_workflow(
            &op,
            &WorkflowDefinitionId::new("demo.approval").unwrap(),
            scope,
            json!({}),
        )
        .await
        .unwrap();
    exec.0.notified().await;
    assert!(
        h.supervisor.scan_once().await.unwrap().stalled_tasks.is_empty(),
        "fresh task is not stalled"
    );
    clock.advance(chrono::Duration::hours(3));
    let r = h.supervisor.scan_once().await.unwrap();
    assert_eq!(r.stalled_tasks.len(), 1);
    assert_eq!(h.engine.get(id).await.unwrap().status, InstanceStatus::Escalated);
    let esc: (i64,) =
        eaeh_store::sqlx::query_as("SELECT COUNT(*) FROM outbox WHERE event_type = 'supervision.escalated'")
            .fetch_one(h.db.pool())
            .await
            .unwrap();
    assert_eq!(esc.0, 1);
}

#[test]
fn config_rejects_unknown_fields_and_relative_programs() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("c.toml");
    std::fs::write(&p, "[storage]\ndata_dir = \"x\"\npassword = \"nope\"\n").unwrap();
    assert!(HarnessConfig::load(&p).is_err());
    std::fs::write(&p, "[isolation]\nexec_allowed_programs = [\"sh\"]\n").unwrap();
    assert!(HarnessConfig::load(&p).is_err());
}

#[tokio::test]
async fn supervision_escalates_repeated_failures() {
    let d = tempfile::tempdir().unwrap();
    let mut cfg = repo_config(d.path());
    cfg.isolation.container_enabled = false;
    cfg.supervision.max_failed_tasks_per_instance = 2;
    let exec = Arc::new(Blocking(tokio::sync::Notify::new()));
    let h = Arc::new(
        Harness::builder(cfg)
            .executor(exec.clone())
            .build()
            .await
            .unwrap(),
    );
    let (_admin, op, _ap, scope) = people(&h).await;
    let id = h
        .start_workflow(
            &op,
            &WorkflowDefinitionId::new("demo.approval").unwrap(),
            scope,
            json!({}),
        )
        .await
        .unwrap();
    exec.0.notified().await;
    // Simulate a history of failed attempts recorded for this still-running instance.
    for i in 0..2 {
        let mut tx = h.db.write().await.unwrap();
        eaeh_store::sqlx::query(
            "INSERT INTO tasks (id, workflow_instance_id, tenant_id, project_id, state_name, kind, idempotency_key, status, attempt, input, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 'draft', '{}', ?, 'failed', 1, '{}', '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z')",
        )
        .bind(eaeh_core::TaskId::new().to_string())
        .bind(id.to_string())
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(format!("simulated-failure-{i}"))
        .execute(tx.conn())
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let r = h.supervisor.scan_once().await.unwrap();
    assert_eq!(r.escalated_instances, vec![id.to_string()]);
    assert_eq!(h.engine.get(id).await.unwrap().status, InstanceStatus::Escalated);
    let audit: (i64,) = eaeh_store::sqlx::query_as(
        "SELECT COUNT(*) FROM audit_log WHERE action = 'supervision.escalate' AND details LIKE '%repeated failures%'",
    )
    .fetch_one(h.db.pool())
    .await
    .unwrap();
    assert_eq!(audit.0, 1);
}

struct KeyProvider(Option<&'static str>);
impl eaeh_core::secret::SecretsProvider for KeyProvider {
    fn name(&self) -> &str {
        "test"
    }
    fn get(&self, key: &str) -> Result<eaeh_core::SecretString> {
        match (key, self.0) {
            ("anthropic.api_key", Some(k)) => Ok(eaeh_core::SecretString::new(k)),
            _ => Err(eaeh_core::HarnessError::not_found("not configured")),
        }
    }
}

#[tokio::test]
async fn anthropic_provider_is_opt_in_and_needs_a_secret() {
    let d = tempfile::tempdir().unwrap();
    // Disabled by default: no model providers registered.
    let mut cfg = repo_config(d.path());
    cfg.isolation.container_enabled = false;
    assert!(!cfg.providers.anthropic.enabled);
    let h = Harness::builder(cfg.clone()).build().await.unwrap();
    assert_eq!(h.health().await.model_providers, 0);
    h.db.close().await;

    // Enabled without a key: startup fails with an explicit message (no default credentials).
    cfg.providers.anthropic.enabled = true;
    let e = Harness::builder(cfg.clone())
        .secrets(Arc::new(KeyProvider(None)))
        .build()
        .await
        .unwrap_err();
    assert!(e.message.contains("EAEH_SECRET_ANTHROPIC_API_KEY"), "{e}");

    // Enabled with a key from the secrets provider: models registered, key redacted.
    let key = "sk-ant-runtime-test-0123456789";
    let h = Harness::builder(cfg)
        .secrets(Arc::new(KeyProvider(Some(key))))
        .build()
        .await
        .unwrap();
    assert_eq!(h.model_provider_ids, vec!["anthropic"]);
    assert_eq!(h.models.models()[0].id.as_str(), "claude-opus-5-5");
    assert!(!h.redactor.redact_str(&format!("key={key}")).contains(key));
    assert!(!format!("{:?}", h.models).contains(key));
}
