//! Lifecycle hook framework (master instructions §6).

use async_trait::async_trait;
use eaeh_core::{HarnessError, Result, Scope, TenantId, TraceContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Hook API version. Registrations declare a compatible requirement.
pub const HOOK_API_VERSION: semver::Version = semver::Version::new(1, 0, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPoint {
    // Agent
    BeforeAgentInstantiation,
    AfterAgentInstantiation,
    BeforeAgentExecution,
    AfterAgentExecution,
    BeforeAgentTermination,
    AfterAgentTermination,
    // Task & workflow
    BeforeTaskExecution,
    AfterTaskExecution,
    BeforeWorkflowTransition,
    AfterWorkflowTransition,
    BeforeApproval,
    AfterApproval,
    OnRejection,
    OnCancellation,
    OnTimeout,
    OnFailure,
    OnRecovery,
    // LLM
    BeforeModelRequest,
    AfterModelRequest,
    BeforeResponseProcessing,
    AfterResponseProcessing,
    // Tool
    BeforeToolInvocation,
    AfterToolInvocation,
    OnToolFailure,
    // Knowledge & context
    BeforeKnowledgeRetrieval,
    AfterKnowledgeRetrieval,
    BeforeContextAssembly,
    AfterContextAssembly,
    // Engineering
    BeforeArtifactCreation,
    AfterArtifactCreation,
    BeforeFileModification,
    AfterFileModification,
    BeforeBuild,
    AfterBuild,
    BeforeTest,
    AfterTest,
    BeforeCommit,
    AfterCommit,
    BeforePullRequest,
    AfterPullRequest,
    BeforeDeployment,
    AfterDeployment,
}

impl HookPoint {
    /// Vetoes are honoured only at `Before*` points (they can only make an operation
    /// more restrictive, never authorize anything).
    pub fn is_before(self) -> bool {
        matches!(
            self,
            HookPoint::BeforeAgentInstantiation
                | HookPoint::BeforeAgentExecution
                | HookPoint::BeforeAgentTermination
                | HookPoint::BeforeTaskExecution
                | HookPoint::BeforeWorkflowTransition
                | HookPoint::BeforeApproval
                | HookPoint::BeforeModelRequest
                | HookPoint::BeforeResponseProcessing
                | HookPoint::BeforeToolInvocation
                | HookPoint::BeforeKnowledgeRetrieval
                | HookPoint::BeforeContextAssembly
                | HookPoint::BeforeArtifactCreation
                | HookPoint::BeforeFileModification
                | HookPoint::BeforeBuild
                | HookPoint::BeforeTest
                | HookPoint::BeforeCommit
                | HookPoint::BeforePullRequest
                | HookPoint::BeforeDeployment
        )
    }

    pub fn as_str(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default()
    }
}

/// Typed payload per hook category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "category")]
pub enum HookPayload {
    Agent {
        definition_id: String,
        definition_version: String,
        instance_id: Option<String>,
        state: Option<String>,
    },
    Task {
        task_id: String,
        kind: String,
        attempt: u32,
        error: Option<String>,
    },
    Workflow {
        instance_id: String,
        from_state: String,
        to_state: Option<String>,
        trigger: String,
    },
    Approval {
        approval_id: String,
        decision: Option<String>,
    },
    Model {
        provider_id: String,
        model_id: String,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        error: Option<String>,
    },
    Tool {
        tool_id: String,
        tool_version: String,
        risk: String,
        invocation_id: String,
        error: Option<String>,
    },
    Context {
        items: u32,
        token_estimate: u64,
    },
    Knowledge {
        query: String,
    },
    Artifact {
        name: String,
        content_hash: Option<String>,
        size_bytes: Option<u64>,
    },
    File {
        workspace_id: String,
        path: String,
    },
    Engineering {
        operation: String,
        detail: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookContext {
    pub point: HookPoint,
    pub scope: Option<Scope>,
    pub trace: TraceContext,
    pub actor: Option<String>,
    /// Filter key (tool id, agent definition id, workflow definition id, …).
    pub subject: String,
    pub payload: HookPayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookOutcome {
    Proceed,
    Veto(String),
}

#[async_trait]
pub trait Hook: Send + Sync {
    async fn on_event(&self, ctx: &HookContext) -> Result<HookOutcome>;
}

/// Behaviour when the hook errors or times out (risk-classified per registration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    /// Abort the operation (high-risk hooks, e.g. compliance checks).
    Block,
    /// Record a warning and continue.
    Warn,
    /// Continue silently (recorded in the dispatch report only).
    Continue,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HookFilter {
    /// Exact subjects or prefixes ending in `*`. `None` matches all.
    pub subjects: Option<Vec<String>>,
    pub tenants: Option<Vec<TenantId>>,
}

impl HookFilter {
    fn matches(&self, ctx: &HookContext) -> bool {
        let subj_ok = self.subjects.as_ref().is_none_or(|subs| {
            subs.iter().any(|s| match s.strip_suffix('*') {
                Some(prefix) => ctx.subject.starts_with(prefix),
                None => &ctx.subject == s,
            })
        });
        let tenant_ok = self
            .tenants
            .as_ref()
            .is_none_or(|ts| ctx.scope.map(|s| ts.contains(&s.tenant)).unwrap_or(false));
        subj_ok && tenant_ok
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookRegistration {
    pub id: String,
    pub version: semver::Version,
    /// Required hook API version (e.g. `^1`).
    pub api: semver::VersionReq,
    pub points: Vec<HookPoint>,
    /// Lower values run first; ties broken by id.
    pub priority: i32,
    pub failure_policy: FailurePolicy,
    pub timeout: Duration,
    pub enabled: bool,
    pub filter: HookFilter,
    /// `core` or the contributing plugin id.
    pub source: String,
}

impl HookRegistration {
    pub fn new(id: &str, points: Vec<HookPoint>, failure_policy: FailurePolicy) -> Self {
        Self {
            id: id.into(),
            version: semver::Version::new(1, 0, 0),
            api: semver::VersionReq::parse("^1").unwrap_or(semver::VersionReq::STAR),
            points,
            priority: 100,
            failure_policy,
            timeout: Duration::from_secs(5),
            enabled: true,
            filter: HookFilter::default(),
            source: "core".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookExecution {
    pub hook_id: String,
    pub hook_version: String,
    pub source: String,
    pub result: String,
    pub duration_ms: u64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HookDispatchReport {
    pub point: Option<HookPoint>,
    pub executions: Vec<HookExecution>,
    pub blocked: Option<String>,
    pub warnings: Vec<String>,
}

impl HookDispatchReport {
    pub fn into_result(self) -> Result<Self> {
        match &self.blocked {
            Some(reason) => Err(HarnessError::hook_blocked(reason.clone())),
            None => Ok(self),
        }
    }

    pub fn hook_ids(&self) -> Vec<String> {
        self.executions
            .iter()
            .map(|e| format!("{}@{}", e.hook_id, e.hook_version))
            .collect()
    }
}

type Entry = (HookRegistration, Arc<dyn Hook>);

#[derive(Default)]
pub struct HookRegistry {
    entries: RwLock<Vec<Entry>>,
}

impl std::fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<String> = self.registrations().into_iter().map(|r| r.id).collect();
        f.debug_struct("HookRegistry").field("hooks", &ids).finish()
    }
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, reg: HookRegistration, hook: Arc<dyn Hook>) -> Result<()> {
        if reg.id.is_empty() || reg.points.is_empty() {
            return Err(HarnessError::invalid_input(
                "hook registration needs an id and at least one point",
            ));
        }
        if !reg.api.matches(&HOOK_API_VERSION) {
            return Err(HarnessError::invalid_input(format!(
                "hook '{}' requires hook API {} but platform provides {}",
                reg.id, reg.api, HOOK_API_VERSION
            )));
        }
        if reg.timeout.is_zero() || reg.timeout > Duration::from_secs(300) {
            return Err(HarnessError::invalid_input(
                "hook timeout must be within (0, 300s]",
            ));
        }
        let mut e = self
            .entries
            .write()
            .map_err(|_| HarnessError::internal("hook lock poisoned"))?;
        if e.iter().any(|(r, _)| r.id == reg.id) {
            return Err(HarnessError::conflict(format!(
                "hook '{}' already registered",
                reg.id
            )));
        }
        e.push((reg, hook));
        e.sort_by(|a, b| a.0.priority.cmp(&b.0.priority).then_with(|| a.0.id.cmp(&b.0.id)));
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let mut e = self
            .entries
            .write()
            .map_err(|_| HarnessError::internal("hook lock poisoned"))?;
        let entry = e
            .iter_mut()
            .find(|(r, _)| r.id == id)
            .ok_or_else(|| HarnessError::not_found("hook not found"))?;
        entry.0.enabled = enabled;
        Ok(())
    }

    /// Remove all hooks contributed by a source (plugin disable/retire).
    pub fn unregister_source(&self, source: &str) -> usize {
        match self.entries.write() {
            Ok(mut e) => {
                let before = e.len();
                e.retain(|(r, _)| r.source != source);
                before - e.len()
            }
            Err(_) => 0,
        }
    }

    pub fn registrations(&self) -> Vec<HookRegistration> {
        self.entries
            .read()
            .map(|e| e.iter().map(|(r, _)| r.clone()).collect())
            .unwrap_or_default()
    }

    /// Run all enabled, matching hooks for `ctx.point` in deterministic order.
    pub async fn dispatch(&self, ctx: &HookContext) -> HookDispatchReport {
        let selected: Vec<Entry> = self
            .entries
            .read()
            .map(|e| {
                e.iter()
                    .filter(|(r, _)| r.enabled && r.points.contains(&ctx.point) && r.filter.matches(ctx))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let mut report = HookDispatchReport {
            point: Some(ctx.point),
            ..Default::default()
        };
        for (reg, hook) in selected {
            let start = Instant::now();
            let res = tokio::time::timeout(reg.timeout, hook.on_event(ctx)).await;
            let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            let (result, message, failed) = match res {
                Ok(Ok(HookOutcome::Proceed)) => ("proceed", None, false),
                Ok(Ok(HookOutcome::Veto(reason))) => {
                    if ctx.point.is_before() {
                        report
                            .executions
                            .push(exec(&reg, "veto", duration_ms, Some(reason.clone())));
                        report.blocked = Some(format!(
                            "hook '{}' vetoed {}: {reason}",
                            reg.id,
                            ctx.point.as_str()
                        ));
                        tracing::info!(hook = %reg.id, point = %ctx.point.as_str(), %reason, "hook veto");
                        return report;
                    }
                    report
                        .warnings
                        .push(format!("hook '{}' veto ignored at non-before point", reg.id));
                    ("veto_ignored", Some(reason), false)
                }
                Ok(Err(e)) => ("error", Some(e.to_string()), true),
                Err(_) => ("timeout", Some(format!("exceeded {:?}", reg.timeout)), true),
            };
            report
                .executions
                .push(exec(&reg, result, duration_ms, message.clone()));
            if failed {
                let msg = format!(
                    "hook '{}' {result} at {}: {}",
                    reg.id,
                    ctx.point.as_str(),
                    message.unwrap_or_default()
                );
                match reg.failure_policy {
                    FailurePolicy::Block => {
                        tracing::warn!(hook = %reg.id, "blocking hook failed: {msg}");
                        report.blocked = Some(msg);
                        return report;
                    }
                    FailurePolicy::Warn => {
                        tracing::warn!(hook = %reg.id, "{msg}");
                        report.warnings.push(msg);
                    }
                    FailurePolicy::Continue => {
                        tracing::debug!(hook = %reg.id, "{msg}");
                    }
                }
            }
        }
        report
    }
}

fn exec(reg: &HookRegistration, result: &str, duration_ms: u64, message: Option<String>) -> HookExecution {
    HookExecution {
        hook_id: reg.id.clone(),
        hook_version: reg.version.to_string(),
        source: reg.source.clone(),
        result: result.into(),
        duration_ms,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Rec {
        name: &'static str,
        log: Arc<Mutex<Vec<&'static str>>>,
        behaviour: Behaviour,
    }

    #[derive(Clone, Copy)]
    enum Behaviour {
        Ok,
        Fail,
        Veto,
        Hang,
    }

    #[async_trait]
    impl Hook for Rec {
        async fn on_event(&self, _ctx: &HookContext) -> Result<HookOutcome> {
            self.log.lock().unwrap().push(self.name);
            match self.behaviour {
                Behaviour::Ok => Ok(HookOutcome::Proceed),
                Behaviour::Fail => Err(HarnessError::internal("boom")),
                Behaviour::Veto => Ok(HookOutcome::Veto("not allowed".into())),
                Behaviour::Hang => {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    Ok(HookOutcome::Proceed)
                }
            }
        }
    }

    fn ctx(point: HookPoint, subject: &str) -> HookContext {
        HookContext {
            point,
            scope: None,
            trace: TraceContext::new_root(),
            actor: None,
            subject: subject.into(),
            payload: HookPayload::Knowledge { query: "q".into() },
        }
    }

    fn add(
        r: &HookRegistry,
        log: &Arc<Mutex<Vec<&'static str>>>,
        name: &'static str,
        prio: i32,
        fp: FailurePolicy,
        b: Behaviour,
    ) {
        let mut reg = HookRegistration::new(
            name,
            vec![HookPoint::BeforeToolInvocation, HookPoint::AfterToolInvocation],
            fp,
        );
        reg.priority = prio;
        reg.timeout = Duration::from_millis(50);
        r.register(
            reg,
            Arc::new(Rec {
                name,
                log: log.clone(),
                behaviour: b,
            }),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn ordering_by_priority_then_id() {
        let r = HookRegistry::new();
        let log = Arc::new(Mutex::new(vec![]));
        add(&r, &log, "c", 5, FailurePolicy::Warn, Behaviour::Ok);
        add(&r, &log, "a", 10, FailurePolicy::Warn, Behaviour::Ok);
        add(&r, &log, "b", 5, FailurePolicy::Warn, Behaviour::Ok);
        let rep = r.dispatch(&ctx(HookPoint::BeforeToolInvocation, "x")).await;
        assert_eq!(*log.lock().unwrap(), vec!["b", "c", "a"]);
        assert!(rep.blocked.is_none());
    }

    #[tokio::test]
    async fn failure_policies() {
        let log = Arc::new(Mutex::new(vec![]));
        let r = HookRegistry::new();
        add(&r, &log, "warn", 1, FailurePolicy::Warn, Behaviour::Fail);
        add(&r, &log, "cont", 2, FailurePolicy::Continue, Behaviour::Hang);
        add(&r, &log, "after", 3, FailurePolicy::Warn, Behaviour::Ok);
        let rep = r.dispatch(&ctx(HookPoint::BeforeToolInvocation, "x")).await;
        assert!(rep.blocked.is_none());
        assert_eq!(rep.warnings.len(), 1);
        assert_eq!(rep.executions[1].result, "timeout");
        assert_eq!(rep.executions.len(), 3);

        let r2 = HookRegistry::new();
        add(&r2, &log, "block", 1, FailurePolicy::Block, Behaviour::Fail);
        add(&r2, &log, "never", 2, FailurePolicy::Warn, Behaviour::Ok);
        let rep = r2.dispatch(&ctx(HookPoint::BeforeToolInvocation, "x")).await;
        assert!(rep.blocked.is_some());
        assert_eq!(rep.executions.len(), 1, "later hooks do not run after a block");
        assert_eq!(
            rep.into_result().unwrap_err().kind,
            eaeh_core::ErrorKind::HookBlocked
        );

        let r3 = HookRegistry::new();
        add(&r3, &log, "slowblock", 1, FailurePolicy::Block, Behaviour::Hang);
        assert!(
            r3.dispatch(&ctx(HookPoint::BeforeToolInvocation, "x"))
                .await
                .blocked
                .is_some()
        );
    }

    #[tokio::test]
    async fn veto_only_honoured_at_before_points() {
        let log = Arc::new(Mutex::new(vec![]));
        let r = HookRegistry::new();
        add(&r, &log, "v", 1, FailurePolicy::Continue, Behaviour::Veto);
        assert!(
            r.dispatch(&ctx(HookPoint::BeforeToolInvocation, "x"))
                .await
                .blocked
                .is_some()
        );
        let after = r.dispatch(&ctx(HookPoint::AfterToolInvocation, "x")).await;
        assert!(after.blocked.is_none());
        assert_eq!(after.warnings.len(), 1);
    }

    #[tokio::test]
    async fn filters_disable_and_compatibility() {
        let log = Arc::new(Mutex::new(vec![]));
        let r = HookRegistry::new();
        let mut reg = HookRegistration::new(
            "fs-only",
            vec![HookPoint::BeforeToolInvocation],
            FailurePolicy::Block,
        );
        reg.filter.subjects = Some(vec!["fs.*".into()]);
        r.register(
            reg,
            Arc::new(Rec {
                name: "fs-only",
                log: log.clone(),
                behaviour: Behaviour::Veto,
            }),
        )
        .unwrap();
        assert!(
            r.dispatch(&ctx(HookPoint::BeforeToolInvocation, "exec.run"))
                .await
                .blocked
                .is_none()
        );
        assert!(
            r.dispatch(&ctx(HookPoint::BeforeToolInvocation, "fs.write"))
                .await
                .blocked
                .is_some()
        );
        r.set_enabled("fs-only", false).unwrap();
        assert!(
            r.dispatch(&ctx(HookPoint::BeforeToolInvocation, "fs.write"))
                .await
                .blocked
                .is_none()
        );

        let mut incompatible = HookRegistration::new("v2", vec![HookPoint::OnFailure], FailurePolicy::Warn);
        incompatible.api = semver::VersionReq::parse("^2").unwrap();
        assert!(
            r.register(
                incompatible,
                Arc::new(Rec {
                    name: "v2",
                    log,
                    behaviour: Behaviour::Ok
                })
            )
            .is_err()
        );
    }
}
