//! EAEH isolated execution (master §3.3, D-07, clarifications 2 & 3).
//!
//! Every [`ExecutionProvider`] declares the isolation properties it *genuinely enforces*
//! ([`IsolationCapabilities`]). Callers state what they need ([`IsolationRequirements`]);
//! [`ProviderRegistry::select`] refuses — it never silently downgrades — when no provider
//! satisfies the requirements, and each provider re-checks the requirements itself.
//!
//! | Capability | `RestrictedProcessProvider` | `ContainerProvider` |
//! |---|---|---|
//! | wall-clock timeout (process-group kill) | enforced | enforced |
//! | environment isolation | enforced | enforced |
//! | CPU limit | enforced (`RLIMIT_CPU` seconds) | enforced (`--cpus`) |
//! | filesystem confinement | **not enforced** | enforced (read-only rootfs, workspace-only mount) |
//! | network isolation | **not enforced** | enforced (`--network none`) |
//! | memory limit | **not enforced** | enforced (cgroup `--memory`) |
//! | process-count limit | **not enforced** | enforced (`--pids-limit`) |
//! | privilege drop | **not enforced** | enforced (non-root uid, `--cap-drop ALL`, `no-new-privileges`) |
//!
//! Claims for the container provider are verified by escape-attempt tests in
//! `tests/container_isolation.rs` against a real runtime.

mod container;
mod output;
mod process;

pub use container::{ContainerConfig, ContainerProvider};
pub use process::RestrictedProcessProvider;

use async_trait::async_trait;
use eaeh_core::{HarnessError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Isolation properties a provider enforces (true = enforced).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolationCapabilities {
    pub timeout: bool,
    pub env_isolation: bool,
    pub cpu_limit: bool,
    pub filesystem_confinement: bool,
    pub network_isolation: bool,
    pub memory_limit: bool,
    pub pid_limit: bool,
    pub privilege_drop: bool,
}

/// Isolation properties an operation requires.
pub type IsolationRequirements = IsolationCapabilities;

impl IsolationCapabilities {
    /// Strongest requirement set: every capability.
    pub fn all() -> Self {
        Self {
            timeout: true,
            env_isolation: true,
            cpu_limit: true,
            filesystem_confinement: true,
            network_isolation: true,
            memory_limit: true,
            pid_limit: true,
            privilege_drop: true,
        }
    }

    /// Names of requirements in `req` not provided by `self`.
    pub fn missing(&self, req: &IsolationRequirements) -> Vec<&'static str> {
        let pairs = [
            (req.timeout, self.timeout, "timeout"),
            (req.env_isolation, self.env_isolation, "env_isolation"),
            (req.cpu_limit, self.cpu_limit, "cpu_limit"),
            (
                req.filesystem_confinement,
                self.filesystem_confinement,
                "filesystem_confinement",
            ),
            (req.network_isolation, self.network_isolation, "network_isolation"),
            (req.memory_limit, self.memory_limit, "memory_limit"),
            (req.pid_limit, self.pid_limit, "pid_limit"),
            (req.privilege_drop, self.privilege_drop, "privilege_drop"),
        ];
        pairs
            .iter()
            .filter(|(need, has, _)| *need && !*has)
            .map(|(_, _, n)| *n)
            .collect()
    }

    pub fn satisfies(&self, req: &IsolationRequirements) -> bool {
        self.missing(req).is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub timeout: Duration,
    pub cpu_seconds: u64,
    pub cpu_cores: f64,
    pub memory_bytes: u64,
    pub max_pids: u64,
    pub max_output_bytes: usize,
    pub max_file_bytes: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            cpu_seconds: 60,
            cpu_cores: 1.0,
            memory_bytes: 256 * 1024 * 1024,
            max_pids: 64,
            max_output_bytes: 1024 * 1024,
            max_file_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionSpec {
    pub program: String,
    pub args: Vec<String>,
    /// The only environment variables visible to the process.
    pub env: BTreeMap<String, String>,
    /// Host directory exposed to the process (cwd / bind mount). Must exist.
    pub workspace: PathBuf,
    pub limits: ResourceLimits,
    pub requirements: IsolationRequirements,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionOutcome {
    pub provider: String,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
    pub duration_ms: u64,
}

impl ExecutionOutcome {
    pub fn success(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }
}

#[async_trait]
pub trait ExecutionProvider: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &str;
    fn capabilities(&self) -> IsolationCapabilities;
    async fn execute(&self, spec: ExecutionSpec) -> Result<ExecutionOutcome>;
}

pub(crate) fn check_spec(provider: &dyn ExecutionProvider, spec: &ExecutionSpec) -> Result<()> {
    let missing = provider.capabilities().missing(&spec.requirements);
    if !missing.is_empty() {
        return Err(HarnessError::isolation(format!(
            "provider '{}' cannot enforce required isolation: {}",
            provider.id(),
            missing.join(", ")
        )));
    }
    if spec.program.is_empty() || spec.program.contains('\0') {
        return Err(HarnessError::invalid_input("invalid program"));
    }
    if !spec.workspace.is_dir() {
        return Err(HarnessError::invalid_input(format!(
            "workspace {} is not a directory",
            spec.workspace.display()
        )));
    }
    if spec.limits.timeout.is_zero() || spec.limits.max_output_bytes == 0 {
        return Err(HarnessError::invalid_input("limits must be non-zero"));
    }
    for k in spec.env.keys() {
        if k.is_empty() || k.contains('=') || k.contains('\0') {
            return Err(HarnessError::invalid_input(format!("invalid env var name '{k}'")));
        }
    }
    Ok(())
}

/// Ordered set of available providers; selection never downgrades isolation.
#[derive(Debug, Default, Clone)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn ExecutionProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, p: Arc<dyn ExecutionProvider>) {
        self.providers.push(p);
    }

    pub fn list(&self) -> Vec<(String, IsolationCapabilities)> {
        self.providers
            .iter()
            .map(|p| (p.id().to_string(), p.capabilities()))
            .collect()
    }

    /// First registered provider that satisfies every requirement.
    pub fn select(&self, req: &IsolationRequirements) -> Result<Arc<dyn ExecutionProvider>> {
        self.providers
            .iter()
            .find(|p| p.capabilities().satisfies(req))
            .cloned()
            .ok_or_else(|| {
                let available: Vec<String> = self
                    .providers
                    .iter()
                    .map(|p| {
                        format!(
                            "{} (missing: {})",
                            p.id(),
                            p.capabilities().missing(req).join(", ")
                        )
                    })
                    .collect();
                HarnessError::isolation(format!(
                    "no execution provider enforces the required isolation; available: [{}]",
                    available.join("; ")
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_refuses_instead_of_downgrading() {
        let mut reg = ProviderRegistry::new();
        reg.add(Arc::new(RestrictedProcessProvider::new()));
        let weak = IsolationRequirements {
            timeout: true,
            env_isolation: true,
            ..Default::default()
        };
        assert_eq!(reg.select(&weak).unwrap().id(), "restricted-process");
        let strong = IsolationCapabilities::all();
        let e = reg.select(&strong).unwrap_err();
        assert_eq!(e.kind, eaeh_core::ErrorKind::Isolation);
        assert!(e.message.contains("network_isolation"));
    }

    #[test]
    fn process_provider_declares_honest_capabilities() {
        let c = RestrictedProcessProvider::new().capabilities();
        assert!(c.timeout && c.env_isolation && c.cpu_limit);
        assert!(
            !c.filesystem_confinement
                && !c.network_isolation
                && !c.memory_limit
                && !c.pid_limit
                && !c.privilege_drop
        );
    }
}
