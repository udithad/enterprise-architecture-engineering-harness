//! EAEH validation framework (master §19).
//!
//! * [`schema::CompiledSchema`] – JSON-Schema contracts used for tool, agent and task
//!   input/output validation (remote `$ref` resolution is disabled at compile time).
//! * [`Validator`] – reusable validators with id, version, kind, applicability and
//!   evidence-bearing findings.
//! * [`QualityGate`] / [`ValidationEngine`] – gates aggregate validators. Gates fail
//!   closed (missing validator ⇒ failure) and **semantic (LLM-assisted) validators can
//!   never be the sole basis for passing**: at least one deterministic validator must run,
//!   and semantic findings only block when the gate explicitly allows it.

pub mod builtin;
pub mod schema;
pub mod shape;

use async_trait::async_trait;
use eaeh_core::{HarnessError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorKind {
    /// Pure, reproducible checks. Required for any gate decision.
    Deterministic,
    /// LLM-assisted semantic checks. Advisory unless the gate opts in to blocking.
    Semantic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub validator: String,
    pub severity: Severity,
    pub message: String,
    pub location: Option<String>,
    pub evidence: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub validator: String,
    pub version: String,
    pub kind: ValidatorKind,
    pub findings: Vec<Finding>,
}

impl ValidationReport {
    pub fn max_severity(&self) -> Option<Severity> {
        self.findings.iter().map(|f| f.severity).max()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationSubject {
    /// e.g. `tool_output`, `artifact`, `agent_output`.
    pub kind: String,
    pub name: String,
    pub content: Value,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[async_trait]
pub trait Validator: Send + Sync {
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    fn kind(&self) -> ValidatorKind;
    fn applies_to(&self, _subject: &ValidationSubject) -> bool {
        true
    }
    async fn validate(&self, subject: &ValidationSubject) -> ValidationReport;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityGate {
    pub id: String,
    pub validators: Vec<String>,
    /// Findings at or above this severity fail the gate.
    pub block_on: Severity,
    #[serde(default)]
    pub semantic_can_block: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateResult {
    pub gate: String,
    pub passed: bool,
    pub reports: Vec<ValidationReport>,
    pub blocking: Vec<Finding>,
    pub advisory: Vec<Finding>,
}

#[derive(Default)]
pub struct ValidationEngine {
    validators: RwLock<BTreeMap<String, Arc<dyn Validator>>>,
}

impl std::fmt::Debug for ValidationEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<String> = self
            .validators
            .read()
            .map(|v| v.keys().cloned().collect())
            .unwrap_or_default();
        f.debug_struct("ValidationEngine")
            .field("validators", &ids)
            .finish()
    }
}

impl ValidationEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, v: Arc<dyn Validator>) -> Result<()> {
        let mut m = self
            .validators
            .write()
            .map_err(|_| HarnessError::internal("validator lock poisoned"))?;
        if m.contains_key(v.id()) {
            return Err(HarnessError::conflict(format!(
                "validator '{}' already registered",
                v.id()
            )));
        }
        m.insert(v.id().to_string(), v);
        Ok(())
    }

    pub fn ids(&self) -> Vec<String> {
        self.validators
            .read()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn run_gate(&self, gate: &QualityGate, subject: &ValidationSubject) -> GateResult {
        let mut reports = vec![];
        let mut blocking = vec![];
        let mut advisory = vec![];
        let mut deterministic_ran = false;
        for id in &gate.validators {
            let v = self.validators.read().ok().and_then(|m| m.get(id).cloned());
            let Some(v) = v else {
                blocking.push(Finding {
                    validator: id.clone(),
                    severity: Severity::Critical,
                    message: format!("required validator '{id}' is not registered (gate fails closed)"),
                    location: None,
                    evidence: Value::Null,
                });
                continue;
            };
            if !v.applies_to(subject) {
                continue;
            }
            let report = v.validate(subject).await;
            if report.kind == ValidatorKind::Deterministic {
                deterministic_ran = true;
            }
            for f in &report.findings {
                let can_block = report.kind == ValidatorKind::Deterministic || gate.semantic_can_block;
                if f.severity >= gate.block_on && can_block {
                    blocking.push(f.clone());
                } else {
                    advisory.push(f.clone());
                }
            }
            reports.push(report);
        }
        if !deterministic_ran {
            blocking.push(Finding {
                validator: gate.id.clone(),
                severity: Severity::Critical,
                message: "no deterministic validator applied; semantic validation alone cannot pass a gate"
                    .into(),
                location: None,
                evidence: Value::Null,
            });
        }
        GateResult {
            gate: gate.id.clone(),
            passed: blocking.is_empty(),
            reports,
            blocking,
            advisory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{JsonSchemaValidator, SecretLeakValidator};

    struct Semantic(Severity);
    #[async_trait]
    impl Validator for Semantic {
        fn id(&self) -> &str {
            "semantic.review"
        }
        fn version(&self) -> &str {
            "1"
        }
        fn kind(&self) -> ValidatorKind {
            ValidatorKind::Semantic
        }
        async fn validate(&self, _s: &ValidationSubject) -> ValidationReport {
            ValidationReport {
                validator: "semantic.review".into(),
                version: "1".into(),
                kind: ValidatorKind::Semantic,
                findings: vec![Finding {
                    validator: "semantic.review".into(),
                    severity: self.0,
                    message: "llm says".into(),
                    location: None,
                    evidence: Value::Null,
                }],
            }
        }
    }

    fn subject(v: Value) -> ValidationSubject {
        ValidationSubject {
            kind: "agent_output".into(),
            name: "x".into(),
            content: v,
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn gate_requires_deterministic_and_fails_closed() {
        let e = ValidationEngine::new();
        e.register(Arc::new(Semantic(Severity::Info))).unwrap();
        e.register(Arc::new(
            JsonSchemaValidator::new(
                "schema.summary",
                serde_json::json!({"type":"object","required":["summary"]}),
            )
            .unwrap(),
        ))
        .unwrap();
        e.register(Arc::new(SecretLeakValidator)).unwrap();

        let only_semantic = QualityGate {
            id: "g1".into(),
            validators: vec!["semantic.review".into()],
            block_on: Severity::Error,
            semantic_can_block: false,
        };
        assert!(
            !e.run_gate(&only_semantic, &subject(serde_json::json!({})))
                .await
                .passed
        );

        let missing = QualityGate {
            id: "g2".into(),
            validators: vec!["does.not.exist".into()],
            block_on: Severity::Error,
            semantic_can_block: false,
        };
        assert!(!e.run_gate(&missing, &subject(serde_json::json!({}))).await.passed);

        let g = QualityGate {
            id: "g3".into(),
            validators: vec![
                "schema.summary".into(),
                "secret.leak".into(),
                "semantic.review".into(),
            ],
            block_on: Severity::Error,
            semantic_can_block: false,
        };
        assert!(
            e.run_gate(&g, &subject(serde_json::json!({"summary": "ok"})))
                .await
                .passed
        );
        let r = e.run_gate(&g, &subject(serde_json::json!({"nope": 1}))).await;
        assert!(!r.passed);
        assert_eq!(r.blocking[0].validator, "schema.summary");
        let leak = e
            .run_gate(
                &g,
                &subject(serde_json::json!({"summary": "key eaeh_0123456789abcdef0123"})),
            )
            .await;
        assert!(!leak.passed);
    }

    #[tokio::test]
    async fn semantic_findings_are_advisory_unless_opted_in() {
        let e = ValidationEngine::new();
        e.register(Arc::new(Semantic(Severity::Critical))).unwrap();
        e.register(Arc::new(SecretLeakValidator)).unwrap();
        let mut g = QualityGate {
            id: "g".into(),
            validators: vec!["secret.leak".into(), "semantic.review".into()],
            block_on: Severity::Error,
            semantic_can_block: false,
        };
        let r = e.run_gate(&g, &subject(serde_json::json!({"a": 1}))).await;
        assert!(r.passed);
        assert_eq!(r.advisory.len(), 1);
        g.semantic_can_block = true;
        assert!(!e.run_gate(&g, &subject(serde_json::json!({"a": 1}))).await.passed);
    }
}
