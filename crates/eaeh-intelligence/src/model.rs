use async_trait::async_trait;
use eaeh_core::{HarnessError, ModelId, ProviderId, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_output_tokens: u32,
    pub temperature: Option<f32>,
}

/// A tool call *proposed* by a model. It is a request, never an authorization: it must
/// pass through the tool gateway like any other call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedToolCall {
    pub tool: String,
    pub input: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelResponse {
    pub content: String,
    pub tool_calls: Vec<ProposedToolCall>,
    pub usage: Usage,
    /// Exact model version reported by the provider.
    pub model_version: String,
    pub finish_reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDescriptor {
    pub id: ModelId,
    pub provider: ProviderId,
    pub version: String,
    pub capabilities: BTreeSet<String>,
    pub context_window: u64,
    pub max_output_tokens: u32,
    pub input_cost_per_mtok_micros: u64,
    pub output_cost_per_mtok_micros: u64,
    /// Relative quality tier (higher is more capable).
    pub quality_tier: u8,
    /// Data residency regions the model's processing is confined to.
    pub residency: BTreeSet<String>,
}

impl ModelDescriptor {
    pub fn cost_micros(&self, u: Usage) -> u64 {
        (u.input_tokens.saturating_mul(self.input_cost_per_mtok_micros)
            + u.output_tokens.saturating_mul(self.output_cost_per_mtok_micros))
        .div_ceil(1_000_000)
    }

    fn unit_cost(&self) -> u64 {
        self.input_cost_per_mtok_micros + self.output_cost_per_mtok_micros
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoutingRequirements {
    pub capabilities: BTreeSet<String>,
    pub min_context: u64,
    pub min_quality: u8,
    pub residency: Option<String>,
    pub max_unit_cost_micros: Option<u64>,
    /// Preferred models in order (e.g. from an agent definition); still filtered.
    pub preferred: Vec<ModelId>,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &ProviderId;
    fn models(&self) -> Vec<ModelDescriptor>;
    /// Where request content goes. Defaults to `External` (fail-safe for egress policy);
    /// only providers that run entirely in-process/on-host may report `Local`.
    fn egress(&self) -> Egress {
        Egress::External
    }
    async fn complete(&self, model: &ModelId, request: &ModelRequest) -> Result<ModelResponse>;
}

/// Data egress of a provider (input to the `model.invoke` policy decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Egress {
    Local,
    External,
}

impl Egress {
    pub fn as_str(self) -> &'static str {
        match self {
            Egress::Local => "local",
            Egress::External => "external",
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Health {
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

pub struct ModelRegistry {
    providers: RwLock<BTreeMap<ProviderId, Arc<dyn ModelProvider>>>,
    health: Mutex<HashMap<ProviderId, Health>>,
    failure_threshold: u32,
    cooldown: Duration,
}

impl std::fmt::Debug for ModelRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<String> = self
            .providers
            .read()
            .map(|p| p.keys().map(|k| k.to_string()).collect())
            .unwrap_or_default();
        f.debug_struct("ModelRegistry").field("providers", &ids).finish()
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new(3, Duration::from_secs(30))
    }
}

impl ModelRegistry {
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            providers: RwLock::new(BTreeMap::new()),
            health: Mutex::new(HashMap::new()),
            failure_threshold: failure_threshold.max(1),
            cooldown,
        }
    }

    pub fn register(&self, p: Arc<dyn ModelProvider>) -> Result<()> {
        for m in p.models() {
            if &m.provider != p.id() {
                return Err(HarnessError::invalid_input(format!(
                    "model {} declares a different provider",
                    m.id
                )));
            }
            if m.context_window == 0 || m.max_output_tokens == 0 {
                return Err(HarnessError::invalid_input(format!(
                    "model {} has invalid limits",
                    m.id
                )));
            }
        }
        let mut ps = self
            .providers
            .write()
            .map_err(|_| HarnessError::internal("model registry poisoned"))?;
        if ps.contains_key(p.id()) {
            return Err(HarnessError::conflict(format!(
                "provider {} already registered",
                p.id()
            )));
        }
        ps.insert(p.id().clone(), p);
        Ok(())
    }

    pub fn provider(&self, id: &ProviderId) -> Option<Arc<dyn ModelProvider>> {
        self.providers.read().ok().and_then(|p| p.get(id).cloned())
    }

    pub fn models(&self) -> Vec<ModelDescriptor> {
        self.providers
            .read()
            .map(|p| p.values().flat_map(|p| p.models()).collect())
            .unwrap_or_default()
    }

    /// Candidate models satisfying `req`, in routing order: preferred first, then lowest
    /// unit cost, then highest quality, then id (deterministic).
    pub fn route(&self, req: &RoutingRequirements) -> Vec<ModelDescriptor> {
        let mut c: Vec<ModelDescriptor> = self
            .models()
            .into_iter()
            .filter(|m| req.capabilities.is_subset(&m.capabilities))
            .filter(|m| m.context_window >= req.min_context)
            .filter(|m| m.quality_tier >= req.min_quality)
            .filter(|m| req.residency.as_ref().is_none_or(|r| m.residency.contains(r)))
            .filter(|m| req.max_unit_cost_micros.is_none_or(|max| m.unit_cost() <= max))
            .collect();
        let pref = |m: &ModelDescriptor| {
            req.preferred
                .iter()
                .position(|p| p == &m.id)
                .unwrap_or(usize::MAX)
        };
        c.sort_by(|a, b| {
            pref(a)
                .cmp(&pref(b))
                .then(a.unit_cost().cmp(&b.unit_cost()))
                .then(b.quality_tier.cmp(&a.quality_tier))
                .then(a.id.cmp(&b.id))
        });
        c
    }

    pub fn is_available(&self, provider: &ProviderId) -> bool {
        let Ok(h) = self.health.lock() else { return true };
        h.get(provider)
            .and_then(|h| h.open_until)
            .is_none_or(|t| Instant::now() >= t)
    }

    pub fn record_success(&self, provider: &ProviderId) {
        if let Ok(mut h) = self.health.lock() {
            h.insert(provider.clone(), Health::default());
        }
    }

    pub fn record_failure(&self, provider: &ProviderId) {
        if let Ok(mut h) = self.health.lock() {
            let e = h.entry(provider.clone()).or_default();
            e.consecutive_failures += 1;
            if e.consecutive_failures >= self.failure_threshold {
                e.open_until = Some(Instant::now() + self.cooldown);
                tracing::warn!(provider = %provider, "model provider circuit opened");
            }
        }
    }
}
