//! Harness configuration (TOML). Relative paths are resolved against the directory of
//! the configuration file. No credentials are ever read from this file; secrets come
//! from the `SecretsProvider` (environment `EAEH_SECRET_*` in Phase 1).

use eaeh_core::hash::from_hex;
use eaeh_core::{HarnessError, Result};
use eaeh_extensibility::{PluginCapability, TrustPolicy};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HarnessConfig {
    pub storage: StorageConfig,
    pub definitions: DefinitionsConfig,
    pub isolation: IsolationConfig,
    pub plugins: PluginsConfig,
    pub api: ApiConfig,
    pub supervision: SupervisionConfig,
    pub models: ModelsConfig,
    pub log: LogConfig,
    /// Phase 2: knowledge management and retrieval settings.
    pub knowledge: eaeh_knowledge::KnowledgeConfig,
    /// Phase 2: production model providers (disabled unless explicitly enabled).
    pub providers: ProvidersConfig,
    /// Phase 3: requirements-analysis heuristics (lexicon, NFR checklist, thresholds).
    pub requirements: eaeh_requirements::AnalysisConfig,
    /// How agent tasks obtain AI reasoning (ADR-0025).
    pub execution: ExecutionConfig,
    #[serde(skip)]
    pub base_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DefinitionsConfig {
    pub prompts_dir: PathBuf,
    pub agents_dir: PathBuf,
    pub workflows_dir: PathBuf,
    pub policies_dir: PathBuf,
    /// Repository-approved skill definitions that `eaeh skill sync` / `eaeh project init`
    /// may register (registration still requires human approval before use).
    pub skills_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IsolationConfig {
    pub process_enabled: bool,
    pub container_enabled: bool,
    pub container_runtime: String,
    pub container_image: String,
    pub container_user: String,
    /// Programs `exec.run` may start (paths inside the execution environment).
    pub exec_allowed_programs: Vec<String>,
    pub exec_timeout_secs: u64,
    pub exec_memory_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginsConfig {
    pub require_signature: bool,
    /// key id → hex-encoded ed25519 public key.
    pub trusted_keys: BTreeMap<String, String>,
    pub grantable: BTreeSet<PluginCapability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApiConfig {
    pub bind: String,
    pub max_body_bytes: usize,
    pub requests_per_minute: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SupervisionConfig {
    pub interval_secs: u64,
    /// A running task without a heartbeat for this long is escalated. Must exceed the
    /// longest state timeout (tasks heartbeat on status changes only in Phase 1).
    pub stall_after_secs: u64,
    pub max_failed_tasks_per_instance: u32,
    pub dispatch_interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelsConfig {
    pub timeout_secs: u64,
    pub context_max_tokens: u64,
    pub circuit_failure_threshold: u32,
    pub circuit_cooldown_secs: u64,
    pub max_delegation_depth: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    /// `json` or `pretty`.
    pub format: String,
    pub filter: String,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
            definitions: DefinitionsConfig::default(),
            isolation: IsolationConfig::default(),
            plugins: PluginsConfig::default(),
            api: ApiConfig::default(),
            supervision: SupervisionConfig::default(),
            models: ModelsConfig::default(),
            log: LogConfig::default(),
            knowledge: eaeh_knowledge::KnowledgeConfig::default(),
            providers: ProvidersConfig::default(),
            requirements: eaeh_requirements::AnalysisConfig::default(),
            execution: ExecutionConfig::default(),
            base_dir: PathBuf::from("."),
        }
    }
}

/// `[execution]`: who performs AI reasoning.
///
/// * `external_ai`: the connected AI client (Claude Code, GitHub Copilot) reasons on the
///   user's subscription; EAEH registers **no** model provider, even if one is enabled below.
/// * `direct_provider`: EAEH calls the enabled providers itself (separate API key and billing).
///   This is the default when the section is absent, for backward compatibility.
/// * `deterministic`: only the replay provider is registered (fixtures, not AI).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionConfig {
    pub mode: eaeh_agents::ExecutionMode,
    /// The External Execution Coordinator (ADR-0026): runs hand-offs automatically through
    /// an installed, externally authenticated AI client. Used only in `external_ai` mode.
    pub coordinator: CoordinatorConfig,
}

/// `[execution.coordinator]`. Every limit is bounded; nothing retries forever.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoordinatorConfig {
    /// Start a background worker automatically when a run waits for the AI client
    /// (`eaeh workflow run`, `eaeh approval decide`). `false`: run `eaeh worker start` yourself.
    pub auto_start: bool,
    /// External execution provider. Implemented: `claude_code`. (`github_copilot` is
    /// recognised but not supported: its headless authentication is not verified.)
    pub provider: String,
    /// Path of the Claude Code CLI; default: `claude` found on `PATH`.
    pub claude_path: Option<String>,
    /// Model passed to the client (`--model`); default: the client's own setting.
    pub model: Option<String>,
    /// Wall-clock limit of one execution attempt; the client process is then stopped.
    pub task_timeout_secs: u64,
    /// Attempts per hand-off (first run + retries) before it is marked `exhausted`.
    pub max_attempts: u32,
    /// First retry delay; doubles per attempt.
    pub backoff_secs: u64,
    /// How often an idle worker looks for work.
    pub poll_interval_ms: u64,
    /// Lease / heartbeat window. A worker silent for longer is presumed dead and its task
    /// is recovered.
    pub lease_secs: u64,
    /// Desktop notification when an approval is needed or a task stops (macOS
    /// `osascript`, Linux `notify-send`; silently skipped when unavailable).
    pub notify: bool,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            auto_start: true,
            provider: "claude_code".into(),
            claude_path: None,
            model: None,
            task_timeout_secs: 1800,
            max_attempts: 3,
            backoff_secs: 30,
            poll_interval_ms: 3000,
            lease_secs: 120,
            notify: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProvidersConfig {
    /// Anthropic Messages API. The API key is read only from the secrets provider
    /// (`EAEH_SECRET_ANTHROPIC_API_KEY`), never from this file.
    pub anthropic: eaeh_provider_anthropic::AnthropicConfig,
    /// Deterministic replay of recorded responses (ADR-0023). Not AI; disabled by default.
    pub replay: ReplayConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReplayConfig {
    pub enabled: bool,
    /// Cassette file (relative to this configuration file).
    pub cassette: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from(".eaeh-data"),
        }
    }
}

impl Default for DefinitionsConfig {
    fn default() -> Self {
        Self {
            prompts_dir: PathBuf::from("prompts"),
            agents_dir: PathBuf::from("config/agents"),
            workflows_dir: PathBuf::from("workflows"),
            policies_dir: PathBuf::from("config/policies"),
            skills_dir: PathBuf::from("config/skills"),
        }
    }
}

impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            process_enabled: true,
            container_enabled: false,
            container_runtime: "podman".into(),
            container_image: "docker.io/library/alpine:3.20".into(),
            container_user: "65534:65534".into(),
            exec_allowed_programs: vec![],
            exec_timeout_secs: 120,
            exec_memory_mb: 256,
        }
    }
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            require_signature: true,
            trusted_keys: BTreeMap::new(),
            grantable: BTreeSet::new(),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".into(),
            max_body_bytes: 1024 * 1024,
            requests_per_minute: 600,
        }
    }
}

impl Default for SupervisionConfig {
    fn default() -> Self {
        Self {
            interval_secs: 30,
            stall_after_secs: 7200,
            max_failed_tasks_per_instance: 10,
            dispatch_interval_ms: 1000,
        }
    }
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 120,
            context_max_tokens: 100_000,
            circuit_failure_threshold: 3,
            circuit_cooldown_secs: 30,
            max_delegation_depth: 3,
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            format: "json".into(),
            filter: "info".into(),
        }
    }
}

impl HarnessConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| HarnessError::not_found(format!("config {}: {e}", path.display())))?;
        let mut cfg: HarnessConfig = toml::from_str(&text)
            .map_err(|e| HarnessError::invalid_input(format!("config {}: {e}", path.display())))?;
        cfg.base_dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        if !matches!(self.log.format.as_str(), "json" | "pretty") {
            return Err(HarnessError::invalid_input("log.format must be json or pretty"));
        }
        if self.api.max_body_bytes == 0 || self.api.requests_per_minute == 0 {
            return Err(HarnessError::invalid_input("api limits must be > 0"));
        }
        if self.supervision.interval_secs == 0 || self.supervision.dispatch_interval_ms == 0 {
            return Err(HarnessError::invalid_input("supervision intervals must be > 0"));
        }
        for p in &self.isolation.exec_allowed_programs {
            if !p.starts_with('/') {
                return Err(HarnessError::invalid_input(format!(
                    "exec_allowed_programs entries must be absolute paths: {p}"
                )));
            }
        }
        self.trust_policy().map(|_| ())
    }

    pub fn resolve(&self, p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base_dir.join(p)
        }
    }

    pub fn data_dir(&self) -> PathBuf {
        self.resolve(&self.storage.data_dir)
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir().join("eaeh.db")
    }

    pub fn trust_policy(&self) -> Result<TrustPolicy> {
        let mut keys = BTreeMap::new();
        for (id, hex) in &self.plugins.trusted_keys {
            let bytes = from_hex(hex)?;
            let arr: [u8; 32] = bytes
                .try_into()
                .map_err(|_| HarnessError::invalid_input(format!("trusted key '{id}' must be 32 bytes")))?;
            keys.insert(id.clone(), arr);
        }
        Ok(TrustPolicy {
            require_signature: self.plugins.require_signature,
            trusted_keys: keys,
            grantable: self.plugins.grantable.clone(),
        })
    }
}
