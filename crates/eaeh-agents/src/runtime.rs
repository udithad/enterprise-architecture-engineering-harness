//! Agent lifecycle, execution loop and delegation.

use crate::definition::AgentDefinition;
use crate::registry::AgentRegistry;
use eaeh_core::hash::{canonical_json, sha256_hex};
use eaeh_core::{
    AgentDefinitionId, AgentInstanceId, ErrorKind, HarnessError, Principal, PromptId, Result, Scope, TaskId,
    TraceContext, Untrusted,
};
use eaeh_events::catalog::{
    AgentCreated, AgentDelegated, AgentExecutionCompleted, AgentExecutionStarted, AgentTerminated,
};
use eaeh_events::{DomainEvent, EventEnvelope, outbox};
use eaeh_extensibility::{HookContext, HookPayload, HookPoint, HookRegistry};
use eaeh_governance::authz::{Action, Authorizer, Target};
use eaeh_governance::{BudgetLedger, BudgetResource};
use eaeh_intelligence::{
    ContextAssembler, ContextItem, MemoryKind, ModelCall, ModelClient, ModelRequest, PromptRegistry,
    RoutingRequirements, ToolSpec, TrustLevel, TrustedText,
};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, WriteTx, sqlx, to_json, ts};
use eaeh_tools::{ToolGateway, ToolRequest};
use eaeh_validation::schema::CompiledSchema;
use eaeh_workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// Platform system prompt included (at `System` trust) when an approved version exists.
pub const SYSTEM_PROMPT_ID: &str = "system.agent-core";
/// Identical tool calls allowed before the loop detector halts the agent.
pub const MAX_IDENTICAL_TOOL_CALLS: u32 = 3;

#[derive(Debug, Clone)]
pub struct AgentTask {
    pub task_id: TaskId,
    pub kind: String,
    pub input: Value,
    pub scope: Scope,
    pub trace: TraceContext,
    /// Budget owner of the enclosing workflow (charged in addition to the instance).
    pub workflow_owner: Option<String>,
    pub workspace: Option<Workspace>,
}

#[derive(Debug, Clone)]
pub struct AgentInstance {
    pub id: AgentInstanceId,
    pub definition: AgentDefinition,
    pub principal: Principal,
    pub scope: Scope,
    pub root: AgentInstanceId,
    pub parent: Option<AgentInstanceId>,
    pub depth: u32,
    /// Definition ids from the root instance to this one (cycle detection).
    pub lineage: Vec<AgentDefinitionId>,
    pub workflow_owner: Option<String>,
}

impl AgentInstance {
    pub fn budget_owner(&self) -> String {
        format!("agent:{}", self.id)
    }

    fn owners(&self) -> Vec<String> {
        let mut o = vec![self.budget_owner()];
        if let Some(w) = &self.workflow_owner {
            o.push(w.clone());
        }
        o
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub instance_id: String,
    pub agent: String,
    /// Schema-validated output (derived from untrusted model output).
    pub output: Value,
    pub steps: u32,
    pub tool_invocations: Vec<String>,
    pub models: Vec<String>,
    pub prompt_refs: Vec<String>,
    pub context_snapshots: Vec<String>,
    pub corrections: u32,
    /// Skills applied (`id@version`), Phase 2.
    #[serde(default)]
    pub skill_refs: Vec<String>,
    /// Knowledge citations available to (retrieved for) this run, Phase 2.
    #[serde(default)]
    pub knowledge_refs: Vec<String>,
    /// Mandatory standards supplied to this run (`src/subject@vN`), Phase 3.
    #[serde(default)]
    pub standards: Vec<String>,
    /// Output validators that passed (`id`), Phase 3.
    #[serde(default)]
    pub validators: Vec<String>,
    /// Set when an external AI client produced the output (ADR-0025).
    #[serde(default)]
    pub external: Option<crate::external::ExternalExecution>,
}

/// Phase 2 knowledge & skills integration (RAG + approved skills).
#[derive(Clone, Debug)]
pub struct KnowledgeIntegration {
    pub knowledge: eaeh_knowledge::KnowledgeService,
    pub skills: eaeh_skills::SkillRegistry,
    /// Hits retrieved per query for automatic context.
    pub k: usize,
}

#[derive(Clone)]
pub struct AgentRuntime {
    db: Db,
    registry: AgentRegistry,
    gateway: Arc<ToolGateway>,
    models: ModelClient,
    prompts: PromptRegistry,
    context: ContextAssembler,
    hooks: Arc<HookRegistry>,
    budgets: BudgetLedger,
    authz: Arc<dyn Authorizer>,
    max_delegation_depth: u32,
    knowledge: Option<KnowledgeIntegration>,
    validation: Option<Arc<eaeh_validation::ValidationEngine>>,
    pub(crate) mode: crate::external::ExecutionMode,
    pub(crate) workspaces: Option<eaeh_workspace::WorkspaceManager>,
}

impl std::fmt::Debug for AgentRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRuntime")
            .field("max_delegation_depth", &self.max_delegation_depth)
            .finish_non_exhaustive()
    }
}

#[allow(clippy::too_many_arguments)]
impl AgentRuntime {
    pub fn new(
        db: Db,
        registry: AgentRegistry,
        gateway: Arc<ToolGateway>,
        models: ModelClient,
        prompts: PromptRegistry,
        context: ContextAssembler,
        hooks: Arc<HookRegistry>,
        budgets: BudgetLedger,
        authz: Arc<dyn Authorizer>,
        max_delegation_depth: u32,
    ) -> Self {
        Self {
            db,
            registry,
            gateway,
            models,
            prompts,
            context,
            hooks,
            budgets,
            authz,
            max_delegation_depth,
            knowledge: None,
            validation: None,
            mode: crate::external::ExecutionMode::DirectProvider,
            workspaces: None,
        }
    }

    /// Select how agent tasks obtain AI reasoning (ADR-0025). In `ExternalAi` mode the
    /// runtime never calls a model: workflow agent tasks are handed off to a connected
    /// AI client and their submitted results validated. `workspaces` lets the client's
    /// governed task-tool calls run in the task's workflow workspace.
    pub fn with_execution_mode(
        mut self,
        mode: crate::external::ExecutionMode,
        workspaces: Option<eaeh_workspace::WorkspaceManager>,
    ) -> Self {
        self.mode = mode;
        self.workspaces = workspaces;
        self
    }

    pub fn execution_mode(&self) -> crate::external::ExecutionMode {
        self.mode
    }

    pub(crate) fn db(&self) -> &Db {
        &self.db
    }
    pub(crate) fn authz(&self) -> &Arc<dyn Authorizer> {
        &self.authz
    }
    pub(crate) fn budgets(&self) -> &BudgetLedger {
        &self.budgets
    }
    pub(crate) fn context(&self) -> &ContextAssembler {
        &self.context
    }
    pub(crate) fn models(&self) -> &ModelClient {
        &self.models
    }
    pub(crate) fn gateway(&self) -> &Arc<ToolGateway> {
        &self.gateway
    }

    /// Enable deterministic output validators declared by agent definitions (Phase 3).
    pub fn with_validation(mut self, v: Arc<eaeh_validation::ValidationEngine>) -> Self {
        self.validation = Some(v);
        self
    }

    /// Run the definition's output validators as a fail-closed gate. `Ok(None)` = passed,
    /// `Ok(Some(feedback))` = blocking findings to feed back to the model.
    pub(crate) async fn domain_validate(
        &self,
        def: &AgentDefinition,
        input: &Value,
        output: &Value,
        standards: &[String],
        retrieved: &BTreeSet<String>,
    ) -> Result<Option<String>> {
        if def.output_validators.is_empty() {
            return Ok(None);
        }
        let engine = self.validation.as_ref().ok_or_else(|| {
            HarnessError::not_supported(format!(
                "agent {} declares output validators but no validation engine is configured",
                def.reference()
            ))
        })?;
        let mut feedback = vec![];
        for r in &def.output_validators {
            let mut ctx = serde_json::Map::new();
            for (k, ptr) in &r.context {
                ctx.insert(k.clone(), input.pointer(ptr).cloned().unwrap_or(Value::Null));
            }
            ctx.entry("standards").or_insert_with(|| json!(standards));
            ctx.entry("knowledge").or_insert_with(|| json!(retrieved));
            let gate = eaeh_validation::QualityGate {
                id: format!("agent-output:{}", r.validator),
                validators: vec![r.validator.clone()],
                block_on: eaeh_validation::Severity::Error,
                semantic_can_block: false,
            };
            let res = engine
                .run_gate(
                    &gate,
                    &eaeh_validation::ValidationSubject {
                        kind: "agent_output".into(),
                        name: def.reference(),
                        content: json!({"output": output, "context": ctx}),
                        metadata: Default::default(),
                    },
                )
                .await;
            for f in res.blocking.iter().take(20) {
                feedback.push(format!(
                    "[{}] {}{}",
                    f.validator,
                    f.location
                        .as_deref()
                        .map(|l| format!("{l}: "))
                        .unwrap_or_default(),
                    f.message
                ));
            }
        }
        Ok(if feedback.is_empty() {
            None
        } else {
            Some(format!(
                "deterministic validation failed ({} findings): {}",
                feedback.len(),
                feedback.join(" | ")
            ))
        })
    }

    /// Enable governed knowledge retrieval (RAG) and approved skills for agent runs.
    pub fn with_knowledge(mut self, k: KnowledgeIntegration) -> Self {
        self.knowledge = Some(k);
        self
    }

    pub fn knowledge(&self) -> Option<&KnowledgeIntegration> {
        self.knowledge.as_ref()
    }

    pub fn registry(&self) -> &AgentRegistry {
        &self.registry
    }

    fn agent_hook(
        &self,
        point: HookPoint,
        inst: &AgentInstance,
        state: &str,
        trace: TraceContext,
    ) -> HookContext {
        HookContext {
            point,
            scope: Some(inst.scope),
            trace,
            actor: Some(inst.principal.id.to_string()),
            subject: inst.definition.id.to_string(),
            payload: HookPayload::Agent {
                definition_id: inst.definition.id.to_string(),
                definition_version: inst.definition.version.to_string(),
                instance_id: Some(inst.id.to_string()),
                state: Some(state.into()),
            },
        }
    }

    async fn emit<E: DomainEvent>(
        &self,
        tx: &mut WriteTx,
        ev: &E,
        scope: Scope,
        trace: TraceContext,
    ) -> Result<()> {
        let env = EventEnvelope::new(ev, "agent-runtime", Some(scope), trace, self.db.now())?;
        outbox::enqueue_in(tx, &env).await.map(|_| ())
    }

    pub(crate) async fn set_state(&self, id: AgentInstanceId, state: &str) -> Result<()> {
        let mut tx = self.db.write().await?;
        sqlx::query("UPDATE agent_instances SET state = ?, updated_at = ? WHERE id = ?")
            .bind(state)
            .bind(ts(self.db.now()))
            .bind(id.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        tx.commit().await
    }

    /// Dynamically instantiate an agent for a task.
    pub async fn instantiate(
        &self,
        def: &AgentDefinition,
        scope: Scope,
        task_id: Option<TaskId>,
        parent: Option<&AgentInstance>,
        workflow_owner: Option<String>,
        trace: TraceContext,
    ) -> Result<AgentInstance> {
        let id = AgentInstanceId::new();
        let depth = parent.map(|p| p.depth + 1).unwrap_or(0);
        let mut lineage = parent.map(|p| p.lineage.clone()).unwrap_or_default();
        lineage.push(def.id.clone());
        let inst = AgentInstance {
            id,
            definition: def.clone(),
            principal: Principal::agent(id, def.id.clone(), def.version.to_string(), def.autonomy, scope),
            scope,
            root: parent.map(|p| p.root).unwrap_or(id),
            parent: parent.map(|p| p.id),
            depth,
            lineage,
            workflow_owner,
        };
        self.hooks
            .dispatch(&self.agent_hook(HookPoint::BeforeAgentInstantiation, &inst, "instantiating", trace))
            .await
            .into_result()?;
        if let Some(w) = &inst.workflow_owner {
            self.budgets
                .consume(&[w.as_str()], BudgetResource::ConcurrentAgents, 1)
                .await?;
        }
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        self.budgets
            .init_in(&mut tx, &inst.budget_owner(), &def.constraints)
            .await?;
        sqlx::query(
            "INSERT INTO agent_instances (id, definition_id, definition_version, tenant_id, project_id, task_id, root_instance_id, \
             parent_instance_id, delegation_depth, lineage, state, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'ready', ?, ?)",
        )
        .bind(id.to_string())
        .bind(def.id.as_str())
        .bind(def.version.to_string())
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(task_id.map(|t| t.to_string()))
        .bind(inst.root.to_string())
        .bind(inst.parent.map(|p| p.to_string()))
        .bind(i64::from(depth))
        .bind(to_json(&inst.lineage)?)
        .bind(&now)
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system("agent-runtime", "agent.instantiate", "agent_instance", id.to_string(), AuditOutcome::Succeeded)
                    .scope(scope)
                    .correlation(trace.correlation)
                    .details(json!({"definition": def.reference(), "autonomy": def.autonomy, "depth": depth, "parent": inst.parent})),
            )
            .await?;
        self.emit(
            &mut tx,
            &AgentCreated {
                instance_id: id.to_string(),
                definition_id: def.id.to_string(),
                definition_version: def.version.to_string(),
                parent_instance_id: inst.parent.map(|p| p.to_string()),
                delegation_depth: depth,
            },
            scope,
            trace,
        )
        .await?;
        tx.commit().await?;
        self.hooks
            .dispatch(&self.agent_hook(HookPoint::AfterAgentInstantiation, &inst, "ready", trace))
            .await;
        Ok(inst)
    }

    /// Terminate an instance (always call, also after failures). Releases concurrency.
    pub async fn terminate(
        &self,
        inst: &AgentInstance,
        final_state: &str,
        trace: TraceContext,
    ) -> Result<()> {
        self.hooks
            .dispatch(&self.agent_hook(HookPoint::BeforeAgentTermination, inst, final_state, trace))
            .await;
        if let Some(w) = &inst.workflow_owner {
            self.budgets
                .release(w, BudgetResource::ConcurrentAgents, 1)
                .await?;
        }
        let mut tx = self.db.write().await?;
        sqlx::query("UPDATE agent_instances SET state = ?, updated_at = ? WHERE id = ?")
            .bind(format!("terminated:{final_state}"))
            .bind(ts(self.db.now()))
            .bind(inst.id.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        self.emit(
            &mut tx,
            &AgentTerminated {
                instance_id: inst.id.to_string(),
                final_state: final_state.into(),
            },
            inst.scope,
            trace,
        )
        .await?;
        tx.commit().await?;
        self.hooks
            .dispatch(&self.agent_hook(HookPoint::AfterAgentTermination, inst, final_state, trace))
            .await;
        Ok(())
    }

    /// Instantiate, execute and terminate in one call.
    pub async fn run(&self, def: &AgentDefinition, task: AgentTask) -> Result<AgentResult> {
        let inst = match self.resume_external_instance(def, &task).await? {
            Some(i) => i,
            None => {
                self.instantiate(
                    def,
                    task.scope,
                    Some(task.task_id),
                    None,
                    task.workflow_owner.clone(),
                    task.trace,
                )
                .await?
            }
        };
        let trace = task.trace;
        let res = self.execute(&inst, task).await;
        if matches!(&res, Err(e) if e.kind == ErrorKind::AwaitingExternal) {
            // Suspended, not terminated: the instance resumes when the client submits.
            if let Some(w) = &inst.workflow_owner {
                self.budgets
                    .release(w, BudgetResource::ConcurrentAgents, 1)
                    .await?;
            }
            return res;
        }
        let state = if res.is_ok() { "completed" } else { "failed" };
        self.terminate(&inst, state, trace).await?;
        res
    }

    async fn instructions(&self, def: &AgentDefinition) -> Result<Vec<ContextItem>> {
        let mut items = vec![];
        if let Ok(id) = PromptId::new(SYSTEM_PROMPT_ID)
            && let Ok(t) = self.prompts.active(&id).await
        {
            items.push(ContextItem::instruction(
                "system",
                &t.render(&BTreeMap::new())?,
                TrustLevel::System,
            )?);
        }
        let t = self.prompts.active(&def.instructions_prompt).await?;
        let available: BTreeMap<&str, String> = [
            ("purpose", def.purpose.clone()),
            ("responsibilities", def.responsibilities.join("; ")),
            ("agent_id", def.id.to_string()),
        ]
        .into();
        let vars = t
            .variables
            .iter()
            .filter_map(|v| {
                available
                    .get(v.name.as_str())
                    .map(|x| (v.name.clone(), TrustedText::new(x.clone())))
            })
            .collect();
        items.push(ContextItem::instruction(
            "agent",
            &t.render(&vars)?,
            TrustLevel::AgentInstruction,
        )?);
        Ok(items)
    }

    /// Governed execution loop.
    /// Runs inside an `agent` span carrying instance, definition and correlation ids.
    pub async fn execute(&self, inst: &AgentInstance, task: AgentTask) -> Result<AgentResult> {
        use tracing::Instrument;
        let span = tracing::info_span!(
            "agent",
            instance = %inst.id,
            definition = %inst.definition.reference(),
            correlation_id = %task.trace.correlation
        );
        self.execute_inner(inst, task).instrument(span).await
    }

    async fn execute_inner(&self, inst: &AgentInstance, task: AgentTask) -> Result<AgentResult> {
        let def = &inst.definition;
        let trace = task.trace;
        self.guard_external_handoff(&task).await?;
        CompiledSchema::compile(&def.input_schema)?.check(&task.input, "agent task input")?;
        let output_schema = CompiledSchema::compile(&def.output_schema)?;
        self.set_state(inst.id, "running").await?;
        {
            let mut tx = self.db.write().await?;
            self.emit(
                &mut tx,
                &AgentExecutionStarted {
                    instance_id: inst.id.to_string(),
                    task_id: Some(task.task_id.to_string()),
                },
                inst.scope,
                trace,
            )
            .await?;
            tx.commit().await?;
        }
        self.hooks
            .dispatch(&self.agent_hook(HookPoint::BeforeAgentExecution, inst, "running", trace))
            .await
            .into_result()?;

        let mut instructions = self.instructions(def).await?;
        // ---- Phase 2: approved skills, mandatory standards, retrieved knowledge ----
        let mut knowledge_items: Vec<ContextItem> = vec![];
        let mut skill_refs: Vec<String> = vec![];
        let mut skill_output_schemas: Vec<(String, CompiledSchema)> = vec![];
        let mut retrieved: BTreeSet<String> = BTreeSet::new();
        let mut standard_refs: Vec<String> = vec![];
        match &self.knowledge {
            Some(k) => {
                let resolution = k
                    .skills
                    .resolve(&eaeh_skills::Binding {
                        explicit: &def.skills,
                        allowed_tools: &def.allowed_tools,
                        autonomy: def.autonomy,
                        task_kind: &task.kind,
                    })
                    .await?;
                for s in &resolution.skills {
                    CompiledSchema::compile(&s.definition.input_schema)?
                        .check(&task.input, &format!("task input for skill {}", s.reference))?;
                    skill_output_schemas.push((
                        s.reference.clone(),
                        CompiledSchema::compile(&s.definition.output_schema)?,
                    ));
                }
                if !resolution.skipped.is_empty() {
                    tracing::info!(skipped = ?resolution.skipped, "dynamically matched skills not applied");
                }
                skill_refs = resolution.references();
                let rctx = k
                    .knowledge
                    .context_for_agent(&inst.principal, inst.scope, Some(task.task_id))
                    .await?;
                let standards = k.knowledge.standards_context(&rctx).await?;
                for st in &standards {
                    if let Some(c) = st.key().strip_prefix("standard:") {
                        retrieved.insert(c.to_string());
                        standard_refs.push(c.to_string());
                    }
                }
                instructions.extend(standards);
                instructions.extend(resolution.instruction_items());
                let input_text: String = canonical_json(&task.input).chars().take(400).collect();
                let mut queries = vec![format!("{} {input_text}", task.kind)];
                for s in &resolution.skills {
                    queries.extend(s.definition.required_knowledge.iter().map(|r| r.query.clone()));
                }
                let mut hits: Vec<eaeh_knowledge::SearchHit> = vec![];
                for q in queries {
                    let found = k
                        .knowledge
                        .search(
                            &rctx,
                            &eaeh_knowledge::SearchQuery::new(&q, eaeh_knowledge::SearchMode::Hybrid, k.k),
                            trace,
                        )
                        .await?;
                    for h in found {
                        if retrieved.insert(h.citation.clone()) {
                            hits.push(h);
                        }
                    }
                }
                knowledge_items = eaeh_knowledge::KnowledgeService::excerpts(&hits, inst.scope);
            }
            None if !def.skills.is_empty() => {
                return Err(HarnessError::not_supported(format!(
                    "agent {} references skills but no skills registry is configured",
                    def.reference()
                )));
            }
            None => {}
        }
        let tool_specs: Vec<ToolSpec> = def
            .allowed_tools
            .iter()
            .filter_map(|t| self.gateway.registry().descriptor(t).ok())
            .map(|d| ToolSpec {
                name: d.id.to_string(),
                description: d.description,
                input_schema: d.input_schema,
            })
            .collect();
        let owners = inst.owners();
        let mut transcript: Vec<ContextItem> = vec![];
        let mut result = AgentResult {
            instance_id: inst.id.to_string(),
            agent: def.reference(),
            output: Value::Null,
            steps: 0,
            tool_invocations: vec![],
            models: vec![],
            prompt_refs: vec![],
            context_snapshots: vec![],
            corrections: 0,
            skill_refs: skill_refs.clone(),
            knowledge_refs: vec![],
            standards: standard_refs.clone(),
            validators: vec![],
            external: None,
        };
        let mut seen_calls: HashMap<String, u32> = HashMap::new();

        let outcome: Result<Value> = if self.mode.is_external() {
            self.external_outcome(
                inst,
                &task,
                crate::external::Prepared {
                    instructions: &instructions,
                    knowledge_items: &knowledge_items,
                    tool_specs: &tool_specs,
                    output_schema: &output_schema,
                    skill_output_schemas: &skill_output_schemas,
                    retrieved: &mut retrieved,
                    standard_refs: &standard_refs,
                    owners: &owners,
                },
                &mut result,
            )
            .await
        } else {
            loop {
                if let Err(e) = self
                    .budgets
                    .consume(&[&inst.budget_owner()], BudgetResource::AgentSteps, 1)
                    .await
                {
                    break Err(e);
                }
                result.steps += 1;
                let mut items = instructions.clone();
                items.push(ContextItem::task_input(
                    "task",
                    canonical_json(&task.input),
                    inst.scope,
                ));
                // The registered output contract, so the model knows the exact shape expected
                // (platform configuration, not model content).
                items.push(ContextItem::task_input(
                    "output_schema",
                    format!(
                        "The final answer must be one JSON object valid against this JSON Schema: {}",
                        canonical_json(&def.output_schema)
                    ),
                    inst.scope,
                ));
                items.extend(knowledge_items.iter().cloned());
                items.extend(transcript.iter().cloned());
                let ctx = self.context.assemble(inst.scope, trace, items).await?;
                result.context_snapshots.push(ctx.snapshot_hash.clone());
                for r in &ctx.prompt_refs {
                    if !result.prompt_refs.contains(r) {
                        result.prompt_refs.push(r.clone());
                    }
                }
                let call = ModelCall {
                    scope: inst.scope,
                    trace,
                    agent_instance: Some(inst.id),
                    budget_owners: owners.clone(),
                    requirements: RoutingRequirements {
                        capabilities: def.model.capabilities.clone(),
                        min_quality: def.model.min_quality,
                        preferred: def.model.preferred.clone(),
                        ..Default::default()
                    },
                    request: ModelRequest {
                        messages: ctx.to_messages(&format!("Task kind: {}", task.kind)),
                        tools: tool_specs.clone(),
                        max_output_tokens: def.model.max_output_tokens,
                        temperature: None,
                    },
                    prompt_refs: ctx.prompt_refs.clone(),
                    context_snapshot: Some(ctx.snapshot_hash.clone()),
                    max_classification: ctx.max_classification,
                    skill_refs: skill_refs.clone(),
                    knowledge_refs: retrieved.iter().cloned().collect(),
                };
                let out = match self.models.complete(call).await {
                    Ok(o) => o,
                    Err(e) => break Err(e),
                };
                let model_ref = format!(
                    "{}/{}@{}",
                    out.model.provider,
                    out.model.id,
                    out.response.as_data().model_version
                );
                if !result.models.contains(&model_ref) {
                    result.models.push(model_ref);
                }
                let response = out.response.into_data();
                let step = result.steps;

                if !response.tool_calls.is_empty() {
                    transcript.push(ContextItem::untrusted(
                        &format!("step{step}:assistant"),
                        Untrusted::new(response.content.clone()),
                        "model",
                        MemoryKind::Execution,
                        inst.scope,
                    ));
                    let mut fatal = None;
                    for (i, tc) in response.tool_calls.iter().enumerate() {
                        let fp = sha256_hex(format!("{}\n{}", tc.tool, canonical_json(&tc.input)).as_bytes());
                        let n = seen_calls.entry(fp).or_insert(0);
                        *n += 1;
                        if *n > MAX_IDENTICAL_TOOL_CALLS {
                            fatal = Some(HarnessError::supervision_halt(format!(
                                "loop detected: identical call to '{}' repeated {} times",
                                tc.tool, *n
                            )));
                            break;
                        }
                        let tool = match eaeh_core::ToolId::new(tc.tool.clone()) {
                            Ok(t) => t,
                            Err(e) => {
                                transcript.push(tool_feedback(
                                    step,
                                    i,
                                    &tc.tool,
                                    &format!("error: {e}"),
                                    inst.scope,
                                ));
                                continue;
                            }
                        };
                        let r = self
                            .gateway
                            .invoke(ToolRequest {
                                principal: inst.principal.clone(),
                                scope: inst.scope,
                                tool,
                                version: None,
                                input: tc.input.clone(),
                                workspace: task.workspace.clone(),
                                budget_owners: owners.clone(),
                                approval: None,
                                allowed_tools: Some(def.allowed_tools.clone()),
                                trace,
                            })
                            .await;
                        match r {
                            Ok(resp) => {
                                result.tool_invocations.push(resp.invocation_id.to_string());
                                if (tc.tool == "knowledge.search" || tc.tool == "knowledge.read")
                                    && let Some(hits) =
                                        resp.output.as_data().get("hits").and_then(Value::as_array)
                                {
                                    for c in hits
                                        .iter()
                                        .filter_map(|h| h.get("citation").and_then(Value::as_str))
                                    {
                                        retrieved.insert(c.to_string());
                                    }
                                }
                                let text = canonical_json(resp.output.as_data());
                                transcript.push(tool_feedback(step, i, &tc.tool, &text, inst.scope));
                            }
                            Err(e)
                                if matches!(
                                    e.kind,
                                    ErrorKind::BudgetExceeded
                                        | ErrorKind::ApprovalRequired
                                        | ErrorKind::SupervisionHalt
                                ) =>
                            {
                                fatal = Some(e);
                                break;
                            }
                            Err(e) => {
                                // Denials and tool errors are reported back to the model as data.
                                transcript.push(tool_feedback(
                                    step,
                                    i,
                                    &tc.tool,
                                    &format!("error ({}): {}", e.kind, e.message),
                                    inst.scope,
                                ));
                            }
                        }
                    }
                    if let Some(e) = fatal {
                        break Err(e);
                    }
                    continue;
                }

                // Final answer: must be JSON matching the output schema.
                let parsed: std::result::Result<Value, String> =
                    serde_json::from_str(response.content.trim())
                        .map_err(|e| format!("output is not valid JSON: {e}"));
                let checked = parsed.and_then(|v| {
                    output_schema.check(&v, "agent output").map_err(|e| e.message)?;
                    for (r, schema) in &skill_output_schemas {
                        schema
                            .check(&v, &format!("output contract of skill {r}"))
                            .map_err(|e| e.message)?;
                    }
                    validate_citations(&v, &retrieved)?;
                    Ok(v)
                });
                let checked = match checked {
                    Ok(v) => match self
                        .domain_validate(def, &task.input, &v, &standard_refs, &retrieved)
                        .await
                    {
                        Ok(None) => Ok(v),
                        Ok(Some(msg)) => Err(msg),
                        Err(e) => break Err(e),
                    },
                    Err(m) => Err(m),
                };
                match checked {
                    Ok(v) => break Ok(v),
                    Err(msg) => {
                        if let Err(e) = self
                            .budgets
                            .consume(&[&inst.budget_owner()], BudgetResource::CorrectionAttempts, 1)
                            .await
                        {
                            break Err(HarnessError::supervision_halt(format!(
                                "output invalid and correction budget exhausted: {msg}"
                            ))
                            .context(e.message));
                        }
                        result.corrections += 1;
                        transcript.push(ContextItem::untrusted(
                            &format!("step{step}:assistant"),
                            Untrusted::new(response.content),
                            "model",
                            MemoryKind::Execution,
                            inst.scope,
                        ));
                        transcript.push(ContextItem::task_input(
                        &format!("step{step}:validation"),
                        format!("Platform validation feedback: {msg}. Respond with JSON matching the output schema."),
                        inst.scope,
                    ));
                    }
                }
            }
        };
        if let Err(e) = &outcome
            && e.kind == ErrorKind::AwaitingExternal
        {
            self.set_state(inst.id, "awaiting_external").await?;
            self.hooks
                .dispatch(&self.agent_hook(HookPoint::AfterAgentExecution, inst, "awaiting_external", trace))
                .await;
            return Err(e.clone());
        }

        let succeeded = outcome.is_ok();
        result.knowledge_refs = retrieved.iter().cloned().collect();
        if succeeded {
            result.validators = def
                .output_validators
                .iter()
                .map(|v| v.validator.clone())
                .collect();
        }
        {
            let mut tx = self.db.write().await?;
            self.emit(
                &mut tx,
                &AgentExecutionCompleted {
                    instance_id: inst.id.to_string(),
                    task_id: Some(task.task_id.to_string()),
                    succeeded,
                    steps: result.steps,
                },
                inst.scope,
                trace,
            )
            .await?;
            self.db
                .audit_in(
                    &mut tx,
                    AuditEntry::new(
                        &inst.principal,
                        "agent.execute",
                        "agent_instance",
                        inst.id.to_string(),
                        if succeeded {
                            AuditOutcome::Succeeded
                        } else {
                            AuditOutcome::Failed
                        },
                    )
                    .scope(inst.scope)
                    .correlation(trace.correlation)
                    .details(json!({
                        "task_id": task.task_id.to_string(),
                        "steps": result.steps,
                        "tool_invocations": result.tool_invocations,
                        "models": result.models,
                        "prompts": result.prompt_refs,
                        "corrections": result.corrections,
                        "skills": result.skill_refs,
                        "knowledge": result.knowledge_refs,
                        "error": outcome.as_ref().err().map(|e| e.to_string()),
                    })),
                )
                .await?;
            tx.commit().await?;
        }
        self.hooks
            .dispatch(&self.agent_hook(
                HookPoint::AfterAgentExecution,
                inst,
                if succeeded { "completed" } else { "failed" },
                trace,
            ))
            .await;
        self.set_state(inst.id, if succeeded { "completed" } else { "failed" })
            .await?;
        result.output = outcome?;
        Ok(result)
    }

    /// Controlled delegation from `parent` to an agent definition.
    pub async fn delegate(
        &self,
        parent: &AgentInstance,
        target: &AgentDefinitionId,
        task: AgentTask,
    ) -> Result<AgentResult> {
        let trace = task.trace;
        let check = self.check_delegation(parent, target, &task).await;
        let fingerprint = match check {
            Ok(fp) => fp,
            Err(e) => {
                self.db
                    .audit(
                        AuditEntry::new(
                            &parent.principal,
                            "agent.delegate",
                            "agent_definition",
                            target.to_string(),
                            AuditOutcome::Denied,
                        )
                        .scope(parent.scope)
                        .correlation(trace.correlation)
                        .details(json!({"reason": e.to_string()})),
                    )
                    .await?;
                return Err(e);
            }
        };
        let def = self.registry.latest(target).await?;
        let child = self
            .instantiate(
                &def,
                parent.scope,
                Some(task.task_id),
                Some(parent),
                parent.workflow_owner.clone(),
                trace,
            )
            .await?;
        {
            let mut tx = self.db.write().await?;
            sqlx::query(
                "INSERT INTO agent_messages (id, from_instance, to_instance, kind, task_fingerprint, root_instance_id, payload_hash, correlation_id, created_at) \
                 VALUES (?, ?, ?, 'delegate', ?, ?, ?, ?, ?)",
            )
            .bind(eaeh_core::InvocationId::new().to_string())
            .bind(parent.id.to_string())
            .bind(child.id.to_string())
            .bind(&fingerprint)
            .bind(parent.root.to_string())
            .bind(sha256_hex(canonical_json(&task.input).as_bytes()))
            .bind(trace.correlation.to_string())
            .bind(ts(self.db.now()))
            .execute(tx.conn())
            .await
            .db()?;
            self.emit(
                &mut tx,
                &AgentDelegated {
                    from_instance_id: parent.id.to_string(),
                    to_instance_id: child.id.to_string(),
                    task_fingerprint: fingerprint.clone(),
                },
                parent.scope,
                trace,
            )
            .await?;
            self.db
                .audit_in(
                    &mut tx,
                    AuditEntry::new(
                        &parent.principal,
                        "agent.delegate",
                        "agent_instance",
                        child.id.to_string(),
                        AuditOutcome::Allowed,
                    )
                    .scope(parent.scope)
                    .correlation(trace.correlation)
                    .details(
                        json!({"target": def.reference(), "depth": child.depth, "fingerprint": fingerprint}),
                    ),
                )
                .await?;
            tx.commit().await?;
        }
        let res = self.execute(&child, task).await;
        self.terminate(&child, if res.is_ok() { "completed" } else { "failed" }, trace)
            .await?;
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO agent_messages (id, from_instance, to_instance, kind, task_fingerprint, root_instance_id, payload_hash, correlation_id, created_at) \
             VALUES (?, ?, ?, 'result', ?, ?, ?, ?, ?)",
        )
        .bind(eaeh_core::InvocationId::new().to_string())
        .bind(child.id.to_string())
        .bind(parent.id.to_string())
        .bind(&fingerprint)
        .bind(parent.root.to_string())
        .bind(sha256_hex(canonical_json(&res.as_ref().map(|r| r.output.clone()).unwrap_or(Value::Null)).as_bytes()))
        .bind(trace.correlation.to_string())
        .bind(ts(self.db.now()))
        .execute(tx.conn())
        .await
        .db()?;
        tx.commit().await?;
        res
    }

    async fn check_delegation(
        &self,
        parent: &AgentInstance,
        target: &AgentDefinitionId,
        task: &AgentTask,
    ) -> Result<String> {
        self.authz.authorize(
            &parent.principal,
            Action::AgentDelegate,
            Target::Scope(parent.scope),
        )?;
        if task.scope != parent.scope {
            return Err(HarnessError::forbidden(
                "delegated task must stay in the parent's scope",
            ));
        }
        if !parent.definition.delegation.may_delegate_to.contains(target) {
            return Err(HarnessError::forbidden(format!(
                "{} may not delegate to {target}",
                parent.definition.id
            )));
        }
        if parent.lineage.contains(target) {
            return Err(HarnessError::supervision_halt(format!(
                "circular delegation: {target} already in chain {:?}",
                parent.lineage.iter().map(|d| d.to_string()).collect::<Vec<_>>()
            )));
        }
        if parent.depth + 1 > self.max_delegation_depth {
            return Err(HarnessError::budget_exceeded(format!(
                "delegation depth limit {} reached",
                self.max_delegation_depth
            )));
        }
        let (children,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM agent_instances WHERE parent_instance_id = ?")
                .bind(parent.id.to_string())
                .fetch_one(self.db.pool())
                .await
                .db()?;
        if children >= i64::from(parent.definition.delegation.max_children) {
            return Err(HarnessError::budget_exceeded(format!(
                "fan-out limit {} reached for {}",
                parent.definition.delegation.max_children, parent.id
            )));
        }
        let fingerprint =
            sha256_hex(format!("{target}\n{}\n{}", task.kind, canonical_json(&task.input)).as_bytes());
        let (dups,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_messages WHERE root_instance_id = ? AND task_fingerprint = ? AND kind = 'delegate'",
        )
        .bind(parent.root.to_string())
        .bind(&fingerprint)
        .fetch_one(self.db.pool())
        .await
        .db()?;
        if dups > 0 {
            return Err(HarnessError::conflict(
                "duplicate delegated task within the same execution tree",
            ));
        }
        Ok(fingerprint)
    }
}

/// Deterministic citation validator: every entry of an output's `citations` array must
/// be a citation retrieved in this run (chunk citation `src/subject@vN#i`, or the
/// document-level base `src/subject@vN` of a retrieved chunk or mandatory standard).
pub fn validate_citations(output: &Value, retrieved: &BTreeSet<String>) -> std::result::Result<(), String> {
    let Some(cites) = output.get("citations") else {
        return Ok(());
    };
    let Some(cites) = cites.as_array() else {
        return Err("'citations' must be an array of strings".into());
    };
    let bases: BTreeSet<&str> = retrieved
        .iter()
        .map(|c| c.split('#').next().unwrap_or(c))
        .collect();
    let unknown: Vec<String> = cites
        .iter()
        .map(|c| {
            c.as_str()
                .map(String::from)
                .ok_or_else(|| "citations must be strings".to_string())
        })
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|c| !retrieved.contains(c) && !bases.contains(c.as_str()))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "output cites sources that were not retrieved in this run: {}",
            unknown.join(", ")
        ))
    }
}

fn tool_feedback(step: u32, i: usize, tool: &str, text: &str, scope: Scope) -> ContextItem {
    ContextItem::untrusted(
        &format!("step{step}:tool{i}:{tool}"),
        Untrusted::new(text.to_string()),
        "tool",
        MemoryKind::Execution,
        scope,
    )
}
