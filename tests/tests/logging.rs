//! Structured logging: secret redaction and correlation ids (AC-25, AC-27).
//!
//! This file is its own test binary (own process) with a single test that installs a
//! **global** subscriber. Thread-scoped subscribers (`set_default`) are unreliable when
//! other tests in the same process race on tracing's callsite interest cache.

use eaeh_agents::AgentTask;
use eaeh_core::secret::SecretsProvider;
use eaeh_core::{
    AgentDefinitionId, HarnessError, PrincipalId, Redactor, Result, Scope, SecretString, TaskId, TraceContext,
};
use eaeh_runtime::observability::RedactingMakeWriter;
use eaeh_runtime::{Harness, HarnessConfig};
use eaeh_testkit::{Script, ScriptedModelProvider};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const SECRET: &str = "ghp_SuperSecretRepoCredential42";

struct FixedSecrets;
impl SecretsProvider for FixedSecrets {
    fn name(&self) -> &str {
        "fixed-test"
    }
    fn get(&self, key: &str) -> Result<SecretString> {
        match key {
            "repo.token" => Ok(SecretString::new(SECRET)),
            _ => Err(HarnessError::not_found("no secret")),
        }
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn logs_are_redacted_and_carry_correlation_ids() {
    let redactor = Redactor::new();
    let capture = Capture::default();
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(RedactingMakeWriter::new(redactor.clone(), capture.clone()))
        .init();

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut cfg = HarnessConfig::load(&root.join("config/eaeh.toml")).unwrap();
    // Uses a scripted model provider: direct_provider mode (backward-compatible path).
    cfg.execution.mode = eaeh_agents::ExecutionMode::DirectProvider;
    cfg.storage.data_dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("logging-{}", uuid::Uuid::now_v7()));
    cfg.isolation.container_enabled = false;
    let provider = Arc::new(ScriptedModelProvider::new(
        "scripted",
        "scripted-model",
        vec![
            Script::tool("fs.read", json!({"path": "config.txt"})),
            Script::json(json!({"summary": "done"})),
        ],
    ));
    let h = Harness::builder(cfg)
        .redactor(redactor)
        .model_provider(provider)
        .secrets(Arc::new(FixedSecrets))
        .build()
        .await
        .unwrap();

    let secret = h.secret("repo.token").unwrap();
    tracing::info!("using credential {}", secret.expose());

    let (_admin, project, _t) = h
        .identity
        .bootstrap(
            "acme",
            "platform",
            PrincipalId::new("admin").unwrap(),
            "Admin",
            30,
        )
        .await
        .unwrap();
    let scope = Scope::new(_admin.tenant, project);
    let ws = h.workspaces.create(scope, &_admin.id).await.unwrap();
    ws.acquire_lock("config.txt", "setup", chrono::Duration::minutes(1))
        .await
        .unwrap();
    ws.write_file(
        "setup",
        "config.txt",
        format!("token={SECRET}").as_bytes(),
        TraceContext::new_root(),
    )
    .await
    .unwrap();
    h.budgets
        .init("wf-l", &eaeh_governance::BudgetLimits::platform_default())
        .await
        .unwrap();
    let analyst = h
        .agent_registry
        .latest(&AgentDefinitionId::new("analyst").unwrap())
        .await
        .unwrap();
    let run_trace = TraceContext::new_root();
    h.agents
        .run(
            &analyst,
            AgentTask {
                task_id: TaskId::new(),
                kind: "analyze".into(),
                input: json!({}),
                scope,
                trace: run_trace,
                workflow_owner: Some("wf-l".into()),
                workspace: Some(ws),
            },
        )
        .await
        .unwrap();

    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("using credential"), "log line emitted");
    assert!(!logs.contains(SECRET), "secret leaked into logs");
    let corr = run_trace.correlation.to_string();
    let correlated: Vec<&str> = logs.lines().filter(|l| l.contains(&corr)).collect();
    assert!(
        correlated.len() >= 2,
        "expected log lines carrying correlation id {corr}, found {}",
        correlated.len()
    );
    assert!(
        correlated.iter().any(|l| l.contains("\"agent\"")),
        "agent span present"
    );
    assert!(
        correlated.iter().any(|l| l.contains("tool_invocation")),
        "tool_invocation span present"
    );
}
