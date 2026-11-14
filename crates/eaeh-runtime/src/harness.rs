//! Composition root: the only place concrete implementations are wired together.

use crate::config::HarnessConfig;
use crate::executor::{AgentPlanner, AgentTaskExecutor};
use crate::extensions::{ExtensionContext, InProcessExtension};
use crate::observability::{Metrics, MetricsSubscriber};
use crate::supervision::Supervisor;
use eaeh_agents::{
    AgentDefinition, AgentRegistry, AgentRuntime, ExecutionMode, ExternalSubmission, KnowledgeIntegration,
};
use eaeh_core::clock::{SharedClock, system_clock};
use eaeh_core::secret::{EnvSecretsProvider, SecretsProvider};
use eaeh_core::{
    ApprovalId, HarnessError, Principal, Redactor, Result, Scope, TaskId, TraceContext, WorkflowDefinitionId,
    WorkflowInstanceId,
};
use eaeh_events::{EventDispatcher, RetryPolicy};
use eaeh_extensibility::interceptor::{RedactionInterceptor, SizeLimitInterceptor};
use eaeh_extensibility::{HookRegistry, PluginRegistry, PluginState};
use eaeh_governance::authz::{Authorizer, RbacAuthorizer};
use eaeh_governance::policy::{self, PolicyDocument, RuleEngine};
use eaeh_governance::{ApprovalDecision, ApprovalManager, BudgetLedger, IdentityService};
use eaeh_intelligence::HashedNgramEmbedder;
use eaeh_intelligence::{ContextAssembler, ModelClient, ModelProvider, ModelRegistry, PromptRegistry};
use eaeh_isolation::{
    ContainerConfig, ContainerProvider, ProviderRegistry, ResourceLimits, RestrictedProcessProvider,
};
use eaeh_knowledge::{KnowledgeReadTool, KnowledgeSearchTool, KnowledgeService};
use eaeh_provider_anthropic::AnthropicProvider;
use eaeh_skills::SkillRegistry;
use eaeh_store::Db;
use eaeh_tools::builtin::{ExecRun, FsList, FsRead, FsWrite};
use eaeh_tools::{Tool, ToolGateway, ToolRegistry};
use eaeh_workflow::{Planner, TaskExecutor, WorkflowDefinition, WorkflowEngine};
use eaeh_workspace::{ArtifactStore, WorkspaceManager};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct HarnessBuilder {
    config: HarnessConfig,
    clock: Option<SharedClock>,
    model_providers: Vec<Arc<dyn ModelProvider>>,
    extensions: Vec<Box<dyn InProcessExtension>>,
    tools: Vec<Arc<dyn Tool>>,
    executor: Option<Arc<dyn TaskExecutor>>,
    planner: Option<Arc<dyn Planner>>,
    secrets: Option<Arc<dyn SecretsProvider>>,
    redactor: Option<Redactor>,
}

impl HarnessBuilder {
    pub fn new(config: HarnessConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    pub fn clock(mut self, c: SharedClock) -> Self {
        self.clock = Some(c);
        self
    }

    /// Register a model provider from the composition root (none in the Phase 1 binary).
    pub fn model_provider(mut self, p: Arc<dyn ModelProvider>) -> Self {
        self.model_providers.push(p);
        self
    }

    pub fn extension(mut self, e: Box<dyn InProcessExtension>) -> Self {
        self.extensions.push(e);
        self
    }

    pub fn tool(mut self, t: Arc<dyn Tool>) -> Self {
        self.tools.push(t);
        self
    }

    /// Share a redactor with the logging pipeline so runtime-registered secrets are
    /// removed from logs as well as audit records and agent context.
    pub fn redactor(mut self, r: Redactor) -> Self {
        self.redactor = Some(r);
        self
    }

    pub fn secrets(mut self, s: Arc<dyn SecretsProvider>) -> Self {
        self.secrets = Some(s);
        self
    }

    /// Override the task executor (tests).
    pub fn executor(mut self, e: Arc<dyn TaskExecutor>) -> Self {
        self.executor = Some(e);
        self
    }

    pub fn planner(mut self, p: Arc<dyn Planner>) -> Self {
        self.planner = Some(p);
        self
    }

    pub async fn build(self) -> Result<Harness> {
        let cfg = self.config;
        cfg.validate()?;
        let redactor = self.redactor.unwrap_or_default();
        let clock = self.clock.unwrap_or_else(system_clock);
        let db = Db::open_with(&cfg.database_path(), clock, redactor.clone()).await?;
        let authz: Arc<dyn Authorizer> = Arc::new(RbacAuthorizer);
        let policy = Arc::new(RuleEngine::with_baseline()?);
        for doc in policy::load_active_policies(&db).await? {
            policy.load(doc)?;
        }
        load_policy_dir(&policy, &cfg.resolve(&cfg.definitions.policies_dir))?;

        let hooks = Arc::new(HookRegistry::new());
        let identity = IdentityService::new(db.clone(), authz.clone());
        let approvals = ApprovalManager::new(db.clone(), authz.clone());
        let budgets = BudgetLedger::new(db.clone());
        let workspaces =
            WorkspaceManager::new(db.clone(), &cfg.data_dir().join("workspaces"), hooks.clone())?;
        let artifacts = ArtifactStore::new(db.clone(), &cfg.data_dir().join("artifacts"), hooks.clone())?;

        // Isolation providers: strongest first so selection prefers containers.
        let mut providers = ProviderRegistry::new();
        let mut isolation_status = vec![];
        if cfg.isolation.container_enabled {
            let cp = ContainerProvider::new(ContainerConfig {
                runtime: cfg.isolation.container_runtime.clone(),
                image: cfg.isolation.container_image.clone(),
                user: cfg.isolation.container_user.clone(),
            })?;
            match cp.probe().await {
                Ok(()) => {
                    providers.add(Arc::new(cp));
                    isolation_status.push(("container".to_string(), "available".to_string()));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "container isolation unavailable; tools requiring it will be refused");
                    isolation_status.push(("container".to_string(), format!("unavailable: {}", e.message)));
                }
            }
        }
        if cfg.isolation.process_enabled {
            providers.add(Arc::new(RestrictedProcessProvider::new()));
            isolation_status.push(("restricted-process".to_string(), "available".to_string()));
        }

        let tools = Arc::new(ToolRegistry::new());
        tools.register(Arc::new(FsRead::default()), "core")?;
        tools.register(Arc::new(FsList::default()), "core")?;
        tools.register(Arc::new(FsWrite::default()), "core")?;
        // Phase 2: governed knowledge, its read-only search tool, and the skills registry.
        let embedder = Arc::new(HashedNgramEmbedder::new(cfg.knowledge.embedding_dims)?);
        let knowledge = KnowledgeService::new(
            db.clone(),
            authz.clone(),
            approvals.clone(),
            hooks.clone(),
            embedder,
            cfg.knowledge.clone(),
        );
        tools.register(Arc::new(KnowledgeSearchTool::new(knowledge.clone())?), "core")?;
        // Phase 3: deterministic requirements/architecture validators, quality gates and
        // read-only engineering-design tools.
        let validation = Arc::new(eaeh_validation::ValidationEngine::new());
        validation.register(Arc::new(eaeh_validation::builtin::SecretLeakValidator))?;
        eaeh_engineering::register_validators(&validation, cfg.requirements.clone())?;
        tools.register(Arc::new(KnowledgeReadTool::new(knowledge.clone())?), "core")?;
        tools.register(
            Arc::new(eaeh_engineering::BrdSectionsTool::new(knowledge.clone())?),
            "core",
        )?;
        tools.register(
            Arc::new(eaeh_engineering::RequirementsRenderTool::new(
                cfg.requirements.clone(),
            )?),
            "core",
        )?;
        tools.register(Arc::new(eaeh_engineering::ArchitectureRenderTool::new()?), "core")?;
        tools.register(
            Arc::new(eaeh_engineering::delivery::DeliverySnapshotTool::new()?),
            "core",
        )?;
        tools.register(
            Arc::new(eaeh_engineering::DesignCheckTool::new(validation.clone())?),
            "core",
        )?;
        tools.register(
            Arc::new(ExecRun::new(
                cfg.isolation.exec_allowed_programs.clone(),
                ResourceLimits {
                    timeout: Duration::from_secs(cfg.isolation.exec_timeout_secs),
                    memory_bytes: cfg.isolation.exec_memory_mb * 1024 * 1024,
                    ..Default::default()
                },
            )),
            "core",
        )?;
        for t in self.tools {
            tools.register(t, "composition-root")?;
        }
        let skills = SkillRegistry::new(db.clone(), authz.clone(), approvals.clone(), tools.clone());
        let secrets: Arc<dyn SecretsProvider> = self
            .secrets
            .clone()
            .unwrap_or_else(|| Arc::new(EnvSecretsProvider));
        let gateway = Arc::new(ToolGateway::new(
            db.clone(),
            tools.clone(),
            authz.clone(),
            policy.clone(),
            approvals.clone(),
            budgets.clone(),
            hooks.clone(),
            providers,
        ));
        gateway
            .output_interceptors()
            .add(Arc::new(RedactionInterceptor(redactor.clone())))?;
        gateway
            .output_interceptors()
            .add(Arc::new(SizeLimitInterceptor(2 * 1024 * 1024)))?;

        let models = Arc::new(ModelRegistry::new(
            cfg.models.circuit_failure_threshold,
            Duration::from_secs(cfg.models.circuit_cooldown_secs),
        ));
        let mode = cfg.execution.mode;
        let mut provider_status = vec![];
        let mut ignored_providers = vec![];
        if mode == ExecutionMode::DirectProvider {
            for p in self.model_providers {
                models.register(p)?;
            }
        } else {
            ignored_providers.extend(self.model_providers.iter().map(|p| p.id().to_string()));
        }
        if cfg.providers.anthropic.enabled && mode != ExecutionMode::DirectProvider {
            ignored_providers.push("anthropic".to_string());
        }
        if cfg.providers.replay.enabled && mode == ExecutionMode::ExternalAi {
            ignored_providers.push("replay".to_string());
        }
        if mode == ExecutionMode::Deterministic && !cfg.providers.replay.enabled {
            return Err(HarnessError::invalid_input(
                "[execution] mode = \"deterministic\" requires [providers.replay] enabled = true with a cassette",
            ));
        }
        if !ignored_providers.is_empty() {
            // No hidden fallback: in external_ai mode nothing may call a model provider.
            tracing::info!(
                mode = mode.as_str(),
                ignored = ?ignored_providers,
                "model providers not registered in this execution mode"
            );
        }
        if cfg.providers.anthropic.enabled && mode == ExecutionMode::DirectProvider {
            // Credentials come only from the secrets provider and are registered for redaction.
            let key = secrets.get("anthropic.api_key").map_err(|e| {
                e.context(
                    "providers.anthropic.enabled = true but EAEH_SECRET_ANTHROPIC_API_KEY is not available",
                )
            })?;
            redactor.register(&key);
            models.register(Arc::new(AnthropicProvider::new(
                cfg.providers.anthropic.clone(),
                key,
            )?))?;
            provider_status.push("anthropic".to_string());
        }
        if cfg.providers.replay.enabled && mode != ExecutionMode::ExternalAi {
            let path = cfg.resolve(&cfg.providers.replay.cassette);
            let p = eaeh_intelligence::replay::ReplayProvider::from_file(&path)
                .map_err(|e| e.context("providers.replay.enabled = true"))?;
            tracing::info!(
                version = p.version(),
                "deterministic replay provider enabled: model output is replayed, not AI-generated"
            );
            models.register(Arc::new(p))?;
            provider_status.push("replay".to_string());
        }
        let model_client = ModelClient::new(
            db.clone(),
            models.clone(),
            budgets.clone(),
            hooks.clone(),
            policy.clone(),
            Duration::from_secs(cfg.models.timeout_secs),
        );
        let prompts = PromptRegistry::new(db.clone(), authz.clone());
        prompts
            .load_platform_dir(&cfg.resolve(&cfg.definitions.prompts_dir))
            .await?;
        let context = ContextAssembler::new(db.clone(), hooks.clone(), cfg.models.context_max_tokens);

        // Plugins: verify compiled-in manifests; activate only admin-enabled ones.
        let plugins = PluginRegistry::new(db.clone(), authz.clone(), policy.clone(), cfg.trust_policy()?);
        let mut active_plugins = vec![];
        for ext in &self.extensions {
            let m = ext.manifest();
            let rec = plugins.register_compiled(m.clone()).await?;
            if rec.state == PluginState::Enabled {
                let mut ctx = ExtensionContext {
                    plugin: m.id.clone(),
                    granted: rec.granted.clone(),
                    hooks: &hooks,
                    tools: &tools,
                    models: &models,
                };
                if let Err(e) = ext.register(&mut ctx) {
                    hooks.unregister_source(m.id.as_str());
                    tools.unregister_source(m.id.as_str());
                    plugins.mark_failed(&m.id, &m.version, &e.to_string()).await?;
                    tracing::error!(plugin = %m.id, error = %e, "plugin registration failed; plugin marked failed");
                } else {
                    active_plugins.push(format!("{}@{}", m.id, m.version));
                }
            }
        }

        let agent_registry = AgentRegistry::new(db.clone(), authz.clone(), tools.clone());
        for def in load_json_dir::<AgentDefinition>(&cfg.resolve(&cfg.definitions.agents_dir))? {
            agent_registry.register_trusted(None, &def).await?;
        }
        let agents = AgentRuntime::new(
            db.clone(),
            agent_registry.clone(),
            gateway.clone(),
            model_client.clone(),
            prompts.clone(),
            context.clone(),
            hooks.clone(),
            budgets.clone(),
            authz.clone(),
            cfg.models.max_delegation_depth,
        )
        .with_knowledge(KnowledgeIntegration {
            knowledge: knowledge.clone(),
            skills: skills.clone(),
            k: cfg.knowledge.retrieval_k,
        })
        .with_validation(validation.clone())
        .with_execution_mode(mode, Some(workspaces.clone()));
        let executor = self
            .executor
            .unwrap_or_else(|| Arc::new(AgentTaskExecutor::new(agents.clone(), gateway.clone())));
        let planner = self
            .planner
            .unwrap_or_else(|| Arc::new(AgentPlanner::new(agents.clone())));
        let engine = Arc::new(
            WorkflowEngine::new(
                db.clone(),
                authz.clone(),
                policy.clone(),
                approvals.clone(),
                budgets.clone(),
                hooks.clone(),
                executor,
                planner,
                artifacts.clone(),
                workspaces.clone(),
            )
            .with_quality_gates(validation.clone(), eaeh_engineering::default_gates()),
        );
        for def in load_json_dir::<WorkflowDefinition>(&cfg.resolve(&cfg.definitions.workflows_dir))? {
            engine.register_definition(None, &def).await?;
        }

        let dispatcher = Arc::new(EventDispatcher::new(db.clone(), RetryPolicy::default()));
        let metrics = Metrics::default();
        dispatcher.subscribe(Arc::new(MetricsSubscriber(metrics.clone())))?;
        let supervisor = Supervisor::new(
            db.clone(),
            chrono::Duration::seconds(i64::try_from(cfg.supervision.stall_after_secs).unwrap_or(i64::MAX)),
            cfg.supervision.max_failed_tasks_per_instance,
        );

        Ok(Harness {
            config: cfg,
            db,
            redactor,
            authz,
            policy,
            identity,
            approvals,
            budgets,
            hooks,
            tools,
            gateway,
            models,
            model_client,
            prompts,
            validation,
            context,
            agent_registry,
            agents,
            artifacts,
            workspaces,
            plugins,
            knowledge,
            skills,
            model_provider_ids: provider_status,
            execution_mode: mode,
            ignored_providers,
            engine,
            dispatcher,
            supervisor,
            metrics,
            secrets,
            isolation_status,
            active_plugins,
            shutdown: CancellationToken::new(),
            background: std::sync::Mutex::new(vec![]),
        })
    }
}

fn load_json_dir<T: serde::de::DeserializeOwned>(dir: &Path) -> Result<Vec<T>> {
    let mut out = vec![];
    if !dir.exists() {
        return Ok(out);
    }
    let mut paths: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    paths.sort();
    for p in paths
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
    {
        let text = std::fs::read_to_string(&p)?;
        out.push(
            serde_json::from_str(&text)
                .map_err(|e| HarnessError::invalid_input(format!("{}: {e}", p.display())))?,
        );
    }
    Ok(out)
}

fn load_policy_dir(engine: &RuleEngine, dir: &Path) -> Result<()> {
    for doc in load_json_dir::<PolicyDocument>(dir)? {
        engine.load(doc)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub status: &'static str,
    pub database: String,
    pub migrations: Vec<i64>,
    pub isolation: Vec<(String, String)>,
    pub model_providers: usize,
    pub active_plugins: Vec<String>,
    pub pending_events: i64,
}

pub struct Harness {
    pub config: HarnessConfig,
    pub db: Db,
    pub redactor: Redactor,
    pub authz: Arc<dyn Authorizer>,
    pub policy: Arc<RuleEngine>,
    pub identity: IdentityService,
    pub approvals: ApprovalManager,
    pub budgets: BudgetLedger,
    pub hooks: Arc<HookRegistry>,
    pub tools: Arc<ToolRegistry>,
    pub gateway: Arc<ToolGateway>,
    pub models: Arc<ModelRegistry>,
    pub model_client: ModelClient,
    pub prompts: PromptRegistry,
    pub context: ContextAssembler,
    pub agent_registry: AgentRegistry,
    pub agents: AgentRuntime,
    pub artifacts: ArtifactStore,
    pub workspaces: WorkspaceManager,
    pub plugins: PluginRegistry,
    pub knowledge: KnowledgeService,
    pub skills: SkillRegistry,
    pub validation: Arc<eaeh_validation::ValidationEngine>,
    pub model_provider_ids: Vec<String>,
    /// Active execution mode (ADR-0025) and the enabled providers it left unregistered.
    pub execution_mode: ExecutionMode,
    pub ignored_providers: Vec<String>,
    pub engine: Arc<WorkflowEngine>,
    pub dispatcher: Arc<EventDispatcher>,
    pub supervisor: Supervisor,
    pub metrics: Metrics,
    pub secrets: Arc<dyn SecretsProvider>,
    pub isolation_status: Vec<(String, String)>,
    pub active_plugins: Vec<String>,
    shutdown: CancellationToken,
    background: std::sync::Mutex<Vec<JoinHandle<()>>>,
}

impl std::fmt::Debug for Harness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Harness")
            .field("data_dir", &self.config.data_dir())
            .finish_non_exhaustive()
    }
}

impl Harness {
    pub fn builder(config: HarnessConfig) -> HarnessBuilder {
        HarnessBuilder::new(config)
    }

    /// Start background loops: event dispatch, supervision, and recovery of
    /// interrupted workflows.
    pub async fn start_background(self: &Arc<Self>) -> Result<Vec<WorkflowInstanceId>> {
        let recovered = self.engine.recover().await?;
        for id in &recovered {
            self.spawn_drive(*id);
        }
        let me = self.clone();
        let dispatch = tokio::spawn(async move {
            let every = Duration::from_millis(me.config.supervision.dispatch_interval_ms);
            loop {
                tokio::select! {
                    _ = me.shutdown.cancelled() => break,
                    _ = tokio::time::sleep(every) => {
                        if let Err(e) = me.dispatcher.dispatch_pending(500).await {
                            tracing::error!(error = %e, "event dispatch failed");
                        }
                    }
                }
            }
        });
        let me = self.clone();
        let supervise = tokio::spawn(async move {
            let every = Duration::from_secs(me.config.supervision.interval_secs);
            loop {
                tokio::select! {
                    _ = me.shutdown.cancelled() => break,
                    _ = tokio::time::sleep(every) => {
                        if let Err(e) = me.supervisor.scan_once().await {
                            tracing::error!(error = %e, "supervision scan failed");
                        }
                    }
                }
            }
        });
        if let Ok(mut b) = self.background.lock() {
            b.push(dispatch);
            b.push(supervise);
        }
        Ok(recovered)
    }

    fn spawn_drive(self: &Arc<Self>, id: WorkflowInstanceId) {
        let me = self.clone();
        let h = tokio::spawn(async move {
            if let Err(e) = me.engine.drive(id).await {
                tracing::error!(workflow = %id, error = %e, "workflow drive failed");
            }
        });
        if let Ok(mut b) = self.background.lock() {
            b.retain(|h| !h.is_finished());
            b.push(h);
        }
    }

    /// Start a workflow and drive it asynchronously.
    pub async fn start_workflow(
        self: &Arc<Self>,
        actor: &Principal,
        definition: &WorkflowDefinitionId,
        scope: Scope,
        input: serde_json::Value,
    ) -> Result<WorkflowInstanceId> {
        self.identity.ensure_project(scope.tenant, scope.project).await?;
        let id = self
            .engine
            .start(actor, definition, scope, input, TraceContext::new_root())
            .await?;
        self.spawn_drive(id);
        Ok(id)
    }

    /// Decide an approval; the workflow resumes asynchronously.
    pub async fn decide_approval(
        self: &Arc<Self>,
        approver: &Principal,
        approval: ApprovalId,
        decision: ApprovalDecision,
        comments: &str,
        artifact_hash: Option<&str>,
    ) -> Result<eaeh_governance::ApprovalRecord> {
        let trace = TraceContext::new_root();
        let rec = self.approvals.get(approval).await?;
        if rec.workflow_instance_id.is_some() {
            let instance = self
                .engine
                .record_decision(approver, approval, decision, comments, artifact_hash, trace)
                .await?;
            self.spawn_drive(instance);
        } else {
            self.approvals
                .decide(approver, approval, decision, comments, artifact_hash, trace)
                .await?;
            // Governed subjects whose state follows the decision (each ignores other types).
            self.knowledge.apply_approval(approval, trace).await?;
            self.skills.apply_approval(approval, trace).await?;
        }
        self.approvals.get(approval).await
    }

    /// Submit an external AI client's result for a handed-off agent task (ADR-0025), then
    /// resume the workflow synchronously so that the caller learns the validation outcome.
    /// The workflow either moves on (to the next hand-off or approval gate) or reopens the
    /// task with feedback.
    pub async fn submit_external(
        self: &Arc<Self>,
        actor: &Principal,
        task: TaskId,
        output: &serde_json::Value,
        client: &str,
    ) -> Result<(ExternalSubmission, eaeh_workflow::InstanceStatus)> {
        let view = self.agents.external_submit(actor, task, output, client).await?;
        let instance: WorkflowInstanceId = view.workflow_instance.parse()?;
        let status = self.engine.drive(instance).await?;
        Ok((self.agents.external_outcome_of(actor, task).await?, status))
    }

    /// Fetch a secret through the configured provider and register it for redaction in
    /// logs, audit records, tool output and agent context.
    pub fn secret(&self, key: &str) -> Result<eaeh_core::SecretString> {
        let s = self.secrets.get(key)?;
        self.redactor.register(&s);
        Ok(s)
    }

    pub async fn health(&self) -> HealthReport {
        let db_ok = sqlx_ping(&self.db).await;
        let pending: i64 =
            eaeh_store::sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM outbox WHERE status = 'pending'")
                .fetch_one(self.db.pool())
                .await
                .map(|r| r.0)
                .unwrap_or(-1);
        HealthReport {
            status: if db_ok { "ok" } else { "degraded" },
            database: if db_ok { "ok".into() } else { "unavailable".into() },
            migrations: self.db.applied_migrations().await.unwrap_or_default(),
            isolation: self.isolation_status.clone(),
            model_providers: self.models.models().len(),
            active_plugins: self.active_plugins.clone(),
            pending_events: pending,
        }
    }

    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        let handles: Vec<JoinHandle<()>> = self
            .background
            .lock()
            .map(|mut b| b.drain(..).collect())
            .unwrap_or_default();
        for h in handles {
            let _ = tokio::time::timeout(Duration::from_secs(10), h).await;
        }
        let _ = self.dispatcher.drain(3).await;
        self.db.close().await;
    }
}

async fn sqlx_ping(db: &Db) -> bool {
    eaeh_store::sqlx::query("SELECT 1")
        .execute(db.pool())
        .await
        .is_ok()
}
