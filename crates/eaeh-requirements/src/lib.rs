//! EAEH requirements engineering (master §30): a typed requirements model drafted by
//! analyst agents and checked, analysed, traced and rendered **deterministically** by the
//! platform. This crate has no dependency on agents, models or storage (ADR-0018).

pub mod analysis;
pub mod model;
pub mod render;

pub use analysis::{
    Analysis, AnalysisConfig, AnalysisContext, Coverage, Metrics, VALIDATOR_ID, analyze, parse,
};
pub use model::*;
pub use render::render_markdown;

use async_trait::async_trait;
use eaeh_validation::{Finding, ValidationReport, ValidationSubject, Validator, ValidatorKind};
use serde_json::Value;

/// Split a validation subject into `(document, context)`.
///
/// Subjects are either the document itself or `{"output": <document>, "context": {...}}`
/// (the form used by workflow quality gates and the agent correction loop).
pub fn subject_parts(subject: &ValidationSubject) -> (&Value, Option<&Value>) {
    match (subject.content.get("output"), subject.content.get("document")) {
        (Some(o), None) => (o, subject.content.get("context")),
        _ => (&subject.content, None),
    }
}

/// Deterministic validator `requirements.set` for quality gates and agent output
/// validation. Context key `brd_sections` (array of `{citation, heading}`) enables coverage.
#[derive(Debug, Clone, Default)]
pub struct RequirementsValidator {
    pub config: AnalysisConfig,
}

impl RequirementsValidator {
    pub fn new(config: AnalysisConfig) -> Self {
        Self { config }
    }

    pub fn findings(&self, subject: &ValidationSubject) -> Vec<Finding> {
        let (doc, ctx) = subject_parts(subject);
        let set = match parse(doc) {
            Ok(s) => s,
            Err(f) => return vec![f],
        };
        let ctx: AnalysisContext = ctx
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or_default();
        analyze(&set, &ctx, &self.config).findings
    }
}

#[async_trait]
impl Validator for RequirementsValidator {
    fn id(&self) -> &str {
        VALIDATOR_ID
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::Deterministic
    }
    fn applies_to(&self, subject: &ValidationSubject) -> bool {
        subject_parts(subject).0.get("document").and_then(Value::as_str) == Some(DOCUMENT_KIND)
            || subject.metadata.get("document").map(String::as_str) == Some(DOCUMENT_KIND)
    }
    async fn validate(&self, subject: &ValidationSubject) -> ValidationReport {
        ValidationReport {
            validator: VALIDATOR_ID.into(),
            version: self.version().into(),
            kind: ValidatorKind::Deterministic,
            findings: self.findings(subject),
        }
    }
}
