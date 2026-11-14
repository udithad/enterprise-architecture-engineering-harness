//! Generic, ordered middleware pipelines (master instructions §7).
//!
//! A [`Pipeline<Req, Res>`] wraps an [`Endpoint`]. Middleware runs in ascending
//! `order()` (ties by name) on the way in and in reverse on the way out. Any middleware
//! may short-circuit by returning without calling `next`; errors propagate unchanged to
//! the caller. A pipeline-wide timeout bounds total execution.

use async_trait::async_trait;
use eaeh_core::{HarnessError, Result};
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[async_trait]
pub trait Endpoint<Req: Send + 'static, Res: Send + 'static>: Send + Sync {
    async fn call(&self, req: Req) -> Result<Res>;
}

#[async_trait]
pub trait Middleware<Req: Send + 'static, Res: Send + 'static>: Send + Sync {
    fn name(&self) -> &str;
    fn order(&self) -> i32 {
        100
    }
    async fn handle(&self, req: Req, next: Next<'_, Req, Res>) -> Result<Res>;
}

pub struct Next<'a, Req: Send + 'static, Res: Send + 'static> {
    rest: &'a [Arc<dyn Middleware<Req, Res>>],
    endpoint: &'a dyn Endpoint<Req, Res>,
}

impl<'a, Req: Send + 'static, Res: Send + 'static> Next<'a, Req, Res> {
    pub async fn run(self, req: Req) -> Result<Res> {
        match self.rest.split_first() {
            Some((first, rest)) => {
                first
                    .handle(
                        req,
                        Next {
                            rest,
                            endpoint: self.endpoint,
                        },
                    )
                    .await
            }
            None => self.endpoint.call(req).await,
        }
    }
}

pub struct Pipeline<Req: Send + 'static, Res: Send + 'static> {
    name: String,
    middleware: RwLock<Vec<Arc<dyn Middleware<Req, Res>>>>,
    timeout: Duration,
}

impl<Req: Send + 'static, Res: Send + 'static> std::fmt::Debug for Pipeline<Req, Res> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("name", &self.name)
            .field("middleware", &self.names())
            .finish()
    }
}

impl<Req: Send + 'static, Res: Send + 'static> Pipeline<Req, Res> {
    pub fn new(name: &str, timeout: Duration) -> Self {
        Self {
            name: name.into(),
            middleware: RwLock::new(Vec::new()),
            timeout,
        }
    }

    pub fn add(&self, mw: Arc<dyn Middleware<Req, Res>>) -> Result<()> {
        let mut list = self
            .middleware
            .write()
            .map_err(|_| HarnessError::internal("pipeline lock poisoned"))?;
        if list.iter().any(|m| m.name() == mw.name()) {
            return Err(HarnessError::conflict(format!(
                "middleware '{}' already in pipeline '{}'",
                mw.name(),
                self.name
            )));
        }
        list.push(mw);
        list.sort_by(|a, b| a.order().cmp(&b.order()).then_with(|| a.name().cmp(b.name())));
        Ok(())
    }

    pub fn remove(&self, name: &str) -> bool {
        self.middleware
            .write()
            .map(|mut l| {
                let before = l.len();
                l.retain(|m| m.name() != name);
                before != l.len()
            })
            .unwrap_or(false)
    }

    pub fn names(&self) -> Vec<String> {
        self.middleware
            .read()
            .map(|l| l.iter().map(|m| m.name().to_string()).collect())
            .unwrap_or_default()
    }

    pub async fn execute(&self, req: Req, endpoint: &dyn Endpoint<Req, Res>) -> Result<Res> {
        let snapshot: Vec<Arc<dyn Middleware<Req, Res>>> =
            self.middleware.read().map(|l| l.clone()).unwrap_or_default();
        let next = Next {
            rest: &snapshot,
            endpoint,
        };
        match tokio::time::timeout(self.timeout, next.run(req)).await {
            Ok(r) => r,
            Err(_) => Err(HarnessError::timeout(format!(
                "pipeline '{}' exceeded {:?}",
                self.name, self.timeout
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Tag(&'static str, i32, Arc<Mutex<Vec<String>>>);

    #[async_trait]
    impl Middleware<String, String> for Tag {
        fn name(&self) -> &str {
            self.0
        }
        fn order(&self) -> i32 {
            self.1
        }
        async fn handle(&self, req: String, next: Next<'_, String, String>) -> Result<String> {
            self.2.lock().unwrap().push(format!("in:{}", self.0));
            let res = next.run(format!("{req}>{}", self.0)).await?;
            self.2.lock().unwrap().push(format!("out:{}", self.0));
            Ok(format!("{res}<{}", self.0))
        }
    }

    struct ShortCircuit;
    #[async_trait]
    impl Middleware<String, String> for ShortCircuit {
        fn name(&self) -> &str {
            "deny"
        }
        fn order(&self) -> i32 {
            0
        }
        async fn handle(&self, _req: String, _next: Next<'_, String, String>) -> Result<String> {
            Err(HarnessError::forbidden("stopped"))
        }
    }

    struct Echo;
    #[async_trait]
    impl Endpoint<String, String> for Echo {
        async fn call(&self, req: String) -> Result<String> {
            Ok(format!("[{req}]"))
        }
    }

    struct Slow;
    #[async_trait]
    impl Endpoint<String, String> for Slow {
        async fn call(&self, _req: String) -> Result<String> {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(String::new())
        }
    }

    #[tokio::test]
    async fn deterministic_onion_order() {
        let log = Arc::new(Mutex::new(vec![]));
        let p = Pipeline::new("t", Duration::from_secs(1));
        p.add(Arc::new(Tag("b", 20, log.clone()))).unwrap();
        p.add(Arc::new(Tag("a", 10, log.clone()))).unwrap();
        let out = p.execute("x".into(), &Echo).await.unwrap();
        assert_eq!(out, "[x>a>b]<b<a");
        assert_eq!(*log.lock().unwrap(), vec!["in:a", "in:b", "out:b", "out:a"]);
        assert!(p.add(Arc::new(Tag("a", 1, log))).is_err());
    }

    #[tokio::test]
    async fn short_circuit_and_errors_propagate() {
        let log = Arc::new(Mutex::new(vec![]));
        let p = Pipeline::new("t", Duration::from_secs(1));
        p.add(Arc::new(Tag("a", 10, log.clone()))).unwrap();
        p.add(Arc::new(ShortCircuit)).unwrap();
        let e = p.execute("x".into(), &Echo).await.unwrap_err();
        assert_eq!(e.kind, eaeh_core::ErrorKind::Forbidden);
        assert!(log.lock().unwrap().is_empty(), "later middleware never ran");
    }

    #[tokio::test]
    async fn pipeline_timeout() {
        let p: Pipeline<String, String> = Pipeline::new("t", Duration::from_millis(20));
        assert_eq!(
            p.execute("x".into(), &Slow).await.unwrap_err().kind,
            eaeh_core::ErrorKind::Timeout
        );
    }
}
