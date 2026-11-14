//! Bridges the deterministic workflow engine to intelligent execution:
//! [`AgentTaskExecutor`] (agents and deterministic tool steps) and [`AgentPlanner`].

use async_trait::async_trait;
use eaeh_agents::{AgentRuntime, AgentTask, CapabilityQuery};
use eaeh_core::{
    AutonomyLevel, HarnessError, Principal, PrincipalId, ProjectAccess, Result, Role, Scope, TaskId,
    Untrusted,
};
use eaeh_tools::{ToolGateway, ToolRequest};
use eaeh_workflow::{
    PlanProposal, Planner, PlanningContext, TaskExecution, TaskExecutor, TaskOutput, TaskSpec,
};
use eaeh_workspace::Provenance;
use serde_json::json;
use std::sync::Arc;

pub const ENGINE_PRINCIPAL: &str = "system:workflow-engine";

/// Service identity used for deterministic tool steps: Operator role, confined to the
/// workflow's own project. It cannot decide approvals (service principals never can).
pub fn engine_principal(scope: Scope) -> Result<Principal> {
    Ok(Principal::service(
        PrincipalId::new(ENGINE_PRINCIPAL)?,
        scope.tenant,
        [Role::Operator].into(),
        ProjectAccess::Only([scope.project].into()),
    ))
}

#[derive(Clone)]
pub struct AgentTaskExecutor {
    agents: AgentRuntime,
    gateway: Arc<ToolGateway>,
}

impl AgentTaskExecutor {
    pub fn new(agents: AgentRuntime, gateway: Arc<ToolGateway>) -> Self {
        Self { agents, gateway }
    }
}

#[async_trait]
impl TaskExecutor for AgentTaskExecutor {
    async fn execute(&self, t: TaskExecution) -> Result<TaskOutput> {
        match &t.spec {
            TaskSpec::Agent {
                task_kind,
                capabilities,
                max_autonomy,
            } => {
                let candidates = self
                    .agents
                    .registry()
                    .discover(&CapabilityQuery {
                        required: capabilities.clone(),
                        max_autonomy: Some(*max_autonomy),
                        ids: None,
                    })
                    .await?;
                let def = candidates.into_iter().next().ok_or_else(|| {
                    HarnessError::not_found(format!(
                        "no active agent provides {capabilities:?} within autonomy {max_autonomy:?}"
                    ))
                })?;
                let fut = self.agents.run(
                    &def,
                    AgentTask {
                        task_id: t.task_id,
                        kind: task_kind.clone(),
                        input: t.input.clone(),
                        scope: t.scope,
                        trace: t.trace,
                        workflow_owner: Some(t.budget_owner.clone()),
                        workspace: Some(t.workspace.clone()),
                    },
                );
                let r = tokio::select! {
                    r = fut => r?,
                    _ = t.cancel.cancelled() => return Err(HarnessError::cancelled("task cancelled")),
                };
                let instance: eaeh_core::AgentInstanceId = r.instance_id.parse()?;
                let author = PrincipalId::for_agent_instance(instance);
                let mut authors = vec![author.clone()];
                // External AI execution: the person who submitted the client's result is a
                // co-author and therefore cannot approve it (separation of duties).
                if let Some(x) = &r.external
                    && let Ok(p) = PrincipalId::new(x.submitted_by.clone())
                    && !authors.contains(&p)
                {
                    authors.push(p);
                }
                let mut inputs = vec![];
                if let Some(x) = &r.external {
                    inputs.push(format!("external-task-package:sha256:{}", x.package_hash));
                    inputs.push(format!("submitted-by:{}", x.submitted_by));
                }
                Ok(TaskOutput {
                    output: r.output,
                    authors,
                    provenance: Provenance {
                        produced_by: author.to_string(),
                        agent_instance: Some(r.instance_id),
                        agent_definition: Some(r.agent),
                        task: Some(t.task_id.to_string()),
                        workflow_instance: Some(t.instance.to_string()),
                        tool_invocations: r.tool_invocations,
                        models: r.models,
                        prompts: r.prompt_refs,
                        context_snapshot: r.context_snapshots.last().cloned(),
                        inputs,
                        skills: r.skill_refs,
                        knowledge: r.knowledge_refs,
                        standards: r.standards,
                        quality_gates: r
                            .validators
                            .iter()
                            .map(|v| format!("agent-output:{v}:passed"))
                            .collect(),
                    },
                })
            }
            TaskSpec::Tool { tool, input } => {
                let principal = engine_principal(t.scope)?;
                let resp = self
                    .gateway
                    .invoke(ToolRequest {
                        principal: principal.clone(),
                        scope: t.scope,
                        tool: tool.clone(),
                        version: None,
                        input: input.clone(),
                        workspace: Some(t.workspace.clone()),
                        budget_owners: vec![t.budget_owner.clone()],
                        approval: None,
                        allowed_tools: None,
                        trace: t.trace,
                    })
                    .await?;
                Ok(TaskOutput {
                    output: resp.output.into_data(),
                    authors: vec![principal.id.clone()],
                    provenance: Provenance {
                        produced_by: principal.id.to_string(),
                        task: Some(t.task_id.to_string()),
                        workflow_instance: Some(t.instance.to_string()),
                        tool_invocations: vec![resp.invocation_id.to_string()],
                        ..Default::default()
                    },
                })
            }
        }
    }
}

/// Planner backed by an agent that declares the `plan` capability. Its output is
/// returned as `Untrusted` and validated by the workflow engine.
#[derive(Clone)]
pub struct AgentPlanner {
    agents: AgentRuntime,
}

impl AgentPlanner {
    pub fn new(agents: AgentRuntime) -> Self {
        Self { agents }
    }
}

#[async_trait]
impl Planner for AgentPlanner {
    async fn propose(&self, ctx: PlanningContext) -> Result<Untrusted<PlanProposal>> {
        let def = self
            .agents
            .registry()
            .discover(&CapabilityQuery {
                required: ["plan".to_string()].into(),
                max_autonomy: Some(AutonomyLevel::Recommend),
                ids: None,
            })
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                HarnessError::not_found(
                    "no planning agent (capability 'plan', autonomy <= recommend) is registered",
                )
            })?;
        let r = self
            .agents
            .run(
                &def,
                AgentTask {
                    task_id: TaskId::new(),
                    kind: "plan".into(),
                    input: json!({
                        "allowed_task_kinds": ctx.allowed_task_kinds,
                        "max_steps": ctx.max_steps,
                        "context": ctx.input,
                    }),
                    scope: ctx.scope,
                    trace: ctx.trace,
                    workflow_owner: Some(ctx.budget_owner),
                    workspace: Some(ctx.workspace),
                },
            )
            .await?;
        let plan: PlanProposal = serde_json::from_value(r.output).map_err(|e| {
            HarnessError::invalid_input(format!("planner output is not a plan proposal: {e}"))
        })?;
        Ok(Untrusted::new(plan))
    }
}
