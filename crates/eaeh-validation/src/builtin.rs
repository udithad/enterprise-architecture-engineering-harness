//! Built-in deterministic validators.

use crate::schema::CompiledSchema;
use crate::{Finding, Severity, ValidationReport, ValidationSubject, Validator, ValidatorKind};
use async_trait::async_trait;
use eaeh_core::Result;
use serde_json::{Value, json};

fn report(id: &str, findings: Vec<Finding>) -> ValidationReport {
    ValidationReport {
        validator: id.into(),
        version: "1.0.0".into(),
        kind: ValidatorKind::Deterministic,
        findings,
    }
}

/// Validates subject content against a JSON Schema.
#[derive(Debug, Clone)]
pub struct JsonSchemaValidator {
    id: String,
    schema: CompiledSchema,
}

impl JsonSchemaValidator {
    pub fn new(id: &str, schema: Value) -> Result<Self> {
        Ok(Self {
            id: id.into(),
            schema: CompiledSchema::compile(&schema)?,
        })
    }
}

#[async_trait]
impl Validator for JsonSchemaValidator {
    fn id(&self) -> &str {
        &self.id
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::Deterministic
    }
    async fn validate(&self, s: &ValidationSubject) -> ValidationReport {
        let findings = self
            .schema
            .errors(&s.content)
            .into_iter()
            .map(|m| Finding {
                validator: self.id.clone(),
                severity: Severity::Error,
                message: m,
                location: None,
                evidence: json!({"schema": self.schema.schema()}),
            })
            .collect();
        report(&self.id, findings)
    }
}

/// Detects credential material (EAEH tokens, PEM private keys, AWS access key ids).
#[derive(Debug, Clone, Copy, Default)]
pub struct SecretLeakValidator;

const PATTERNS: [(&str, &str); 4] = [
    ("eaeh_", "EAEH API token"),
    ("-----BEGIN PRIVATE KEY-----", "PEM private key"),
    ("-----BEGIN RSA PRIVATE KEY-----", "PEM RSA private key"),
    ("AKIA", "AWS access key id"),
];

#[async_trait]
impl Validator for SecretLeakValidator {
    fn id(&self) -> &str {
        "secret.leak"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::Deterministic
    }
    async fn validate(&self, s: &ValidationSubject) -> ValidationReport {
        let text = s.content.to_string();
        let mut findings = vec![];
        for (pat, label) in PATTERNS {
            if let Some(pos) = text.find(pat) {
                // Require some credential-like tail to limit false positives on prose.
                let tail = &text[pos + pat.len()..];
                let tail_len = tail.chars().take_while(|c| c.is_ascii_alphanumeric()).count();
                if pat.starts_with("-----") || tail_len >= 12 {
                    findings.push(Finding {
                        validator: "secret.leak".into(),
                        severity: Severity::Critical,
                        message: format!("possible {label} in {} '{}'", s.kind, s.name),
                        location: Some(format!("offset {pos}")),
                        evidence: json!({"pattern": label}),
                    });
                }
            }
        }
        report("secret.leak", findings)
    }
}

/// Rejects content larger than a byte limit.
#[derive(Debug, Clone, Copy)]
pub struct MaxSizeValidator(pub usize);

#[async_trait]
impl Validator for MaxSizeValidator {
    fn id(&self) -> &str {
        "size.max"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn kind(&self) -> ValidatorKind {
        ValidatorKind::Deterministic
    }
    async fn validate(&self, s: &ValidationSubject) -> ValidationReport {
        let n = s.content.to_string().len();
        let findings = if n > self.0 {
            vec![Finding {
                validator: "size.max".into(),
                severity: Severity::Error,
                message: format!("content is {n} bytes (limit {})", self.0),
                location: None,
                evidence: json!({"size": n, "limit": self.0}),
            }]
        } else {
            vec![]
        };
        report("size.max", findings)
    }
}
