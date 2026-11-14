//! The governed tool gateway.

use crate::registry::{Entry, ToolContext, ToolRegistry};
use async_trait::async_trait;
use eaeh_core::hash::{canonical_json, sha256_hex};
use eaeh_core::{
    ApprovalId, HarnessError, InvocationId, Principal, Result, Scope, ToolId, TraceContext, Untrusted,
};
use eaeh_events::catalog::ToolInvoked;
use eaeh_events::{EventEnvelope, outbox};
use eaeh_extensibility::interceptor::InterceptorChain;
use eaeh_extensibility::{Endpoint, HookContext, HookPayload, HookPoint, HookRegistry, Pipeline};
use eaeh_governance::authz::{Action, Authorizer, Target};
use eaeh_governance::policy::{Effect, PolicyEngine, PolicyRequest};
use eaeh_governance::{ApprovalManager, ApprovalRequest, ApprovalSubject, BudgetLedger, BudgetResource};
use eaeh_isolation::ProviderRegistry;
use eaeh_store::Db;
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_workspace::Workspace;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ToolRequest {
    pub principal: Principal,
    pub scope: Scope,
    pub tool: ToolId,
    pub version: Option<semver::VersionReq>,
    pub input: Value,
    pub workspace: Option<Workspace>,
    /// Budget owners charged for this call (e.g. agent instance + workflow instance).
    pub budget_owners: Vec<String>,
    /// Previously granted approval for exactly this invocation, if any.
    pub approval: Option<ApprovalId>,
    /// Tool allowlist from the calling agent's definition (`None` for non-agents).
    pub allowed_tools: Option<Vec<ToolId>>,
    pub trace: TraceContext,
}

#[derive(Debug, Clone)]
pub struct ToolResponse {
    pub invocation_id: InvocationId,
    pub tool_id: ToolId,
    pub tool_version: String,
    /// Tool output is always untrusted data (master §22).
    pub output: Untrusted<Value>,
    pub duration_ms: u64,
    pub hooks: Vec<String>,
    pub policy_rules: Vec<String>,
    pub provider: Option<String>,
}

/// Request flowing through the tool middleware pipeline.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub descriptor: Arc<crate::ToolDescriptor>,
    pub ctx: ToolContext,
    pub input: Value,
}

struct ToolEndpoint {
    entry: Entry,
}

#[async_trait]
impl Endpoint<ToolCall, Value> for ToolEndpoint {
    async fn call(&self, call: ToolCall) -> Result<Value> {
        let d = &self.entry.descriptor;
        let timeout = Duration::from_millis(d.timeout_ms);
        let mut attempt = 0u32;
        loop {
            let res =
                match tokio::time::timeout(timeout, self.entry.tool.invoke(&call.ctx, call.input.clone()))
                    .await
                {
                    Ok(r) => r,
                    Err(_) => Err(HarnessError::timeout(format!(
                        "tool {} exceeded {timeout:?}",
                        d.id
                    ))),
                };
            match res {
                Err(e) if e.is_retryable() && d.idempotent && attempt < d.max_retries => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(100 * 2u64.pow(attempt.min(6)))).await;
                }
                other => return other,
            }
        }
    }
}

pub struct ToolGateway {
    db: Db,
    registry: Arc<ToolRegistry>,
    authz: Arc<dyn Authorizer>,
    policy: Arc<dyn PolicyEngine>,
    approvals: ApprovalManager,
    budgets: BudgetLedger,
    hooks: Arc<HookRegistry>,
    pipeline: Pipeline<ToolCall, Value>,
    output_interceptors: InterceptorChain<Value>,
    providers: ProviderRegistry,
    rate: Mutex<HashMap<(String, String), VecDeque<Instant>>>,
}

impl std::fmt::Debug for ToolGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolGateway")
            .field("pipeline", &self.pipeline)
            .finish_non_exhaustive()
    }
}

#[allow(clippy::too_many_arguments)]
impl ToolGateway {
    pub fn new(
        db: Db,
        registry: Arc<ToolRegistry>,
        authz: Arc<dyn Authorizer>,
        policy: Arc<dyn PolicyEngine>,
        approvals: ApprovalManager,
        budgets: BudgetLedger,
        hooks: Arc<HookRegistry>,
        providers: ProviderRegistry,
    ) -> Self {
        Self {
            db,
            registry,
            authz,
            policy,
            approvals,
            budgets,
            hooks,
            pipeline: Pipeline::new("tool-invocation", Duration::from_secs(3600)),
            output_interceptors: InterceptorChain::new(),
            providers,
            rate: Mutex::new(HashMap::new()),
        }
    }

    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }

    pub fn pipeline(&self) -> &Pipeline<ToolCall, Value> {
        &self.pipeline
    }

    pub fn output_interceptors(&self) -> &InterceptorChain<Value> {
        &self.output_interceptors
    }

    pub fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }

    /// Fingerprint binding an approval to an exact tool version and input.
    pub fn fingerprint(tool: &ToolId, version: &semver::Version, input: &Value) -> String {
        sha256_hex(format!("{tool}@{version}\n{}", canonical_json(input)).as_bytes())
    }

    /// Invoke a tool through the governed pipeline. Runs inside a `tool_invocation` span
    /// carrying the correlation id, tool and principal.
    pub async fn invoke(&self, req: ToolRequest) -> Result<ToolResponse> {
        use tracing::Instrument;
        let span = tracing::info_span!(
            "tool_invocation",
            correlation_id = %req.trace.correlation,
            tool = %req.tool,
            principal = %req.principal.id
        );
        self.invoke_inner(req).instrument(span).await
    }

    async fn invoke_inner(&self, req: ToolRequest) -> Result<ToolResponse> {
        let invocation_id = InvocationId::new();
        let start = Instant::now();
        let entry = match self.registry.resolve(&req.tool, req.version.as_ref()) {
            Ok(e) => e,
            Err(e) => {
                self.audit_denied(&req, invocation_id, None, &e, json!({}))
                    .await?;
                return Err(e);
            }
        };
        let d = entry.descriptor.clone();
        let mut policy_rules = vec![];

        // ---- Deterministic controls (never delegated to hooks or middleware) ----
        let pre = self.preflight(&req, &entry, &mut policy_rules).await;
        let provider = match pre {
            Ok(p) => p,
            Err(e) => {
                self.audit_denied(
                    &req,
                    invocation_id,
                    Some(&entry),
                    &e,
                    json!({"policy_rules": policy_rules}),
                )
                .await?;
                return Err(e);
            }
        };

        // ---- Hooks, middleware, execution ----
        let hook_ctx = |point, error: Option<String>| HookContext {
            point,
            scope: Some(req.scope),
            trace: req.trace,
            actor: Some(req.principal.id.to_string()),
            subject: d.id.to_string(),
            payload: HookPayload::Tool {
                tool_id: d.id.to_string(),
                tool_version: d.version.to_string(),
                risk: d.risk.to_string(),
                invocation_id: invocation_id.to_string(),
                error,
            },
        };
        let mut hooks_run = vec![];
        let before = self
            .hooks
            .dispatch(&hook_ctx(HookPoint::BeforeToolInvocation, None))
            .await;
        hooks_run.extend(before.hook_ids());
        if let Err(e) = before.into_result() {
            self.audit_denied(&req, invocation_id, Some(&entry), &e, json!({"hooks": hooks_run}))
                .await?;
            return Err(e);
        }

        let ctx = ToolContext {
            invocation_id,
            principal: req.principal.clone(),
            scope: req.scope,
            workspace: req.workspace.clone(),
            provider: provider.clone(),
            trace: req.trace,
        };
        let call = ToolCall {
            descriptor: d.clone(),
            ctx,
            input: req.input.clone(),
        };
        let endpoint = ToolEndpoint { entry: entry.clone() };
        let result = self.pipeline.execute(call, &endpoint).await.and_then(|out| {
            entry.output.check(&out, "tool output")?;
            self.output_interceptors.apply(out)
        });
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        let (point, err) = match &result {
            Ok(_) => (HookPoint::AfterToolInvocation, None),
            Err(e) => (HookPoint::OnToolFailure, Some(e.to_string())),
        };
        let after = self.hooks.dispatch(&hook_ctx(point, err)).await;
        hooks_run.extend(after.hook_ids());

        // ---- Audit + event (single transaction) ----
        let mut tx = self.db.write().await?;
        let outcome = if result.is_ok() {
            AuditOutcome::Succeeded
        } else {
            AuditOutcome::Failed
        };
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    &req.principal,
                    "tool.invoke",
                    "tool",
                    format!("{}@{}", d.id, d.version),
                    outcome,
                )
                .scope(req.scope)
                .correlation(req.trace.correlation)
                .details(json!({
                    "invocation_id": invocation_id.to_string(),
                    "risk": d.risk,
                    "input_sha256": sha256_hex(canonical_json(&req.input).as_bytes()),
                    "policy_rules": policy_rules,
                    "hooks": hooks_run,
                    "middleware": self.pipeline.names(),
                    "provider": provider.as_ref().map(|p| p.id().to_string()),
                    "duration_ms": duration_ms,
                    "error": result.as_ref().err().map(|e| e.to_string()),
                })),
            )
            .await?;
        let ev = EventEnvelope::new(
            &ToolInvoked {
                invocation_id: invocation_id.to_string(),
                tool_id: d.id.to_string(),
                tool_version: d.version.to_string(),
                principal_id: req.principal.id.to_string(),
                succeeded: result.is_ok(),
                duration_ms,
            },
            "tool-gateway",
            Some(req.scope),
            req.trace.caused_by(invocation_id.as_uuid()),
            self.db.now(),
        )?;
        outbox::enqueue_in(&mut tx, &ev).await?;
        tx.commit().await?;

        let output = result?;
        Ok(ToolResponse {
            invocation_id,
            tool_id: d.id.clone(),
            tool_version: d.version.to_string(),
            output: Untrusted::new(output),
            duration_ms,
            hooks: hooks_run,
            policy_rules,
            provider: provider.map(|p| p.id().to_string()),
        })
    }

    /// Steps 0–8 of the gateway: all deterministic, all before any extension code runs.
    async fn preflight(
        &self,
        req: &ToolRequest,
        entry: &Entry,
        policy_rules: &mut Vec<String>,
    ) -> Result<Option<Arc<dyn eaeh_isolation::ExecutionProvider>>> {
        let d = &entry.descriptor;
        // 0. Agent definition allowlist.
        if let Some(allowed) = &req.allowed_tools
            && !allowed.contains(&d.id)
        {
            return Err(HarnessError::forbidden(format!(
                "tool '{}' is not in the agent's allowed tools",
                d.id
            )));
        }
        if req.principal.is_agent() && req.allowed_tools.is_none() {
            return Err(HarnessError::forbidden(
                "agent tool calls must carry the agent definition allowlist",
            ));
        }
        // 1. Authorization (role / scope).
        self.authz
            .authorize(&req.principal, Action::ToolInvoke, Target::Scope(req.scope))?;
        // 2. Autonomy ceiling.
        if let Some(level) = req.principal.autonomy()
            && !level.permits(d.risk)
        {
            return Err(HarnessError::forbidden(format!(
                "autonomy level {level:?} does not permit {} tools (max {})",
                d.risk,
                level.max_risk()
            )));
        }
        // 3. Input contract.
        entry.input.check(&req.input, "tool input")?;
        // 4. Policy.
        let decision = self.policy.evaluate(&PolicyRequest {
            action: "tool.invoke".into(),
            principal_id: req.principal.id.to_string(),
            principal_kind: req.principal.kind_str().into(),
            autonomy: req.principal.autonomy(),
            risk: Some(d.risk),
            resource_type: "tool".into(),
            resource_id: d.id.to_string(),
            scope: Some(req.scope),
            attributes: Default::default(),
        });
        policy_rules.extend(decision.matched_rules.iter().cloned());
        match decision.effect {
            Effect::Permit => {}
            Effect::Deny => {
                return Err(HarnessError::policy_denied(format!(
                    "tool '{}': {}",
                    d.id, decision.reason
                )));
            }
            Effect::RequireApproval { role } => {
                // 5. Approval bound to exact tool version + input.
                let subject = ApprovalSubject::ToolInvocation {
                    tool: d.id.to_string(),
                    fingerprint: Self::fingerprint(&d.id, &d.version, &req.input),
                };
                let approved = match req.approval {
                    Some(id) => self.approvals.approved_for(id, &subject, req.scope).await?,
                    None => false,
                };
                if !approved {
                    let id = self
                        .approvals
                        .request(ApprovalRequest {
                            scope: req.scope,
                            subject,
                            required_role: role,
                            authors: vec![req.principal.id.clone()],
                            requested_by: req.principal.id.clone(),
                            trace: req.trace,
                        })
                        .await?;
                    return Err(HarnessError::approval_required(format!(
                        "tool '{}' requires human approval; approval {id} requested",
                        d.id
                    )));
                }
            }
        }
        // 6. Workspace / isolation availability (refuse, never downgrade).
        if d.needs_workspace && req.workspace.is_none() {
            return Err(HarnessError::invalid_input(format!(
                "tool '{}' requires a workspace",
                d.id
            )));
        }
        if let Some(ws) = &req.workspace
            && ws.scope != req.scope
        {
            return Err(HarnessError::forbidden("workspace belongs to a different scope"));
        }
        let provider = if d.needs_execution {
            Some(self.providers.select(&d.isolation)?)
        } else {
            None
        };
        // 7. Rate limit.
        if let Some(limit) = d.rate_limit_per_minute {
            self.check_rate(&req.principal.id.to_string(), d.id.as_str(), limit)?;
        }
        // 8. Budget (fail closed for agents without budget owners).
        if req.principal.is_agent() && req.budget_owners.is_empty() {
            return Err(HarnessError::budget_exceeded(
                "agent tool calls require a budget owner",
            ));
        }
        if !req.budget_owners.is_empty() {
            let owners: Vec<&str> = req.budget_owners.iter().map(String::as_str).collect();
            self.budgets
                .consume(&owners, BudgetResource::ToolCalls, 1)
                .await?;
        }
        Ok(provider)
    }

    fn check_rate(&self, principal: &str, tool: &str, per_minute: u32) -> Result<()> {
        let mut map = self
            .rate
            .lock()
            .map_err(|_| HarnessError::internal("rate limiter poisoned"))?;
        let q = map.entry((principal.to_string(), tool.to_string())).or_default();
        let now = Instant::now();
        while q
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
        {
            q.pop_front();
        }
        if q.len() >= per_minute as usize {
            return Err(HarnessError::unavailable(format!(
                "rate limit of {per_minute}/min exceeded for tool '{tool}'"
            )));
        }
        q.push_back(now);
        Ok(())
    }

    async fn audit_denied(
        &self,
        req: &ToolRequest,
        invocation_id: InvocationId,
        entry: Option<&Entry>,
        err: &HarnessError,
        extra: Value,
    ) -> Result<()> {
        let resource = entry
            .map(|e| format!("{}@{}", e.descriptor.id, e.descriptor.version))
            .unwrap_or_else(|| req.tool.to_string());
        tracing::info!(tool = %resource, principal = %req.principal.id, kind = %err.kind, "tool invocation denied");
        self.db
            .audit(
                AuditEntry::new(
                    &req.principal,
                    "tool.invoke",
                    "tool",
                    resource,
                    AuditOutcome::Denied,
                )
                .scope(req.scope)
                .correlation(req.trace.correlation)
                .details(json!({
                    "invocation_id": invocation_id.to_string(),
                    "reason_kind": err.kind,
                    "reason": err.message,
                    "extra": extra,
                })),
            )
            .await
            .map(|_| ())
    }
}
