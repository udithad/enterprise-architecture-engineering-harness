//! OCI container provider driving the Docker or Podman CLI.
//!
//! Each execution runs in a fresh, auto-removed container with:
//! `--network none`, `--read-only` root filesystem, a size-limited `noexec` `/tmp`,
//! only the workspace bind-mounted (read-write) at `/workspace`, `--memory` =
//! `--memory-swap` (no swap), `--cpus`, `--pids-limit`, `--cap-drop ALL`,
//! `--security-opt no-new-privileges`, a non-root numeric user, `--pull never`
//! (images must be pre-provisioned; no network at execution time) and explicit `-e`
//! variables only. On timeout the container is killed by name.

use crate::output::read_bounded;
use crate::{ExecutionOutcome, ExecutionProvider, ExecutionSpec, IsolationCapabilities, check_spec};
use async_trait::async_trait;
use eaeh_core::{HarnessError, Result};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerConfig {
    /// `podman` or `docker` (or an absolute path to either).
    pub runtime: String,
    /// Image reference; pin by digest in production configurations.
    pub image: String,
    /// Numeric `uid:gid`; must not be root.
    pub user: String,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            runtime: "podman".into(),
            image: "docker.io/library/alpine:3.20".into(),
            user: "65534:65534".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerProvider {
    cfg: ContainerConfig,
}

impl ContainerProvider {
    pub fn new(cfg: ContainerConfig) -> Result<Self> {
        let valid_user = cfg
            .user
            .split_once(':')
            .map(|(u, g)| u.parse::<u32>().is_ok_and(|u| u != 0) && g.parse::<u32>().is_ok_and(|g| g != 0))
            .unwrap_or(false);
        if !valid_user {
            return Err(HarnessError::invalid_input(
                "container user must be numeric non-root uid:gid",
            ));
        }
        if cfg.image.is_empty() || cfg.image.starts_with('-') || cfg.runtime.is_empty() {
            return Err(HarnessError::invalid_input("invalid container runtime or image"));
        }
        Ok(Self { cfg })
    }

    pub fn config(&self) -> &ContainerConfig {
        &self.cfg
    }

    /// Check that the runtime responds and the image is present locally.
    pub async fn probe(&self) -> Result<()> {
        let run = |args: Vec<&str>| {
            let mut c = Command::new(&self.cfg.runtime);
            c.args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            c
        };
        let ok = |s: std::io::Result<std::process::ExitStatus>| s.map(|s| s.success()).unwrap_or(false);
        let version = tokio::time::timeout(Duration::from_secs(20), run(vec!["version"]).status()).await;
        if !version.map(ok).unwrap_or(false) {
            return Err(HarnessError::unavailable(format!(
                "container runtime '{}' not available",
                self.cfg.runtime
            )));
        }
        let image = tokio::time::timeout(
            Duration::from_secs(20),
            run(vec!["image", "inspect", &self.cfg.image]).status(),
        )
        .await;
        if !image.map(ok).unwrap_or(false) {
            return Err(HarnessError::unavailable(format!(
                "container image '{}' is not present locally (pull it explicitly; executions use --pull never)",
                self.cfg.image
            )));
        }
        Ok(())
    }

    /// Build the full argument vector (exposed for inspection in tests and audit).
    pub fn build_args(&self, spec: &ExecutionSpec, name: &str) -> Result<Vec<String>> {
        let ws = spec
            .workspace
            .canonicalize()
            .map_err(|e| HarnessError::invalid_input(format!("workspace: {e}")))?;
        let ws = ws
            .to_str()
            .ok_or_else(|| HarnessError::invalid_input("workspace path not UTF-8"))?;
        if ws.contains(':') || ws.contains(',') {
            return Err(HarnessError::invalid_input(
                "workspace path may not contain ':' or ','",
            ));
        }
        let l = &spec.limits;
        let mut a: Vec<String> = vec![
            "run".into(),
            "--rm".into(),
            "--name".into(),
            name.into(),
            "--pull".into(),
            "never".into(),
            "--network".into(),
            "none".into(),
            "--read-only".into(),
            "--tmpfs".into(),
            "/tmp:rw,noexec,nosuid,nodev,size=16m".into(),
            "--memory".into(),
            format!("{}b", l.memory_bytes.max(8 * 1024 * 1024)),
            "--memory-swap".into(),
            format!("{}b", l.memory_bytes.max(8 * 1024 * 1024)),
            "--cpus".into(),
            format!("{:.2}", l.cpu_cores.clamp(0.1, 64.0)),
            "--pids-limit".into(),
            l.max_pids.max(1).to_string(),
            "--ulimit".into(),
            format!("fsize={}", l.max_file_bytes),
            "--ulimit".into(),
            "nofile=256:256".into(),
            "--cap-drop".into(),
            "ALL".into(),
            "--security-opt".into(),
            "no-new-privileges".into(),
            "--user".into(),
            self.cfg.user.clone(),
            "--workdir".into(),
            "/workspace".into(),
            "--volume".into(),
            format!("{ws}:/workspace:rw"),
            "--env".into(),
            "HOME=/workspace".into(),
        ];
        if self.is_podman() {
            // Podman injects host proxy variables by default; disable to keep the
            // environment limited to the explicit allowlist. (Docker injects proxies
            // configured in ~/.docker/config.json and has no per-run switch — see
            // docs/architecture/isolation.md, known limitation ISO-L2.)
            a.push("--http-proxy=false".into());
        }
        for (k, v) in &spec.env {
            a.push("--env".into());
            a.push(format!("{k}={v}"));
        }
        a.push(self.cfg.image.clone());
        a.push(spec.program.clone());
        a.extend(spec.args.iter().cloned());
        Ok(a)
    }

    fn is_podman(&self) -> bool {
        std::path::Path::new(&self.cfg.runtime)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains("podman"))
    }

    async fn force_remove(&self, name: &str) {
        for args in [vec!["kill", name], vec!["rm", "-f", name]] {
            let _ = tokio::time::timeout(
                Duration::from_secs(20),
                Command::new(&self.cfg.runtime)
                    .args(&args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status(),
            )
            .await;
        }
    }
}

#[async_trait]
impl ExecutionProvider for ContainerProvider {
    fn id(&self) -> &str {
        "container"
    }

    fn capabilities(&self) -> IsolationCapabilities {
        IsolationCapabilities::all()
    }

    async fn execute(&self, spec: ExecutionSpec) -> Result<ExecutionOutcome> {
        check_spec(self, &spec)?;
        let name = format!("eaeh-{}", uuid::Uuid::now_v7());
        let args = self.build_args(&spec, &name)?;
        let start = Instant::now();
        let mut child = Command::new(&self.cfg.runtime)
            .args(&args)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| HarnessError::isolation(format!("failed to start container runtime: {e}")))?;
        let out = child
            .stdout
            .take()
            .ok_or_else(|| HarnessError::internal("stdout not piped"))?;
        let err = child
            .stderr
            .take()
            .ok_or_else(|| HarnessError::internal("stderr not piped"))?;
        let max = spec.limits.max_output_bytes;
        let out_task = tokio::spawn(read_bounded(out, max));
        let err_task = tokio::spawn(read_bounded(err, max));

        let (status, timed_out) = match tokio::time::timeout(spec.limits.timeout, child.wait()).await {
            Ok(s) => (Some(s?), false),
            Err(_) => {
                self.force_remove(&name).await;
                let _ = child.kill().await;
                (child.wait().await.ok(), true)
            }
        };
        let (stdout, stdout_truncated) = out_task.await.unwrap_or_default();
        let (stderr, stderr_truncated) = err_task.await.unwrap_or_default();
        let exit_code = status.and_then(|s| s.code());
        if exit_code == Some(125) && !timed_out {
            // Runtime-level failure (image missing, invalid flag): not the workload's exit.
            return Err(HarnessError::isolation(format!(
                "container runtime error: {}",
                stderr.trim()
            )));
        }
        Ok(ExecutionOutcome {
            provider: self.id().into(),
            exit_code,
            signal: None,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            timed_out,
            duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}
