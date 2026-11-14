//! Interceptors inspect or transform a value as it crosses a defined boundary
//! (e.g. tool output entering agent context, text entering logs). They are synchronous,
//! side-effect free with respect to control flow, and run in registration order.

use eaeh_core::{HarnessError, Redactor, Result};
use serde_json::Value;
use std::sync::{Arc, RwLock};

pub trait Interceptor<T>: Send + Sync {
    fn name(&self) -> &str;
    fn intercept(&self, value: T) -> Result<T>;
}

pub struct InterceptorChain<T> {
    list: RwLock<Vec<Arc<dyn Interceptor<T>>>>,
}

impl<T> Default for InterceptorChain<T> {
    fn default() -> Self {
        Self {
            list: RwLock::new(Vec::new()),
        }
    }
}

impl<T> std::fmt::Debug for InterceptorChain<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self
            .list
            .read()
            .map(|l| l.iter().map(|i| i.name().to_string()).collect())
            .unwrap_or_default();
        f.debug_struct("InterceptorChain")
            .field("interceptors", &names)
            .finish()
    }
}

impl<T> InterceptorChain<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&self, i: Arc<dyn Interceptor<T>>) -> Result<()> {
        let mut l = self
            .list
            .write()
            .map_err(|_| HarnessError::internal("interceptor lock poisoned"))?;
        if l.iter().any(|x| x.name() == i.name()) {
            return Err(HarnessError::conflict(format!(
                "interceptor '{}' already registered",
                i.name()
            )));
        }
        l.push(i);
        Ok(())
    }

    pub fn apply(&self, mut value: T) -> Result<T> {
        let snapshot: Vec<Arc<dyn Interceptor<T>>> = self.list.read().map(|l| l.clone()).unwrap_or_default();
        for i in snapshot {
            value = i
                .intercept(value)
                .map_err(|e| e.context(format!("interceptor {}", i.name())))?;
        }
        Ok(value)
    }
}

/// Redacts registered secrets and credential patterns from JSON values.
#[derive(Debug, Clone)]
pub struct RedactionInterceptor(pub Redactor);

impl Interceptor<Value> for RedactionInterceptor {
    fn name(&self) -> &str {
        "redaction"
    }
    fn intercept(&self, value: Value) -> Result<Value> {
        Ok(self.0.redact_json(&value))
    }
}

impl Interceptor<String> for RedactionInterceptor {
    fn name(&self) -> &str {
        "redaction"
    }
    fn intercept(&self, value: String) -> Result<String> {
        Ok(self.0.redact_str(&value))
    }
}

/// Rejects values whose serialized size exceeds a limit (protects context budgets and
/// storage from oversized tool output).
#[derive(Debug, Clone, Copy)]
pub struct SizeLimitInterceptor(pub usize);

impl Interceptor<Value> for SizeLimitInterceptor {
    fn name(&self) -> &str {
        "size-limit"
    }
    fn intercept(&self, value: Value) -> Result<Value> {
        let n = serde_json::to_vec(&value).map(|v| v.len()).unwrap_or(usize::MAX);
        if n > self.0 {
            return Err(HarnessError::invalid_input(format!(
                "value of {n} bytes exceeds limit {}",
                self.0
            )));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eaeh_core::SecretString;

    #[test]
    fn chain_applies_in_order_and_stops_on_error() {
        let r = Redactor::new();
        r.register(&SecretString::new("topsecret123"));
        let c: InterceptorChain<Value> = InterceptorChain::new();
        c.add(Arc::new(RedactionInterceptor(r))).unwrap();
        c.add(Arc::new(SizeLimitInterceptor(64))).unwrap();
        let out = c.apply(serde_json::json!({"out": "x topsecret123"})).unwrap();
        assert!(!out.to_string().contains("topsecret123"));
        assert!(c.apply(serde_json::json!({"out": "y".repeat(100)})).is_err());
        assert!(c.add(Arc::new(SizeLimitInterceptor(1))).is_err());
    }
}
