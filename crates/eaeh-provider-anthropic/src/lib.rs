//! Anthropic Messages API adapter (Phase 2, P2-D1, ADR-0015).
//!
//! * `POST {base_url}/v1/messages` with headers `x-api-key`, `anthropic-version`
//!   (default `2026-10-21`) and, when server-side refusal fallback is enabled,
//!   `anthropic-beta: server-side-fallback-2026-07-01` with `"fallbacks": "default"`.
//! * Raw HTTPS because Rust has no official Anthropic SDK; TLS via rustls + `ring`.
//! * **Never** sends `temperature`, `top_p` or `top_k` (rejected by API version
//!   `2026-10-21`); `ModelRequest::temperature` is ignored by this adapter.
//! * Tool ids such as `knowledge.search` are mapped to API-safe names
//!   (`[A-Za-z0-9_-]`, ≤ 64 chars) and mapped back on `tool_use` blocks.
//! * The harness rebuilds context each step (system + user only) and never replays
//!   assistant content blocks, so thinking-block replay rules do not apply.
//! * Error mapping: 429/5xx/529 and transport errors → `unavailable` (retried here
//!   with `retry-after`, then eligible for `ModelClient` fallback); 408 → `timeout`;
//!   401/403 → `forbidden`; other 4xx → `invalid_input`; `stop_reason: "refusal"` →
//!   `policy_denied`. The API key never appears in errors, logs or `Debug` output.

use async_trait::async_trait;
use eaeh_core::{HarnessError, ModelId, ProviderId, Result, SecretString};
use eaeh_intelligence::{
    Egress, MessageRole, ModelDescriptor, ModelProvider, ModelRequest, ModelResponse, ProposedToolCall, Usage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_API_VERSION: &str = "2026-10-21";
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicModel {
    pub id: String,
    pub context_window: u64,
    pub max_output_tokens: u32,
    pub input_cost_per_mtok_micros: u64,
    pub output_cost_per_mtok_micros: u64,
    #[serde(default = "default_quality")]
    pub quality_tier: u8,
    #[serde(default = "default_caps")]
    pub capabilities: BTreeSet<String>,
}

fn default_quality() -> u8 {
    8
}
fn default_caps() -> BTreeSet<String> {
    ["tools".to_string(), "json".to_string()].into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnthropicConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_version: String,
    pub timeout_secs: u64,
    pub max_retries: u32,
    /// Server-side refusal fallback (`fallbacks: "default"`).
    pub refusal_fallback: bool,
    /// Optional `output_config.effort` (`low`…`max`).
    pub effort: Option<String>,
    pub models: Vec<AnthropicModel>,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: DEFAULT_BASE_URL.into(),
            api_version: DEFAULT_API_VERSION.into(),
            timeout_secs: 300,
            max_retries: 2,
            refusal_fallback: true,
            effort: None,
            models: vec![AnthropicModel {
                id: "claude-opus-5-5".into(),
                context_window: 1_000_000,
                max_output_tokens: 128_000,
                input_cost_per_mtok_micros: 4_000_000,
                output_cost_per_mtok_micros: 20_000_000,
                quality_tier: 9,
                capabilities: default_caps(),
            }],
        }
    }
}

pub struct AnthropicProvider {
    id: ProviderId,
    cfg: AnthropicConfig,
    api_key: SecretString,
    http: reqwest::Client,
}

impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicProvider")
            .field("base_url", &self.cfg.base_url)
            .field("api_version", &self.cfg.api_version)
            .field("api_key", &self.api_key)
            .finish()
    }
}

/// Map a tool id to an API-safe tool name.
pub fn encode_tool_name(id: &str) -> String {
    let mut s: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    s.truncate(64);
    s
}

impl AnthropicProvider {
    pub fn new(cfg: AnthropicConfig, api_key: SecretString) -> Result<Self> {
        if api_key.expose().trim().is_empty() {
            return Err(HarnessError::invalid_input("Anthropic API key is empty"));
        }
        if cfg.models.is_empty() {
            return Err(HarnessError::invalid_input(
                "at least one Anthropic model must be configured",
            ));
        }
        if let Some(e) = &cfg.effort
            && !matches!(e.as_str(), "low" | "medium" | "high" | "xhigh" | "max")
        {
            return Err(HarnessError::invalid_input(
                "effort must be low|medium|high|xhigh|max",
            ));
        }
        if !(cfg.base_url.starts_with("https://")
            || cfg.base_url.starts_with("http://127.0.0.1")
            || cfg.base_url.starts_with("http://localhost"))
        {
            return Err(HarnessError::invalid_input(
                "base_url must use https (plain http only for localhost testing)",
            ));
        }
        // Process-wide rustls crypto provider (ring); ignore if one is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.timeout_secs.max(1)))
            .build()
            .map_err(|e| HarnessError::internal(format!("http client: {e}")))?;
        Ok(Self {
            id: ProviderId::new("anthropic")?,
            cfg,
            api_key,
            http,
        })
    }

    /// Build the request body (exposed for tests and inspection; contains no credentials).
    pub fn build_body(&self, model: &ModelId, req: &ModelRequest) -> (Value, BTreeMap<String, String>) {
        let mut names = BTreeMap::new();
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                let mut name = encode_tool_name(&t.name);
                while names.contains_key(&name) && names.get(&name) != Some(&t.name) {
                    name.push('_');
                }
                names.insert(name.clone(), t.name.clone());
                json!({"name": name, "description": t.description, "input_schema": t.input_schema})
            })
            .collect();
        let system: Vec<&str> = req
            .messages
            .iter()
            .filter(|m| m.role == MessageRole::System)
            .map(|m| m.content.as_str())
            .collect();
        let mut messages: Vec<(String, String)> = vec![];
        for m in req.messages.iter().filter(|m| m.role != MessageRole::System) {
            let role = if m.role == MessageRole::Assistant {
                "assistant"
            } else {
                "user"
            };
            match messages.last_mut() {
                Some((r, c)) if r == role => {
                    c.push_str("\n\n");
                    c.push_str(&m.content);
                }
                _ => messages.push((role.into(), m.content.clone())),
            }
        }
        if messages.first().is_none_or(|(r, _)| r != "user") {
            messages.insert(0, ("user".into(), "Begin.".into()));
        }
        let mut body = json!({
            "model": model.as_str(),
            "max_tokens": req.max_output_tokens,
            "messages": messages.iter().map(|(r, c)| json!({"role": r, "content": c})).collect::<Vec<_>>(),
        });
        if !system.is_empty() {
            body["system"] = json!(system.join("\n\n"));
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
        if let Some(e) = &self.cfg.effort {
            body["output_config"] = json!({"effort": e});
        }
        if self.cfg.refusal_fallback {
            body["fallbacks"] = json!("default");
        }
        (body, names)
    }

    fn map_status(status: u16, body: &str) -> HarnessError {
        let msg: String = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(String::from)
            })
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        let text = format!("anthropic API {status}: {msg}");
        match status {
            429 | 500..=599 => HarnessError::unavailable(text),
            408 => HarnessError::timeout(text),
            401 | 403 => HarnessError::forbidden(text),
            _ => HarnessError::invalid_input(text),
        }
    }

    fn parse(resp: &Value, names: &BTreeMap<String, String>) -> Result<ModelResponse> {
        let stop = resp.get("stop_reason").and_then(Value::as_str).unwrap_or("");
        if stop == "refusal" {
            let cat = resp
                .pointer("/stop_details/category")
                .and_then(Value::as_str)
                .unwrap_or("unspecified");
            return Err(HarnessError::policy_denied(format!(
                "model declined the request (refusal, category {cat})"
            )));
        }
        let mut content = String::new();
        let mut tool_calls = vec![];
        for block in resp
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => content.push_str(block.get("text").and_then(Value::as_str).unwrap_or("")),
                Some("tool_use") => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                    tool_calls.push(ProposedToolCall {
                        tool: names.get(name).cloned().unwrap_or_else(|| name.to_string()),
                        input: block.get("input").cloned().unwrap_or(json!({})),
                    });
                }
                _ => {} // thinking / other blocks are not replayed by the harness
            }
        }
        let usage = Usage {
            input_tokens: resp
                .pointer("/usage/input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: resp
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        };
        Ok(ModelResponse {
            content,
            tool_calls,
            usage,
            model_version: resp
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            finish_reason: stop.to_string(),
        })
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn models(&self) -> Vec<ModelDescriptor> {
        self.cfg
            .models
            .iter()
            .filter_map(|m| {
                Some(ModelDescriptor {
                    id: ModelId::new(m.id.clone()).ok()?,
                    provider: self.id.clone(),
                    version: m.id.clone(),
                    capabilities: m.capabilities.clone(),
                    context_window: m.context_window,
                    max_output_tokens: m.max_output_tokens,
                    input_cost_per_mtok_micros: m.input_cost_per_mtok_micros,
                    output_cost_per_mtok_micros: m.output_cost_per_mtok_micros,
                    quality_tier: m.quality_tier,
                    residency: ["anthropic".to_string()].into(),
                })
            })
            .collect()
    }

    fn egress(&self) -> Egress {
        Egress::External
    }

    async fn complete(&self, model: &ModelId, req: &ModelRequest) -> Result<ModelResponse> {
        let (body, names) = self.build_body(model, req);
        let url = format!("{}/v1/messages", self.cfg.base_url.trim_end_matches('/'));
        let mut attempt = 0u32;
        loop {
            let mut rb = self
                .http
                .post(&url)
                .header("x-api-key", self.api_key.expose())
                .header("anthropic-version", &self.cfg.api_version)
                .header("content-type", "application/json");
            if self.cfg.refusal_fallback {
                rb = rb.header("anthropic-beta", FALLBACK_BETA);
            }
            let res = rb.json(&body).send().await;
            let err = match res {
                Ok(r) => {
                    let status = r.status().as_u16();
                    let retry_after = r
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok());
                    let text = r.text().await.unwrap_or_default();
                    if (200..300).contains(&status) {
                        let v: Value = serde_json::from_str(&text)
                            .map_err(|_| HarnessError::unavailable("anthropic API returned invalid JSON"))?;
                        return Self::parse(&v, &names);
                    }
                    let e = Self::map_status(status, &text);
                    if !e.is_retryable() || attempt >= self.cfg.max_retries {
                        return Err(e);
                    }
                    let wait = retry_after
                        .map(|s| s.min(30) * 1000)
                        .unwrap_or(500 * 2u64.pow(attempt.min(5)));
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                    e
                }
                Err(e) => {
                    // reqwest errors contain the URL, never request headers.
                    let he = if e.is_timeout() {
                        HarnessError::timeout("anthropic API request timed out")
                    } else {
                        HarnessError::unavailable(format!(
                            "anthropic API transport error: {}",
                            e.without_url()
                        ))
                    };
                    if attempt >= self.cfg.max_retries {
                        return Err(he);
                    }
                    tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt.min(5)))).await;
                    he
                }
            };
            tracing::warn!(attempt, error = %err, "anthropic request failed; retrying");
            attempt += 1;
        }
    }
}
