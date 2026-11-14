//! EAEH architecture generation support (master §30): typed HLD/LLD models drafted by
//! architect agents; deterministic validation, rendering (Markdown, Mermaid, ADRs) and
//! traceability by the platform. No dependency on agents, models or storage (ADR-0019).

pub mod mermaid;
pub mod model;
pub mod render;
pub mod validate;

pub use model::*;
pub use render::{Bundle, hld_trace, lld_trace, render_hld, render_lld};
pub use validate::{DesignContext, HLD_VALIDATOR, LLD_VALIDATOR, validate_hld, validate_lld};

use async_trait::async_trait;
use eaeh_requirements::subject_parts;
use eaeh_validation::{Finding, Severity, ValidationReport, ValidationSubject, Validator, ValidatorKind};
use serde_json::Value;

fn parse_err(validator: &str, what: &str, e: serde_json::Error) -> Finding {
    Finding {
        validator: validator.into(),
        severity: Severity::Error,
        message: format!("not a valid {what} document: {e}"),
        location: Some("$".into()),
        evidence: Value::Null,
    }
}

/// Parse the platform-supplied design context; malformed context is a finding (fail closed).
fn context(validator: &str, ctx: Option<&Value>) -> Result<DesignContext, Finding> {
    match ctx {
        None => Ok(DesignContext::default()),
        Some(c) => serde_json::from_value(c.clone()).map_err(|e| Finding {
            validator: validator.into(),
            severity: Severity::Critical,
            message: format!("invalid design context supplied by the platform: {e}"),
            location: Some("context".into()),
            evidence: Value::Null,
        }),
    }
}

/// Deterministic validator `architecture.hld`. Context: `requirements`, `standards`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HldValidator;

/// Deterministic validator `architecture.lld`. Context: `hld`, `requirements`, `standards`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LldValidator;

pub fn hld_findings(subject: &ValidationSubject) -> Vec<Finding> {
    let (doc, ctx) = subject_parts(subject);
    let h: Hld = match serde_json::from_value(doc.clone()) {
        Ok(h) => h,
        Err(e) => return vec![parse_err(HLD_VALIDATOR, "HLD", e)],
    };
    match context(HLD_VALIDATOR, ctx) {
        Ok(c) => validate_hld(&h, &c),
        Err(f) => vec![f],
    }
}

pub fn lld_findings(subject: &ValidationSubject) -> Vec<Finding> {
    let (doc, ctx) = subject_parts(subject);
    let l: Lld = match serde_json::from_value(doc.clone()) {
        Ok(l) => l,
        Err(e) => return vec![parse_err(LLD_VALIDATOR, "LLD", e)],
    };
    match context(LLD_VALIDATOR, ctx) {
        Ok(c) => validate_lld(&l, &c),
        Err(f) => vec![f],
    }
}

fn is_kind(subject: &ValidationSubject, kind: &str) -> bool {
    subject_parts(subject).0.get("document").and_then(Value::as_str) == Some(kind)
        || subject.metadata.get("document").map(String::as_str) == Some(kind)
}

#[async_trait]
impl Validator for HldValidator {
    fn id(&self) -> &str {
        HLD_VALIDATOR
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::Deterministic
    }
    fn applies_to(&self, subject: &ValidationSubject) -> bool {
        is_kind(subject, HLD_KIND)
    }
    async fn validate(&self, subject: &ValidationSubject) -> ValidationReport {
        ValidationReport {
            validator: HLD_VALIDATOR.into(),
            version: "1.0.0".into(),
            kind: ValidatorKind::Deterministic,
            findings: hld_findings(subject),
        }
    }
}

#[async_trait]
impl Validator for LldValidator {
    fn id(&self) -> &str {
        LLD_VALIDATOR
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::Deterministic
    }
    fn applies_to(&self, subject: &ValidationSubject) -> bool {
        is_kind(subject, LLD_KIND)
    }
    async fn validate(&self, subject: &ValidationSubject) -> ValidationReport {
        ValidationReport {
            validator: LLD_VALIDATOR.into(),
            version: "1.0.0".into(),
            kind: ValidatorKind::Deterministic,
            findings: lld_findings(subject),
        }
    }
}
