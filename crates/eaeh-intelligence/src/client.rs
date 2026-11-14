//! Governed model client: the only path from harness code to a model provider.

use crate::estimate_tokens;
use crate::model::Egress;
use crate::model::{ModelDescriptor, ModelRegistry, ModelRequest, ModelResponse, RoutingRequirements};
use eaeh_core::{AgentInstanceId, Classification, HarnessError, Result, Scope, TraceContext, Untrusted};
use eaeh_events::catalog::ModelInvoked;
use eaeh_events::{EventEnvelope, outbox};
use eaeh_extensibility::{HookContext, HookPayload, HookPoint, HookRegistry};
use eaeh_governance::policy::{Effect, PolicyEngine, PolicyRequest};
use eaeh_governance::{BudgetLedger, BudgetResource};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, sqlx, to_json, ts};
use std::sync::Arc;
use std::time::Duration;

/// Provider id under which hand-offs to an external AI client are policy-checked.
pub const EXTERNAL_AI_CLIENT: &str = "external-ai-client";

#[derive(Debug, Clone)]
pub struct ModelCall {
    pub scope: Scope,
    pub trace: TraceContext,
    pub agent_instance: Option<AgentInstanceId>,
    pub budget_owners: Vec<String>,
    pub requirements: RoutingRequirements,
    pub request: ModelRequest,
    /// `prompt-id@version` of every template used to build the request.
    pub prompt_refs: Vec<String>,
    pub context_snapshot: Option<String>,
    /// Highest data classification present in the request (egress policy input).
    pub max_classification: Classification,
    /// `skill-id@version` of every skill applied (Phase 2 traceability).
    pub skill_refs: Vec<String>,
    /// Knowledge citations included in the request.
    pub knowledge_refs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModelOutcome {
    pub response: Untrusted<ModelResponse>,
    pub model: ModelDescriptor,
    /// Failed attempts before success: (model id, error).
    pub fallbacks: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ModelClient {
    db: Db,
    registry: Arc<ModelRegistry>,
    budgets: BudgetLedger,
    hooks: Arc<HookRegistry>,
    policy: Arc<dyn PolicyEngine>,
    timeout: Duration,
}

impl ModelClient {
    pub fn new(
        db: Db,
        registry: Arc<ModelRegistry>,
        budgets: BudgetLedger,
        hooks: Arc<HookRegistry>,
        policy: Arc<dyn PolicyEngine>,
        timeout: Duration,
    ) -> Self {
        Self {
            db,
            registry,
            budgets,
            hooks,
            policy,
            timeout,
        }
    }

    /// Deterministic egress decision for sending this call's content to a provider.
    fn egress_decision(&self, call: &ModelCall, provider: &str, egress: Egress) -> (Effect, Vec<String>) {
        let d = self.policy.evaluate(&PolicyRequest {
            action: "model.invoke".into(),
            principal_id: call
                .agent_instance
                .map(|a| format!("agent:{a}"))
                .unwrap_or_else(|| "system:model-client".into()),
            principal_kind: if call.agent_instance.is_some() {
                "agent".into()
            } else {
                "service".into()
            },
            autonomy: None,
            risk: None,
            resource_type: "model_provider".into(),
            resource_id: provider.into(),
            scope: Some(call.scope),
            attributes: [
                ("egress".to_string(), egress.as_str().to_string()),
                (
                    "classification".to_string(),
                    call.max_classification.as_str().to_string(),
                ),
            ]
            .into(),
        });
        (d.effect, d.matched_rules)
    }

    pub fn registry(&self) -> &Arc<ModelRegistry> {
        &self.registry
    }

    /// Egress decision for handing task content to an external AI client (`external_ai`
    /// execution mode, ADR-0025). The content leaves EAEH exactly as it would for an
    /// external provider, so the same `model.invoke` policy applies (`egress = external`,
    /// provider `external-ai-client`). Denials are audited and returned as `policy_denied`.
    pub async fn authorize_external_handoff(
        &self,
        scope: Scope,
        trace: TraceContext,
        agent_instance: AgentInstanceId,
        classification: Classification,
    ) -> Result<()> {
        let call = ModelCall {
            scope,
            trace,
            agent_instance: Some(agent_instance),
            budget_owners: vec![],
            requirements: RoutingRequirements::default(),
            request: ModelRequest {
                messages: vec![],
                tools: vec![],
                max_output_tokens: 0,
                temperature: None,
            },
            prompt_refs: vec![],
            context_snapshot: None,
            max_classification: classification,
            skill_refs: vec![],
            knowledge_refs: vec![],
        };
        let (effect, rules) = self.egress_decision(&call, EXTERNAL_AI_CLIENT, Egress::External);
        if effect == Effect::Permit {
            return Ok(());
        }
        self.db
            .audit(
                AuditEntry::system(
                    "model-client",
                    "model.invoke",
                    "model_provider",
                    EXTERNAL_AI_CLIENT,
                    AuditOutcome::Denied,
                )
                .scope(scope)
                .correlation(trace.correlation)
                .details(serde_json::json!({
                    "egress": "external",
                    "classification": classification.as_str(),
                    "policy_rules": rules,
                })),
            )
            .await?;
        Err(HarnessError::policy_denied(format!(
            "policy forbids handing {classification} content to an external AI client"
        )))
    }

    pub async fn complete(&self, call: ModelCall) -> Result<ModelOutcome> {
        let candidates = self.registry.route(&call.requirements);
        if candidates.is_empty() {
            return Err(HarnessError::unavailable(
                "no registered model satisfies the routing requirements",
            ));
        }
        if call.budget_owners.is_empty() {
            return Err(HarnessError::budget_exceeded(
                "model calls require a budget owner",
            ));
        }
        let owners: Vec<&str> = call.budget_owners.iter().map(String::as_str).collect();
        self.budgets
            .consume(&owners, BudgetResource::ModelCalls, 1)
            .await?;

        let est_input: u64 = call
            .request
            .messages
            .iter()
            .map(|m| estimate_tokens(&m.content))
            .sum();
        let mut fallbacks = vec![];
        let mut denied_by_policy: Vec<String> = vec![];
        for model in candidates {
            if !self.registry.is_available(&model.provider) {
                fallbacks.push((model.id.to_string(), "provider circuit open".into()));
                continue;
            }
            if est_input + u64::from(call.request.max_output_tokens) > model.context_window {
                fallbacks.push((model.id.to_string(), "context window too small".into()));
                continue;
            }
            let Some(provider) = self.registry.provider(&model.provider) else {
                continue;
            };
            let (effect, rules) = self.egress_decision(&call, model.provider.as_str(), provider.egress());
            if effect != Effect::Permit {
                self.db
                    .audit(
                        AuditEntry::system(
                            "model-client",
                            "model.invoke",
                            "model_provider",
                            model.provider.to_string(),
                            AuditOutcome::Denied,
                        )
                        .scope(call.scope)
                        .correlation(call.trace.correlation)
                        .details(serde_json::json!({
                            "model": model.id.to_string(),
                            "egress": provider.egress().as_str(),
                            "classification": call.max_classification.as_str(),
                            "policy_rules": rules,
                        })),
                    )
                    .await?;
                denied_by_policy.push(model.id.to_string());
                fallbacks.push((model.id.to_string(), "egress denied by policy".into()));
                continue;
            }
            let hook = |point, usage: Option<crate::Usage>, error: Option<String>| HookContext {
                point,
                scope: Some(call.scope),
                trace: call.trace,
                actor: call.agent_instance.map(|a| a.to_string()),
                subject: model.id.to_string(),
                payload: HookPayload::Model {
                    provider_id: model.provider.to_string(),
                    model_id: model.id.to_string(),
                    input_tokens: usage.map(|u| u.input_tokens),
                    output_tokens: usage.map(|u| u.output_tokens),
                    error,
                },
            };
            self.hooks
                .dispatch(&hook(HookPoint::BeforeModelRequest, None, None))
                .await
                .into_result()?;
            let res =
                match tokio::time::timeout(self.timeout, provider.complete(&model.id, &call.request)).await {
                    Ok(r) => r,
                    Err(_) => Err(HarnessError::timeout(format!("model {} timed out", model.id))),
                };
            match res {
                Ok(resp) => {
                    self.registry.record_success(&model.provider);
                    self.hooks
                        .dispatch(&hook(HookPoint::AfterModelRequest, Some(resp.usage), None))
                        .await;
                    self.record_usage(&call, &model, &resp, "succeeded").await?;
                    let tokens = resp.usage.input_tokens + resp.usage.output_tokens;
                    // Usage already happened; charge it, then stop if it exceeded budget.
                    self.budgets
                        .consume(&owners, BudgetResource::Tokens, tokens)
                        .await?;
                    self.budgets
                        .consume(&owners, BudgetResource::CostMicros, model.cost_micros(resp.usage))
                        .await?;
                    self.hooks
                        .dispatch(&hook(HookPoint::BeforeResponseProcessing, Some(resp.usage), None))
                        .await
                        .into_result()?;
                    let mut resp = resp;
                    resp.content = self.db.redactor().redact_str(&resp.content);
                    self.hooks
                        .dispatch(&hook(HookPoint::AfterResponseProcessing, Some(resp.usage), None))
                        .await;
                    return Ok(ModelOutcome {
                        response: Untrusted::new(resp),
                        model,
                        fallbacks,
                    });
                }
                Err(e) => {
                    self.hooks
                        .dispatch(&hook(HookPoint::AfterModelRequest, None, Some(e.to_string())))
                        .await;
                    tracing::warn!(model = %model.id, error = %e, "model call failed");
                    if e.is_retryable() {
                        self.registry.record_failure(&model.provider);
                        fallbacks.push((model.id.to_string(), e.to_string()));
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        if !denied_by_policy.is_empty() && denied_by_policy.len() == fallbacks.len() {
            return Err(HarnessError::policy_denied(format!(
                "model egress denied by policy for {} content (models: {})",
                call.max_classification,
                denied_by_policy.join(", ")
            )));
        }
        Err(HarnessError::unavailable(format!(
            "all candidate models failed: {}",
            fallbacks
                .iter()
                .map(|(m, e)| format!("{m}: {e}"))
                .collect::<Vec<_>>()
                .join("; ")
        )))
    }

    async fn record_usage(
        &self,
        call: &ModelCall,
        model: &ModelDescriptor,
        resp: &ModelResponse,
        outcome: &str,
    ) -> Result<()> {
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO model_usage (id, tenant_id, project_id, provider_id, model_id, model_version, input_tokens, output_tokens, \
             cost_micros, outcome, agent_instance_id, correlation_id, context_snapshot, prompt_refs, occurred_at, skill_refs, knowledge_refs) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(eaeh_core::InvocationId::new().to_string())
        .bind(call.scope.tenant.to_string())
        .bind(call.scope.project.to_string())
        .bind(model.provider.to_string())
        .bind(model.id.to_string())
        .bind(&resp.model_version)
        .bind(i64::try_from(resp.usage.input_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(resp.usage.output_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(model.cost_micros(resp.usage)).unwrap_or(i64::MAX))
        .bind(outcome)
        .bind(call.agent_instance.map(|a| a.to_string()))
        .bind(call.trace.correlation.to_string())
        .bind(&call.context_snapshot)
        .bind(to_json(&call.prompt_refs)?)
        .bind(ts(self.db.now()))
        .bind(to_json(&call.skill_refs)?)
        .bind(to_json(&call.knowledge_refs)?)
        .execute(tx.conn())
        .await
        .db()?;
        let ev = EventEnvelope::new(
            &ModelInvoked {
                provider_id: model.provider.to_string(),
                model_id: model.id.to_string(),
                model_version: resp.model_version.clone(),
                input_tokens: resp.usage.input_tokens,
                output_tokens: resp.usage.output_tokens,
                succeeded: true,
            },
            "model-client",
            Some(call.scope),
            call.trace,
            self.db.now(),
        )?;
        outbox::enqueue_in(&mut tx, &ev).await?;
        tx.commit().await
    }
}
