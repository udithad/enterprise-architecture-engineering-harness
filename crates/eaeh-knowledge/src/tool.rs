//! `knowledge.search` – read-only governed tool giving agents scoped retrieval. Invoked
//! only through the tool gateway (authorization, allow-list, autonomy, policy, budget,
//! audit); results are cited and returned as untrusted data.

use crate::model::{SearchMode, SearchQuery};
use crate::service::KnowledgeService;
use async_trait::async_trait;
use eaeh_core::{HarnessError, Result, RiskClass, ToolId};
use eaeh_tools::{Tool, ToolContext, ToolDescriptor};
use serde_json::{Value, json};

pub struct KnowledgeSearchTool {
    d: ToolDescriptor,
    svc: KnowledgeService,
}

impl KnowledgeSearchTool {
    pub fn new(svc: KnowledgeService) -> Result<Self> {
        Ok(Self {
            d: ToolDescriptor {
                id: ToolId::new("knowledge.search")?,
                version: semver::Version::new(1, 0, 0),
                description: "Search governed enterprise knowledge visible to this task. Returns cited excerpts (cite them as given).".into(),
                risk: RiskClass::ReadOnly,
                input_schema: json!({"type":"object","additionalProperties":false,"required":["query"],"properties":{
                    "query":{"type":"string","minLength":1,"maxLength":1000},
                    "k":{"type":"integer","minimum":1,"maximum":10},
                    "mode":{"enum":["lexical","vector","hybrid"]}}}),
                output_schema: json!({"type":"object","required":["hits"],"properties":{"hits":{"type":"array"}}}),
                isolation: Default::default(),
                needs_execution: false,
                needs_workspace: false,
                timeout_ms: 10_000,
                rate_limit_per_minute: Some(120),
                idempotent: true,
                max_retries: 0,
            },
            svc,
        })
    }
}

#[async_trait]
impl Tool for KnowledgeSearchTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.d
    }

    async fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value> {
        let text = input
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| HarnessError::invalid_input("query required"))?;
        let k = input.get("k").and_then(Value::as_u64).unwrap_or(5) as usize;
        let mode: SearchMode = match input.get("mode") {
            Some(m) => serde_json::from_value(m.clone())?,
            None => SearchMode::Hybrid,
        };
        let rctx = if ctx.principal.is_agent() {
            self.svc
                .context_for_agent(&ctx.principal, ctx.scope, None)
                .await?
        } else {
            self.svc
                .context_for_human(&ctx.principal, ctx.scope.project)
                .await?
        };
        let hits = self
            .svc
            .search(&rctx, &SearchQuery::new(text, mode, k), ctx.trace)
            .await?;
        Ok(json!({"hits": hits.iter().map(|h| json!({
            "citation": h.citation, "title": h.title, "heading": h.heading, "text": h.text,
            "kind": h.source_kind, "mandatory": h.mandatory, "stale": h.stale, "conflict": h.conflict,
            "classification": h.classification,
        })).collect::<Vec<_>>()}))
    }
}

/// `knowledge.read` – read every chunk of one visible document (e.g. a BRD) with
/// citations. Read-only, governed like `knowledge.search` (Phase 3).
pub struct KnowledgeReadTool {
    d: ToolDescriptor,
    svc: KnowledgeService,
}

impl KnowledgeReadTool {
    pub fn new(svc: KnowledgeService) -> Result<Self> {
        Ok(Self {
            d: ToolDescriptor {
                id: ToolId::new("knowledge.read")?,
                version: semver::Version::new(1, 0, 0),
                description: "Read all sections of one governed knowledge document visible to this task, given its citation 'source/subject@vN'. Returns cited sections (cite them as given).".into(),
                risk: RiskClass::ReadOnly,
                input_schema: json!({"type":"object","additionalProperties":false,"required":["document"],"properties":{
                    "document":{"type":"string","minLength":3,"maxLength":300}}}),
                output_schema: json!({"type":"object","required":["hits"],"properties":{"hits":{"type":"array"}}}),
                isolation: Default::default(),
                needs_execution: false,
                needs_workspace: false,
                timeout_ms: 10_000,
                rate_limit_per_minute: Some(60),
                idempotent: true,
                max_retries: 0,
            },
            svc,
        })
    }
}

/// Retrieval context of the tool caller: agents are project-bound; services and humans use
/// the human context of the target project (authorization still applies).
pub(crate) async fn caller_context(
    svc: &KnowledgeService,
    ctx: &ToolContext,
) -> Result<crate::RetrievalContext> {
    if ctx.principal.is_agent() {
        svc.context_for_agent(&ctx.principal, ctx.scope, None).await
    } else {
        svc.context_for_human(&ctx.principal, ctx.scope.project).await
    }
}

#[async_trait]
impl Tool for KnowledgeReadTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.d
    }

    async fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value> {
        let doc = input
            .get("document")
            .and_then(Value::as_str)
            .ok_or_else(|| HarnessError::invalid_input("document required"))?;
        let rctx = caller_context(&self.svc, ctx).await?;
        let hits = self.svc.read_document(&rctx, doc, ctx.trace).await?;
        Ok(json!({"hits": hits.iter().map(|h| json!({
            "citation": h.citation, "title": h.title, "heading": h.heading, "text": h.text,
            "kind": h.source_kind, "classification": h.classification,
        })).collect::<Vec<_>>()}))
    }
}
