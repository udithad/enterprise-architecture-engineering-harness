//! Phase 3 integration of the deterministic requirements/architecture crates into the
//! harness: governed read-only tools (invoked only through the tool gateway), validator
//! registration and the default quality gates referenced by design workflows.

pub mod delivery;

use async_trait::async_trait;
use eaeh_architecture::{Hld, HldValidator, Lld, LldValidator, hld_trace, lld_trace, render_hld, render_lld};
use eaeh_core::{HarnessError, Result, RiskClass, ToolId};
use eaeh_knowledge::KnowledgeService;
use eaeh_requirements::{AnalysisConfig, AnalysisContext, BrdSection, RequirementsValidator};
use eaeh_tools::{Tool, ToolContext, ToolDescriptor};
use eaeh_validation::{QualityGate, Severity, ValidationEngine, ValidationSubject};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Register the deterministic Phase 3 validators.
pub fn register_validators(engine: &ValidationEngine, cfg: AnalysisConfig) -> Result<()> {
    engine.register(Arc::new(RequirementsValidator::new(cfg)))?;
    engine.register(Arc::new(HldValidator))?;
    engine.register(Arc::new(LldValidator))?;
    engine.register(Arc::new(delivery::DeliveryPlanValidator))?;
    engine.register(Arc::new(delivery::ImplementationValidator))?;
    engine.register(Arc::new(delivery::BuildResultValidator))?;
    Ok(())
}

/// Default quality gates (`requirements`, `hld`, `lld`): block on errors; semantic
/// validators can never block on their own.
pub fn default_gates() -> Vec<QualityGate> {
    [
        ("requirements", eaeh_requirements::VALIDATOR_ID),
        ("hld", eaeh_architecture::HLD_VALIDATOR),
        ("lld", eaeh_architecture::LLD_VALIDATOR),
        ("delivery-plan", delivery::PLAN_VALIDATOR),
        ("delivery", delivery::IMPLEMENTATION_VALIDATOR),
        ("build", delivery::BUILD_VALIDATOR),
    ]
    .into_iter()
    .map(|(id, v)| QualityGate {
        id: id.into(),
        validators: vec![v.into()],
        block_on: Severity::Error,
        semantic_can_block: false,
    })
    .collect()
}

fn descriptor(
    id: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
) -> Result<ToolDescriptor> {
    Ok(ToolDescriptor {
        id: ToolId::new(id)?,
        version: semver::Version::new(1, 0, 0),
        description: description.into(),
        risk: RiskClass::ReadOnly,
        input_schema,
        output_schema,
        isolation: Default::default(),
        needs_execution: false,
        needs_workspace: false,
        timeout_ms: 30_000,
        rate_limit_per_minute: Some(120),
        idempotent: true,
        max_retries: 0,
    })
}

/// `brd.sections` – the sections (citations + headings) of a governed BRD document visible
/// to the caller. Used by workflows to give analysts and the analyzer the BRD outline.
pub struct BrdSectionsTool {
    d: ToolDescriptor,
    svc: KnowledgeService,
}

impl BrdSectionsTool {
    pub fn new(svc: KnowledgeService) -> Result<Self> {
        Ok(Self {
            d: descriptor(
                "brd.sections",
                "List the sections (citation and heading) of a governed BRD document 'source/subject@vN'.",
                json!({"type":"object","additionalProperties":false,"required":["document"],
                       "properties":{"document":{"type":"string","minLength":3,"maxLength":300}}}),
                json!({"type":"object","required":["document","sections"]}),
            )?,
            svc,
        })
    }
}

#[async_trait]
impl Tool for BrdSectionsTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.d
    }
    async fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value> {
        let doc = input
            .get("document")
            .and_then(Value::as_str)
            .ok_or_else(|| HarnessError::invalid_input("document required"))?;
        let rctx = if ctx.principal.is_agent() {
            self.svc
                .context_for_agent(&ctx.principal, ctx.scope, None)
                .await?
        } else {
            self.svc
                .context_for_human(&ctx.principal, ctx.scope.project)
                .await?
        };
        let hits = self.svc.read_document(&rctx, doc, ctx.trace).await?;
        let sections: Vec<BrdSection> = hits
            .iter()
            .map(|h| BrdSection {
                citation: h.citation.clone(),
                heading: h.heading.clone(),
            })
            .collect();
        Ok(json!({
            "document": doc.split('#').next().unwrap_or(doc),
            "title": hits.first().map(|h| h.title.clone()).unwrap_or_default(),
            "classification": hits.first().map(|h| h.classification),
            "sections": sections,
        }))
    }
}

/// `requirements.render` – deterministic analysis + Markdown rendering + trace links of a
/// requirements document. Output is a bundle (`files`, `trace`, `summary`).
pub struct RequirementsRenderTool {
    d: ToolDescriptor,
    cfg: AnalysisConfig,
}

impl RequirementsRenderTool {
    pub fn new(cfg: AnalysisConfig) -> Result<Self> {
        Ok(Self {
            d: descriptor(
                "requirements.render",
                "Analyse (ambiguity, conflicts, gaps, BRD coverage) and render a requirements document.",
                json!({"type":"object","additionalProperties":false,"required":["requirements"],
                       "properties":{"requirements":{"type":"object"},"brd":{"type":"object"}}}),
                json!({"type":"object","required":["files","trace"]}),
            )?,
            cfg,
        })
    }
}

#[async_trait]
impl Tool for RequirementsRenderTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.d
    }
    async fn invoke(&self, _ctx: &ToolContext, input: Value) -> Result<Value> {
        let set = eaeh_requirements::parse(&input["requirements"])
            .map_err(|f| HarnessError::invalid_input(f.message))?;
        let actx = AnalysisContext {
            brd_sections: input
                .pointer("/brd/sections")
                .cloned()
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default(),
        };
        let a = eaeh_requirements::analyze(&set, &actx, &self.cfg);
        let files = BTreeMap::from([
            (
                "requirements.md".to_string(),
                eaeh_requirements::render_markdown(&set, &a),
            ),
            (
                "requirements.json".to_string(),
                serde_json::to_string_pretty(&set)?,
            ),
            ("analysis.json".to_string(), serde_json::to_string_pretty(&a)?),
        ]);
        Ok(json!({
            "document": "requirements",
            "files": files,
            "trace": eaeh_requirements::trace_links(&set),
            "summary": {"metrics": a.metrics, "coverage": a.coverage,
                        "blocking": a.blocking().count(), "issues": set.issues.len() + a.issues.len()},
        }))
    }
}

/// `architecture.render` – deterministic HLD/LLD rendering (Markdown, Mermaid, ADRs) and
/// trace links. Input `{model, hld?}`.
pub struct ArchitectureRenderTool {
    d: ToolDescriptor,
}

impl ArchitectureRenderTool {
    pub fn new() -> Result<Self> {
        Ok(Self {
            d: descriptor(
                "architecture.render",
                "Render an HLD or LLD model into Markdown, Mermaid diagrams and ADRs.",
                json!({"type":"object","additionalProperties":false,"required":["model"],
                       "properties":{"model":{"type":"object"},"hld":{"type":"object"}}}),
                json!({"type":"object","required":["files","trace"]}),
            )?,
        })
    }
}

#[async_trait]
impl Tool for ArchitectureRenderTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.d
    }
    async fn invoke(&self, _ctx: &ToolContext, input: Value) -> Result<Value> {
        let model = &input["model"];
        let (doc, bundle, trace) = match model.get("document").and_then(Value::as_str) {
            Some("hld") => {
                let h: Hld = serde_json::from_value(model.clone())
                    .map_err(|e| HarnessError::invalid_input(format!("invalid HLD: {e}")))?;
                ("hld", render_hld(&h), hld_trace(&h))
            }
            Some("lld") => {
                let l: Lld = serde_json::from_value(model.clone())
                    .map_err(|e| HarnessError::invalid_input(format!("invalid LLD: {e}")))?;
                let hld: Option<Hld> = match input.get("hld") {
                    Some(h) if !h.is_null() => Some(
                        serde_json::from_value(h.clone())
                            .map_err(|e| HarnessError::invalid_input(format!("invalid HLD context: {e}")))?,
                    ),
                    _ => None,
                };
                ("lld", render_lld(&l, hld.as_ref()), lld_trace(&l))
            }
            _ => {
                return Err(HarnessError::invalid_input(
                    "model.document must be 'hld' or 'lld'",
                ));
            }
        };
        let mut files = bundle.files;
        files.insert(format!("{doc}.json"), serde_json::to_string_pretty(model)?);
        Ok(json!({
            "document": doc,
            "files": files,
            "trace": trace,
            "summary": {"files": files.len(), "trace_links": trace.len()},
        }))
    }
}

/// `design.check` – lets an agent run the deterministic validators on a draft before
/// answering. Findings are advisory data for the agent; the platform re-validates anyway.
pub struct DesignCheckTool {
    d: ToolDescriptor,
    engine: Arc<ValidationEngine>,
}

impl DesignCheckTool {
    pub fn new(engine: Arc<ValidationEngine>) -> Result<Self> {
        Ok(Self {
            d: descriptor(
                "design.check",
                "Validate a draft requirements, HLD or LLD document deterministically. Input: {document, context?}. Returns findings.",
                json!({"type":"object","additionalProperties":false,"required":["document"],
                       "properties":{"document":{"type":"object"},"context":{"type":"object"}}}),
                json!({"type":"object","required":["passed","findings"]}),
            )?,
            engine,
        })
    }
}

#[async_trait]
impl Tool for DesignCheckTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.d
    }
    async fn invoke(&self, _ctx: &ToolContext, input: Value) -> Result<Value> {
        let gate_id = match input.pointer("/document/document").and_then(Value::as_str) {
            Some("requirements") => "requirements",
            Some("hld") => "hld",
            Some("lld") => "lld",
            _ => {
                return Err(HarnessError::invalid_input(
                    "document.document must be requirements, hld or lld",
                ));
            }
        };
        let gate = default_gates()
            .into_iter()
            .find(|g| g.id == gate_id)
            .ok_or_else(|| HarnessError::internal("gate missing"))?;
        let r = self
            .engine
            .run_gate(
                &gate,
                &ValidationSubject {
                    kind: "draft".into(),
                    name: gate_id.into(),
                    content: json!({"output": input["document"], "context": input.get("context").cloned().unwrap_or(json!({}))}),
                    metadata: Default::default(),
                },
            )
            .await;
        Ok(json!({
            "passed": r.passed,
            "findings": r.blocking.iter().chain(r.advisory.iter()).take(100).map(|f| json!({
                "severity": f.severity, "location": f.location, "message": f.message})).collect::<Vec<_>>(),
        }))
    }
}
