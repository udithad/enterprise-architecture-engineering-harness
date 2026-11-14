//! **TEST-ONLY.** Deterministic test doubles and fixtures for EAEH.
//!
//! [`ScriptedModelProvider`] is a test double for the LLM provider abstraction (D-04). It
//! returns pre-programmed responses and records the requests it received. It is not a
//! model, makes no network calls, and must never be registered by a production
//! composition root (the architecture fitness test rejects any non-dev dependency on
//! this crate).

use async_trait::async_trait;
use eaeh_core::{HarnessError, ModelId, ProviderId, Result};
use eaeh_intelligence::{
    ModelDescriptor, ModelProvider, ModelRequest, ModelResponse, ProposedToolCall, Usage,
};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

/// One scripted step.
#[derive(Debug, Clone)]
pub enum Script {
    /// Final answer text (agents expect JSON).
    Answer(String),
    /// Request tool calls.
    ToolCalls(Vec<(String, Value)>),
    /// Provider failure with the given error.
    Fail(HarnessError),
}

impl Script {
    pub fn json(v: Value) -> Self {
        Script::Answer(v.to_string())
    }
    pub fn tool(name: &str, input: Value) -> Self {
        Script::ToolCalls(vec![(name.to_string(), input)])
    }
}

pub struct ScriptedModelProvider {
    id: ProviderId,
    model: ModelDescriptor,
    script: Mutex<VecDeque<Script>>,
    /// Response used once the script is exhausted (default: error).
    fallback: Option<Script>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl std::fmt::Debug for ScriptedModelProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedModelProvider (TEST DOUBLE)")
            .field("id", &self.id)
            .finish()
    }
}

impl ScriptedModelProvider {
    pub fn new(provider: &str, model: &str, steps: Vec<Script>) -> Self {
        let id = ProviderId::new(provider).unwrap_or_else(|_| panic!("invalid provider id"));
        let model = ModelDescriptor {
            id: ModelId::new(model).unwrap_or_else(|_| panic!("invalid model id")),
            provider: id.clone(),
            version: format!("{model}-scripted-1"),
            capabilities: ["tools".to_string(), "json".to_string()].into(),
            context_window: 200_000,
            max_output_tokens: 8192,
            input_cost_per_mtok_micros: 1_000_000,
            output_cost_per_mtok_micros: 2_000_000,
            quality_tier: 5,
            residency: ["test".to_string()].into(),
        };
        Self {
            id,
            model,
            script: Mutex::new(steps.into()),
            fallback: None,
            requests: Mutex::new(vec![]),
        }
    }

    pub fn repeat_when_exhausted(mut self, s: Script) -> Self {
        self.fallback = Some(s);
        self
    }

    pub fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }

    pub fn remaining(&self) -> usize {
        self.script.lock().map(|s| s.len()).unwrap_or(0)
    }
}

#[async_trait]
impl ModelProvider for ScriptedModelProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn models(&self) -> Vec<ModelDescriptor> {
        vec![self.model.clone()]
    }

    async fn complete(&self, _model: &ModelId, request: &ModelRequest) -> Result<ModelResponse> {
        if let Ok(mut r) = self.requests.lock() {
            r.push(request.clone());
        }
        let next = self
            .script
            .lock()
            .ok()
            .and_then(|mut s| s.pop_front())
            .or_else(|| self.fallback.clone());
        let usage = Usage {
            input_tokens: request
                .messages
                .iter()
                .map(|m| m.content.len() as u64 / 4 + 1)
                .sum(),
            output_tokens: 20,
        };
        let base = |content: String, tool_calls: Vec<ProposedToolCall>| ModelResponse {
            content,
            tool_calls,
            usage,
            model_version: self.model.version.clone(),
            finish_reason: "scripted".into(),
        };
        match next {
            Some(Script::Answer(a)) => Ok(base(a, vec![])),
            Some(Script::ToolCalls(calls)) => Ok(base(
                "calling tools".into(),
                calls
                    .into_iter()
                    .map(|(tool, input)| ProposedToolCall { tool, input })
                    .collect(),
            )),
            Some(Script::Fail(e)) => Err(e),
            None => Err(HarnessError::unavailable("scripted provider exhausted")),
        }
    }
}
