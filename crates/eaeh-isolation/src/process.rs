//! Restricted host-process provider.
//!
//! Enforces: cleared environment (explicit allowlist only), working directory set to the
//! workspace, `RLIMIT_CPU`, `RLIMIT_FSIZE`, `RLIMIT_CORE=0`, a new process group killed
//! with `SIGKILL` on wall-clock timeout, bounded output capture.
//! Does **not** enforce filesystem confinement, network isolation, memory or process-count
//! limits, or privilege drop — and declares so. Operations that need those properties
//! are refused by provider selection.

use crate::output::read_bounded;
use crate::{ExecutionOutcome, ExecutionProvider, ExecutionSpec, IsolationCapabilities, check_spec};
use async_trait::async_trait;
use eaeh_core::{HarnessError, Result};
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;

pub const DEFAULT_PATH: &str = "/usr/bin:/bin";

#[derive(Debug, Default, Clone)]
pub struct RestrictedProcessProvider;

impl RestrictedProcessProvider {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn apply_limits(cmd: &mut Command, cpu_seconds: u64, max_file_bytes: u64) {
    // SAFETY: `pre_exec` runs in the forked child before exec. We only call
    // async-signal-safe libc functions (setrlimit) with stack-allocated arguments and
    // perform no allocation.
    unsafe {
        cmd.pre_exec(move || {
            let set = |res, v: u64| {
                let lim = libc::rlimit {
                    rlim_cur: v as libc::rlim_t,
                    rlim_max: v as libc::rlim_t,
                };
                if libc::setrlimit(res, &lim) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            };
            set(libc::RLIMIT_CPU, cpu_seconds.max(1))?;
            set(libc::RLIMIT_FSIZE, max_file_bytes)?;
            set(libc::RLIMIT_CORE, 0)?;
            Ok(())
        });
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn kill_group(pgid: u32) {
    if let Ok(pgid) = i32::try_from(pgid) {
        // SAFETY: plain syscall wrapper; pgid is the child's own process group (setpgid 0).
        unsafe {
            libc::killpg(pgid, libc::SIGKILL);
        }
    }
}

#[async_trait]
impl ExecutionProvider for RestrictedProcessProvider {
    fn id(&self) -> &str {
        "restricted-process"
    }

    fn capabilities(&self) -> IsolationCapabilities {
        IsolationCapabilities {
            timeout: true,
            env_isolation: true,
            cpu_limit: true,
            ..Default::default()
        }
    }

    async fn execute(&self, spec: ExecutionSpec) -> Result<ExecutionOutcome> {
        check_spec(self, &spec)?;
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .env_clear()
            .env("PATH", DEFAULT_PATH)
            .env("HOME", &spec.workspace)
            .envs(&spec.env)
            .current_dir(&spec.workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        #[cfg(unix)]
        apply_limits(&mut cmd, spec.limits.cpu_seconds, spec.limits.max_file_bytes);

        let start = Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| HarnessError::isolation(format!("failed to start '{}': {e}", spec.program)))?;
        let pid = child
            .id()
            .ok_or_else(|| HarnessError::isolation("child has no pid"))?;
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
                kill_group(pid);
                let s = child.wait().await.ok();
                (s, true)
            }
        };
        // Grandchildren in the same group may still hold the pipes; ensure they are gone.
        kill_group(pid);
        let (stdout, stdout_truncated) = out_task.await.unwrap_or_default();
        let (stderr, stderr_truncated) = err_task.await.unwrap_or_default();

        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt;
            status.and_then(|s| s.signal())
        };
        #[cfg(not(unix))]
        let signal = None;

        Ok(ExecutionOutcome {
            provider: self.id().into(),
            exit_code: status.and_then(|s| s.code()),
            signal,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            timed_out,
            duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}
