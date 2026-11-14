//! Structured logging with redaction, and basic metrics derived from domain events.

use async_trait::async_trait;
use eaeh_core::{Redactor, Result};
use eaeh_events::{EventEnvelope, EventSubscriber};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

/// Writer that redacts secrets from every formatted log line before it reaches stderr
/// (or the provided sink).
#[derive(Clone)]
pub struct RedactingMakeWriter<W: Write + Send + 'static> {
    redactor: Redactor,
    sink: Arc<Mutex<W>>,
}

impl<W: Write + Send + 'static> RedactingMakeWriter<W> {
    pub fn new(redactor: Redactor, sink: W) -> Self {
        Self {
            redactor,
            sink: Arc::new(Mutex::new(sink)),
        }
    }
}

pub struct RedactingWriter<W: Write + Send + 'static> {
    redactor: Redactor,
    sink: Arc<Mutex<W>>,
    buf: Vec<u8>,
}

impl<W: Write + Send + 'static> Write for RedactingWriter<W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.buf).into_owned();
        self.buf.clear();
        let clean = self.redactor.redact_str(&text);
        let mut s = self
            .sink
            .lock()
            .map_err(|_| std::io::Error::other("log sink poisoned"))?;
        s.write_all(clean.as_bytes())?;
        s.flush()
    }
}

impl<W: Write + Send + 'static> Drop for RedactingWriter<W> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl<'a, W: Write + Send + 'static> MakeWriter<'a> for RedactingMakeWriter<W> {
    type Writer = RedactingWriter<W>;
    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            redactor: self.redactor.clone(),
            sink: self.sink.clone(),
            buf: Vec::new(),
        }
    }
}

/// Install the global tracing subscriber (once per process).
pub fn init_tracing(format: &str, filter: &str, redactor: Redactor) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter));
    let writer = RedactingMakeWriter::new(redactor, std::io::stderr());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_target(true);
    let _ = if format == "json" {
        builder.json().flatten_event(true).try_init()
    } else {
        builder.try_init()
    };
}

/// Counters derived from the event stream (Prometheus text exposition).
#[derive(Debug, Default, Clone)]
pub struct Metrics {
    counters: Arc<Mutex<BTreeMap<String, u64>>>,
}

impl Metrics {
    pub fn incr(&self, name: &str, by: u64) {
        if let Ok(mut c) = self.counters.lock() {
            *c.entry(name.to_string()).or_default() += by;
        }
    }

    pub fn snapshot(&self) -> BTreeMap<String, u64> {
        self.counters.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn render_prometheus(&self, gauges: &[(&str, i64)]) -> String {
        let mut out = String::new();
        out.push_str("# TYPE eaeh_events_total counter\n");
        for (k, v) in self.snapshot() {
            out.push_str(&format!("{k} {v}\n"));
        }
        for (k, v) in gauges {
            out.push_str(&format!("# TYPE {k} gauge\n{k} {v}\n"));
        }
        out
    }
}

pub struct MetricsSubscriber(pub Metrics);

#[async_trait]
impl EventSubscriber for MetricsSubscriber {
    fn name(&self) -> &str {
        "metrics"
    }
    fn accepts(&self, _event_type: &str) -> bool {
        true
    }
    async fn handle(&self, e: &EventEnvelope) -> Result<()> {
        self.0
            .incr(&format!("eaeh_events_total{{type=\"{}\"}}", e.event_type), 1);
        match e.event_type.as_str() {
            "tool.invoked" => {
                let ok = e
                    .payload
                    .get("succeeded")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.0
                    .incr(&format!("eaeh_tool_invocations_total{{succeeded=\"{ok}\"}}"), 1);
            }
            "model.invoked" => {
                let tokens = e
                    .payload
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                    + e.payload
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                self.0.incr("eaeh_model_tokens_total", tokens);
            }
            "workflow.completed" => {
                let s = e
                    .payload
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                self.0
                    .incr(&format!("eaeh_workflows_completed_total{{status=\"{s}\"}}"), 1);
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eaeh_core::SecretString;

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn log_lines_are_redacted() {
        let r = Redactor::new();
        r.register(&SecretString::new("db-password-12345"));
        let sink = Sink::default();
        let mw = RedactingMakeWriter::new(r, sink.clone());
        let subscriber = tracing_subscriber::fmt().with_writer(mw).json().finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                token = "eaeh_0123456789abcdef0123",
                "connecting with db-password-12345"
            );
        });
        let out = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(out.contains("connecting with"));
        assert!(!out.contains("db-password-12345"), "{out}");
        assert!(!out.contains("0123456789abcdef0123"), "{out}");
    }
}
