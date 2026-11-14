//! Secret handling.
//!
//! * [`SecretString`] never reveals its value through `Debug`, `Display` or `Serialize`.
//! * [`SecretsProvider`] is the replaceable abstraction for secret retrieval.
//! * [`Redactor`] removes registered secret values and known credential patterns from
//!   free text and JSON before it reaches logs, audit records or agent context.

use crate::error::{HarnessError, Result};
use serde::{Serialize, Serializer};
use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Arc, RwLock};

pub const REDACTED: &str = "[REDACTED]";

/// Prefix of every API token issued by EAEH; used for pattern-based redaction.
pub const TOKEN_PREFIX: &str = "eaeh_";

#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Explicit, greppable access to the secret value.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl Serialize for SecretString {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(REDACTED)
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        // Best-effort scrub; not a guarantee against copies made by the allocator.
        // SAFETY-free approach: overwrite via a fresh String of the same length.
        let len = self.0.len();
        self.0.clear();
        self.0.extend(std::iter::repeat_n('\0', len));
        self.0.clear();
    }
}

/// Replaceable secrets backend (environment, vault, cloud KMS…).
pub trait SecretsProvider: Send + Sync {
    fn name(&self) -> &str;
    fn get(&self, key: &str) -> Result<SecretString>;
}

/// Reads secrets from environment variables named `EAEH_SECRET_<KEY>` (upper-cased, `.`
/// and `-` mapped to `_`). Only that prefix is readable, so arbitrary environment
/// variables cannot be exfiltrated through this provider.
#[derive(Debug, Default)]
pub struct EnvSecretsProvider;

impl EnvSecretsProvider {
    pub fn var_name(key: &str) -> String {
        format!(
            "EAEH_SECRET_{}",
            key.to_ascii_uppercase().replace(['.', '-'], "_")
        )
    }
}

impl SecretsProvider for EnvSecretsProvider {
    fn name(&self) -> &str {
        "env"
    }

    fn get(&self, key: &str) -> Result<SecretString> {
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(HarnessError::invalid_input("invalid secret key"));
        }
        std::env::var(Self::var_name(key))
            .map(SecretString::new)
            .map_err(|_| HarnessError::not_found(format!("secret '{key}' not configured")))
    }
}

/// Shared redaction engine. Cheap to clone; registered secrets are shared.
#[derive(Clone, Default)]
pub struct Redactor {
    known: Arc<RwLock<BTreeSet<String>>>,
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.known.read().map(|k| k.len()).unwrap_or(0);
        write!(f, "Redactor {{ known_secrets: {n} }}")
    }
}

impl Redactor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a secret value so it is removed wherever it appears. Values shorter than
    /// 6 characters are ignored to avoid destroying unrelated text.
    pub fn register(&self, secret: &SecretString) {
        let v = secret.expose();
        if v.len() >= 6
            && let Ok(mut k) = self.known.write()
        {
            k.insert(v.to_string());
        }
    }

    pub fn redact_str(&self, input: &str) -> String {
        let mut out = input.to_string();
        if let Ok(known) = self.known.read() {
            // Longest first so overlapping secrets are fully removed.
            let mut list: Vec<&String> = known.iter().collect();
            list.sort_by_key(|s| std::cmp::Reverse(s.len()));
            for s in list {
                if out.contains(s.as_str()) {
                    out = out.replace(s.as_str(), REDACTED);
                }
            }
        }
        redact_patterns(&out)
    }

    pub fn redact_json(&self, value: &serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::String(s) => Value::String(self.redact_str(s)),
            Value::Array(a) => Value::Array(a.iter().map(|v| self.redact_json(v)).collect()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| {
                        if is_sensitive_key(k) {
                            (k.clone(), Value::String(REDACTED.into()))
                        } else {
                            (k.clone(), self.redact_json(v))
                        }
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

fn is_sensitive_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    [
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "authorization",
        "private_key",
        "credential",
    ]
    .iter()
    .any(|s| k.contains(s))
        && !k.ends_with("_id")
        && !k.ends_with("_hash")
        && !k.ends_with("tokens")
}

/// Pattern-based redaction for credentials that were never registered:
/// EAEH tokens (`eaeh_<hex>`) and `Bearer <value>` headers.
fn redact_patterns(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let next_token = rest.find(TOKEN_PREFIX);
        let next_bearer = find_ci(rest, "bearer ");
        let (idx, skip) = match (next_token, next_bearer) {
            (None, None) => break,
            (Some(t), None) => (t, TOKEN_PREFIX.len()),
            (None, Some(b)) => (b, "bearer ".len()),
            (Some(t), Some(b)) if t <= b => (t, TOKEN_PREFIX.len()),
            (Some(_), Some(b)) => (b, "bearer ".len()),
        };
        let after = &rest[idx + skip..];
        let tail_len = after
            .char_indices()
            .find(|(_, c)| {
                !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '~' | '+' | '/' | '='))
            })
            .map(|(i, _)| i)
            .unwrap_or(after.len());
        if tail_len >= 8 {
            out.push_str(&rest[..idx + skip]);
            out.push_str(REDACTED);
            rest = &after[tail_len..];
        } else {
            out.push_str(&rest[..idx + skip]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().find(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_never_formats_value() {
        let s = SecretString::new("hunter2-super-secret");
        assert_eq!(format!("{s:?}"), REDACTED);
        assert_eq!(format!("{s}"), REDACTED);
        assert_eq!(serde_json::to_string(&s).unwrap(), format!("\"{REDACTED}\""));
        assert_eq!(s.expose(), "hunter2-super-secret");
    }

    #[test]
    fn redactor_removes_registered_and_pattern_secrets() {
        let r = Redactor::new();
        r.register(&SecretString::new("s3cr3t-value-xyz"));
        let text = "pw=s3cr3t-value-xyz token eaeh_0123456789abcdef auth: Bearer abcdefghijkl end";
        let out = r.redact_str(text);
        assert!(!out.contains("s3cr3t-value-xyz"));
        assert!(!out.contains("0123456789abcdef"));
        assert!(!out.contains("abcdefghijkl"));
        assert!(out.contains("end"));
    }

    #[test]
    fn redactor_handles_json_and_sensitive_keys() {
        let r = Redactor::new();
        let v = serde_json::json!({"password": "x", "token_id": "keep", "nested": ["eaeh_aaaaaaaaaaaaaaaa"], "input_tokens": 5});
        let out = r.redact_json(&v);
        assert_eq!(out["password"], REDACTED);
        assert_eq!(out["token_id"], "keep");
        assert_eq!(out["input_tokens"], 5);
        assert_eq!(out["nested"][0], format!("eaeh_{REDACTED}"));
    }

    #[test]
    fn env_provider_is_prefix_restricted() {
        assert_eq!(
            EnvSecretsProvider::var_name("db.password"),
            "EAEH_SECRET_DB_PASSWORD"
        );
        assert!(EnvSecretsProvider.get("../x").is_err());
        assert!(EnvSecretsProvider.get("definitely-not-set-123").is_err());
    }
}
