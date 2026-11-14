use async_trait::async_trait;
use eaeh_core::{HarnessError, InvocationId, Principal, Result, RiskClass, Scope, ToolId, TraceContext};
use eaeh_isolation::{ExecutionProvider, IsolationRequirements};
use eaeh_validation::schema::CompiledSchema;
use eaeh_workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub id: ToolId,
    pub version: semver::Version,
    pub description: String,
    pub risk: RiskClass,
    pub input_schema: Value,
    pub output_schema: Value,
    /// Isolation the tool's execution needs (checked against provider capabilities).
    pub isolation: IsolationRequirements,
    /// Whether the tool executes processes through an isolation provider.
    pub needs_execution: bool,
    pub needs_workspace: bool,
    pub timeout_ms: u64,
    pub rate_limit_per_minute: Option<u32>,
    pub idempotent: bool,
    pub max_retries: u32,
}

/// Per-invocation context handed to a tool by the gateway.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub invocation_id: InvocationId,
    pub principal: Principal,
    pub scope: Scope,
    pub workspace: Option<Workspace>,
    /// Provider selected by the gateway (only for `needs_execution` tools).
    pub provider: Option<Arc<dyn ExecutionProvider>>,
    pub trace: TraceContext,
}

impl ToolContext {
    pub fn workspace(&self) -> Result<&Workspace> {
        self.workspace
            .as_ref()
            .ok_or_else(|| HarnessError::invalid_input("tool requires a workspace"))
    }

    /// Lock-holder identity used for workspace coordination.
    pub fn holder(&self) -> String {
        self.principal.id.to_string()
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn descriptor(&self) -> &ToolDescriptor;
    async fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value>;
}

#[derive(Clone)]
pub(crate) struct Entry {
    pub tool: Arc<dyn Tool>,
    pub descriptor: Arc<ToolDescriptor>,
    pub input: CompiledSchema,
    pub output: CompiledSchema,
    pub source: String,
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: RwLock<BTreeMap<ToolId, BTreeMap<semver::Version, Entry>>>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tools", &self.list().len())
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool version. `source` is `core` or the contributing plugin id.
    pub fn register(&self, tool: Arc<dyn Tool>, source: &str) -> Result<()> {
        let d = tool.descriptor().clone();
        if d.description.trim().is_empty() {
            return Err(HarnessError::invalid_input("tool description required"));
        }
        if d.timeout_ms == 0 || d.timeout_ms > 3_600_000 {
            return Err(HarnessError::invalid_input("tool timeout_ms must be 1..=3600000"));
        }
        if d.needs_execution && !(d.isolation.timeout && d.isolation.env_isolation) {
            return Err(HarnessError::invalid_input(
                "executing tools must require at least timeout and env isolation",
            ));
        }
        if d.max_retries > 0 && !d.idempotent {
            return Err(HarnessError::invalid_input(
                "only idempotent tools may be retried",
            ));
        }
        let input = CompiledSchema::compile(&d.input_schema)?;
        let output = CompiledSchema::compile(&d.output_schema)?;
        let mut t = self
            .tools
            .write()
            .map_err(|_| HarnessError::internal("tool registry lock poisoned"))?;
        let versions = t.entry(d.id.clone()).or_default();
        if versions.contains_key(&d.version) {
            return Err(HarnessError::conflict(format!(
                "tool {}@{} already registered",
                d.id, d.version
            )));
        }
        versions.insert(
            d.version.clone(),
            Entry {
                tool,
                descriptor: Arc::new(d),
                input,
                output,
                source: source.into(),
            },
        );
        Ok(())
    }

    /// Remove all tools contributed by a source (plugin disable/retire).
    pub fn unregister_source(&self, source: &str) -> usize {
        let Ok(mut t) = self.tools.write() else { return 0 };
        let mut removed = 0;
        for versions in t.values_mut() {
            let before = versions.len();
            versions.retain(|_, e| e.source != source);
            removed += before - versions.len();
        }
        t.retain(|_, v| !v.is_empty());
        removed
    }

    /// Highest registered version matching `req` (any version if `None`).
    pub(crate) fn resolve(&self, id: &ToolId, req: Option<&semver::VersionReq>) -> Result<Entry> {
        let t = self
            .tools
            .read()
            .map_err(|_| HarnessError::internal("tool registry lock poisoned"))?;
        let versions = t
            .get(id)
            .ok_or_else(|| HarnessError::not_found(format!("tool '{id}' not registered")))?;
        versions
            .iter()
            .rev()
            .find(|(v, _)| req.is_none_or(|r| r.matches(v)))
            .map(|(_, e)| e.clone())
            .ok_or_else(|| {
                HarnessError::not_found(format!("no version of tool '{id}' matches the requirement"))
            })
    }

    pub fn list(&self) -> Vec<ToolDescriptor> {
        self.tools
            .read()
            .map(|t| {
                t.values()
                    .flat_map(|v| v.values().map(|e| (*e.descriptor).clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn descriptor(&self, id: &ToolId) -> Result<ToolDescriptor> {
        Ok((*self.resolve(id, None)?.descriptor).clone())
    }
}
