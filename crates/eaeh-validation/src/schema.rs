use eaeh_core::{HarnessError, Result};
use serde_json::Value;
use std::sync::Arc;

/// Schema errors reported per validation (the total is always reported).
pub const MAX_REPORTED_ERRORS: usize = 60;

/// A compiled JSON Schema. Cheap to clone.
#[derive(Clone)]
pub struct CompiledSchema {
    schema: Arc<Value>,
    validator: Arc<jsonschema::Validator>,
}

impl std::fmt::Debug for CompiledSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledSchema")
            .field("schema", &self.schema)
            .finish()
    }
}

impl CompiledSchema {
    pub fn compile(schema: &Value) -> Result<Self> {
        let validator = jsonschema::validator_for(schema)
            .map_err(|e| HarnessError::invalid_input(format!("invalid JSON schema: {e}")))?;
        Ok(Self {
            schema: Arc::new(schema.clone()),
            validator: Arc::new(validator),
        })
    }

    pub fn schema(&self) -> &Value {
        &self.schema
    }

    /// Up to [`MAX_REPORTED_ERRORS`] error messages (`<instance path>: <message>`).
    pub fn errors(&self, instance: &Value) -> Vec<String> {
        self.errors_with_total(instance).0
    }

    /// Up to [`MAX_REPORTED_ERRORS`] messages and the total number of errors, so feedback
    /// can say how many remain instead of revealing them a few at a time.
    pub fn errors_with_total(&self, instance: &Value) -> (Vec<String>, usize) {
        let mut shown = vec![];
        let mut total = 0;
        for e in self.validator.iter_errors(instance) {
            total += 1;
            if shown.len() < MAX_REPORTED_ERRORS {
                let path = e.instance_path().to_string();
                shown.push(format!(
                    "{}: {e}",
                    if path.is_empty() {
                        "(top level)".to_string()
                    } else {
                        path
                    }
                ));
            }
        }
        (shown, total)
    }

    pub fn check(&self, instance: &Value, what: &str) -> Result<()> {
        let (errs, total) = self.errors_with_total(instance);
        if errs.is_empty() {
            Ok(())
        } else {
            Err(HarnessError::invalid_input(format!(
                "{what} failed schema validation ({total} problem(s){}): {}",
                if total > errs.len() {
                    format!(", first {} shown", errs.len())
                } else {
                    ", all shown".to_string()
                },
                errs.join("; ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_and_checks() {
        let s = CompiledSchema::compile(&serde_json::json!({
            "type": "object", "required": ["path"], "additionalProperties": false,
            "properties": {"path": {"type": "string", "maxLength": 10}}
        }))
        .unwrap();
        assert!(s.check(&serde_json::json!({"path": "a"}), "input").is_ok());
        let e = s
            .check(&serde_json::json!({"path": "a", "x": 1}), "input")
            .unwrap_err();
        assert!(e.message.contains("input failed schema validation"));
        assert!(CompiledSchema::compile(&serde_json::json!({"type": 5})).is_err());
    }

    #[test]
    fn remote_refs_are_not_fetched() {
        let r = CompiledSchema::compile(&serde_json::json!({"$ref": "https://example.com/schema.json"}));
        assert!(r.is_err(), "remote $ref resolution must be disabled");
    }
}
