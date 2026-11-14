//! Claude Code CLI adapter (ADR-0026).
//!
//! Runs one hand-off as a non-interactive Claude Code session (`claude -p`) on the user's
//! machine, signed in with the user's Claude subscription. Billing safety is enforced in
//! three independent layers:
//!
//! 1. **Preflight:** `claude auth status --json` must report a `claude.ai` login with the
//!    first-party provider. API-key, console or cloud-provider authentication is refused.
//! 2. **Environment:** the client starts with an allowlisted environment, so
//!    `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, Bedrock/Vertex/Foundry switches, EAEH
//!    tokens and `EAEH_SECRET_*` never reach it. `--bare` (which forces API-key auth) is
//!    never used.
//! 3. **Session check:** the session's `init` event must report `apiKeySource: "none"`
//!    (subscription login); otherwise the process is stopped at once.
//!
//! The session has **no built-in tools** (`--tools ""`): no shell, no file access, no web.
//! Its only tools are the three task-scoped EAEH MCP tools of this one hand-off, all
//! governed and audited by EAEH. EAEH itself makes no model call and opens no network
//! connection here.

use super::{
    AuthState, ClientReport, ExecFailure, ExecResult, ExecutionRequest, ExternalExecutor, FailureClass,
    Preflight,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio_util::sync::CancellationToken;

/// Flags this adapter relies on; a client without them is reported as unavailable.
const REQUIRED_FLAGS: [&str; 6] = [
    "--output-format",
    "--strict-mcp-config",
    "--mcp-config",
    "--allowedTools",
    "--permission-prompts",
    "--no-session-persistence",
];

/// Environment variables passed to the client. Everything else (API keys, provider
/// switches, EAEH tokens and secrets) is withheld.
const ENV_ALLOW: [&str; 16] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "TERM",
    "TZ",
    "CLAUDE_CONFIG_DIR",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "XDG_CONFIG_HOME",
];

/// Variables that would switch Claude Code to separately billed authentication.
pub const BILLING_ENV: [&str; 8] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "AWS_BEARER_TOKEN_BEDROCK",
    "ANTHROPIC_VERTEX_PROJECT_ID",
];

/// The MCP tools the session may call (server name `eaeh`).
pub const ALLOWED_MCP_TOOLS: [&str; 4] = [
    "mcp__eaeh__eaeh_task_get",
    "mcp__eaeh__eaeh_task_tool",
    "mcp__eaeh__eaeh_task_validate",
    "mcp__eaeh__eaeh_task_submit",
];

const MAX_STDERR: usize = 64 * 1024;

/// Tool-result cap (tokens) given to Claude Code sessions run by the worker.
pub const MAX_MCP_OUTPUT_TOKENS: u32 = 200_000;

#[derive(Debug, Clone)]
pub struct ClaudeCodeCli {
    configured: Option<PathBuf>,
    model: Option<String>,
}

impl ClaudeCodeCli {
    pub fn new(path: Option<&str>, model: Option<String>) -> Self {
        Self {
            configured: path.map(PathBuf::from),
            model,
        }
    }

    /// The program to run: the configured path, or `claude` on `PATH`.
    pub fn program(&self) -> Option<PathBuf> {
        if let Some(p) = &self.configured {
            return p.is_file().then(|| p.clone());
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|d| d.join("claude"))
            .find(|p| p.is_file())
    }

    /// The allowlisted environment for the client.
    pub fn client_env() -> Vec<(String, String)> {
        let mut env = filter_env(std::env::vars());
        // Claude Code caps an MCP tool result at 25 000 tokens by default; a task package
        // (e.g. requirements refinement with the analysis and BRD) can be larger. Raise the
        // cap for these sessions with Claude Code's own setting.
        env.push(("MAX_MCP_OUTPUT_TOKENS".into(), MAX_MCP_OUTPUT_TOKENS.to_string()));
        env
    }

    /// Billing-relevant variables present in EAEH's own environment (names only). They are
    /// withheld from the client, but reported so the user knows.
    pub fn billing_env_present() -> Vec<String> {
        BILLING_ENV
            .iter()
            .filter(|k| std::env::var_os(k).is_some())
            .map(|k| k.to_string())
            .collect()
    }

    fn command(&self, program: &Path) -> tokio::process::Command {
        let mut c = tokio::process::Command::new(program);
        c.env_clear().envs(Self::client_env());
        c.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        c
    }

    async fn capture(&self, program: &Path, args: &[&str]) -> std::result::Result<(i32, String), String> {
        let mut c = self.command(program);
        c.args(args);
        let out = tokio::time::timeout(Duration::from_secs(30), c.output())
            .await
            .map_err(|_| {
                format!(
                    "`{} {}` did not finish in 30 s",
                    program.display(),
                    args.join(" ")
                )
            })?
            .map_err(|e| format!("cannot run {}: {e}", program.display()))?;
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        if text.trim().is_empty() {
            text = String::from_utf8_lossy(&out.stderr).to_string();
        }
        Ok((out.status.code().unwrap_or(-1), text))
    }

    /// Arguments of one session (the prompt is sent on stdin).
    pub fn session_args(&self, req: &ExecutionRequest) -> Vec<String> {
        let mcp = json!({"mcpServers": {"eaeh": {
            "type": "stdio",
            "command": req.mcp.program.display().to_string(),
            "args": req.mcp.args,
            "env": req.mcp.env.iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect::<serde_json::Map<String, Value>>(),
        }}});
        let mut a: Vec<String> = vec![
            "-p".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--strict-mcp-config".into(),
            "--mcp-config".into(),
            mcp.to_string(),
            "--tools".into(),
            String::new(),
            "--allowedTools".into(),
            ALLOWED_MCP_TOOLS.join(","),
            "--permission-prompts".into(),
            "none".into(),
            "--no-session-persistence".into(),
            "--disable-slash-commands".into(),
        ];
        if let Some(m) = &self.model {
            a.push("--model".into());
            a.push(m.clone());
        }
        a
    }
}

/// Keep only allowlisted variables (and `LC_*`); everything else is withheld.
pub fn filter_env(vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.into_iter()
        .filter(|(k, _)| ENV_ALLOW.contains(&k.as_str()) || k.starts_with("LC_"))
        .collect()
}

/// Classify an error text reported by the client.
pub fn classify_error(text: &str) -> FailureClass {
    let t = text.to_ascii_lowercase();
    if t.contains("usage limit")
        || t.contains("limit reached")
        || t.contains("hit your")
        || t.contains("rate limit")
        || t.contains("rate_limit")
    {
        FailureClass::UsageLimit
    } else if t.contains("/login")
        || t.contains("not logged in")
        || t.contains("oauth token")
        || t.contains("invalid api key")
        || t.contains("authentication")
        || t.contains("unauthorized")
    {
        FailureClass::AuthUnavailable
    } else if t.contains("credit balance") || t.contains("billing") {
        FailureClass::BillingUnsafe
    } else {
        FailureClass::ToolFailure
    }
}

/// Interpret `claude auth status --json`.
pub fn auth_from_status(v: &Value) -> AuthState {
    let logged_in = v.get("loggedIn").and_then(Value::as_bool).unwrap_or(false);
    if !logged_in {
        return AuthState::NotLoggedIn;
    }
    let method = v.get("authMethod").and_then(Value::as_str).unwrap_or("");
    let provider = v.get("apiProvider").and_then(Value::as_str).unwrap_or("");
    if provider != "firstParty" {
        return AuthState::ApiKeyBilling {
            reason: format!("API provider '{provider}'"),
        };
    }
    if method == "claude.ai" {
        return AuthState::Subscription {
            plan: v
                .get("subscriptionType")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
    }
    if method.is_empty() {
        AuthState::Unknown {
            reason: "the client did not report its authentication method".into(),
        }
    } else {
        AuthState::ApiKeyBilling {
            reason: format!("authentication method '{method}'"),
        }
    }
}

fn kill_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        // The client runs in its own process group (it starts the EAEH MCP server and
        // helpers); stop the whole group.
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = pid;
}

fn fail(class: FailureClass, detail: impl Into<String>, report: Option<ClientReport>) -> ExecFailure {
    ExecFailure {
        class,
        detail: detail.into(),
        report,
    }
}

#[async_trait]
impl ExternalExecutor for ClaudeCodeCli {
    fn provider(&self) -> &str {
        "claude_code"
    }

    fn client_label(&self) -> &str {
        "claude-code"
    }

    async fn preflight(&self) -> Preflight {
        let mut pf = Preflight {
            provider: "claude_code".into(),
            available: false,
            program: None,
            version: None,
            auth: AuthState::Unknown {
                reason: "not checked".into(),
            },
            detail: String::new(),
        };
        let Some(program) = self.program() else {
            pf.detail = match &self.configured {
                Some(p) => format!("Claude Code CLI not found at {}", p.display()),
                None => "Claude Code CLI (`claude`) not found on PATH".into(),
            };
            return pf;
        };
        pf.program = Some(program.display().to_string());
        match self.capture(&program, &["--version"]).await {
            Ok((0, v)) => pf.version = v.split_whitespace().next().map(str::to_string),
            Ok((code, v)) => {
                pf.detail = format!("`claude --version` failed ({code}): {}", v.trim());
                return pf;
            }
            Err(e) => {
                pf.detail = e;
                return pf;
            }
        }
        match self.capture(&program, &["--help"]).await {
            Ok((_, help)) => {
                let missing: Vec<&str> = REQUIRED_FLAGS
                    .iter()
                    .copied()
                    .filter(|f| !help.contains(f))
                    .collect();
                if !missing.is_empty() {
                    pf.detail = format!(
                        "this Claude Code version ({}) lacks required options: {}; update Claude Code",
                        pf.version.clone().unwrap_or_default(),
                        missing.join(", ")
                    );
                    return pf;
                }
            }
            Err(e) => {
                pf.detail = e;
                return pf;
            }
        }
        pf.available = true;
        pf.auth = match self.capture(&program, &["auth", "status", "--json"]).await {
            Ok((_, text)) => match serde_json::from_str::<Value>(text.trim()) {
                Ok(v) => auth_from_status(&v),
                Err(_) => AuthState::Unknown {
                    reason: "`claude auth status --json` returned no JSON".into(),
                },
            },
            Err(e) => AuthState::Unknown { reason: e },
        };
        pf.detail = format!(
            "Claude Code {} – {}",
            pf.version.clone().unwrap_or_default(),
            pf.auth.summary()
        );
        pf
    }

    async fn execute(&self, req: &ExecutionRequest, cancel: CancellationToken) -> ExecResult {
        let Some(program) = self.program() else {
            return Err(fail(
                FailureClass::CliUnavailable,
                "Claude Code CLI not found",
                None,
            ));
        };
        let mut cmd = self.command(&program);
        cmd.args(self.session_args(req))
            .current_dir(&req.workdir)
            .stdin(Stdio::piped());
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|e| {
            fail(
                FailureClass::CliUnavailable,
                format!("cannot start Claude Code: {e}"),
                None,
            )
        })?;
        let pid = child.id();
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(req.prompt.as_bytes()).await;
            let _ = stdin.shutdown().await;
        }
        let stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let err_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(e) = stderr.as_mut() {
                let mut chunk = [0u8; 4096];
                while let Ok(n) = e.read(&mut chunk).await {
                    if n == 0 {
                        break;
                    }
                    if buf.len() < MAX_STDERR {
                        buf.extend_from_slice(&chunk[..n]);
                    }
                }
            }
            String::from_utf8_lossy(&buf).to_string()
        });
        let mut report = ClientReport::default();
        let mut result_seen: Option<(bool, String)> = None;
        let mut abort: Option<ExecFailure> = None;
        let deadline = tokio::time::sleep(req.timeout);
        tokio::pin!(deadline);
        if let Some(out) = stdout {
            let mut lines = BufReader::new(out).lines();
            loop {
                tokio::select! {
                    _ = &mut deadline => {
                        abort = Some(fail(FailureClass::Timeout, format!("no result within {} s", req.timeout.as_secs()), None));
                        break;
                    }
                    _ = cancel.cancelled() => {
                        abort = Some(fail(FailureClass::Cancelled, "cancelled", None));
                        break;
                    }
                    line = lines.next_line() => {
                        let Ok(Some(line)) = line else { break };
                        let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
                        match (ev.get("type").and_then(Value::as_str), ev.get("subtype").and_then(Value::as_str)) {
                            (Some("system"), Some("init")) => {
                                let src = ev.get("apiKeySource").and_then(Value::as_str).unwrap_or("").to_string();
                                report.auth_source = Some(src.clone());
                                if src != "none" {
                                    abort = Some(fail(
                                        FailureClass::BillingUnsafe,
                                        format!("the session reported credential source '{src}' instead of the subscription login; stopped"),
                                        None,
                                    ));
                                    break;
                                }
                                let eaeh_ok = ev.get("mcp_servers").and_then(Value::as_array).is_some_and(|s| {
                                    s.iter().any(|m| m.get("name").and_then(Value::as_str) == Some("eaeh")
                                        && m.get("status").and_then(Value::as_str) == Some("connected"))
                                });
                                if !eaeh_ok {
                                    abort = Some(fail(
                                        FailureClass::ToolFailure,
                                        format!("the EAEH MCP server did not connect: {}", ev.get("mcp_servers").cloned().unwrap_or(Value::Null)),
                                        None,
                                    ));
                                    break;
                                }
                            }
                            (Some("result"), _) => {
                                report.num_turns = ev.get("num_turns").and_then(Value::as_i64);
                                report.duration_ms = ev.get("duration_ms").and_then(Value::as_i64);
                                report.notional_cost_usd = ev.get("total_cost_usd").and_then(Value::as_f64);
                                let text = ev.get("result").and_then(Value::as_str).unwrap_or("").to_string();
                                report.final_text = text.chars().take(4000).collect();
                                let is_error = ev.get("is_error").and_then(Value::as_bool).unwrap_or(false)
                                    || ev.get("subtype").and_then(Value::as_str).is_some_and(|s| s != "success");
                                result_seen = Some((is_error, text));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        if abort.is_some() {
            kill_group(pid);
            let _ = child.start_kill();
        }
        let status = tokio::time::timeout(Duration::from_secs(20), child.wait()).await;
        if status.is_err() {
            kill_group(pid);
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        let stderr = err_task.await.unwrap_or_default();
        if let Some(mut f) = abort {
            f.report = Some(report);
            return Err(f);
        }
        match result_seen {
            Some((false, _)) => Ok(report),
            Some((true, text)) => {
                let class = classify_error(&format!("{text}\n{stderr}"));
                Err(fail(
                    class,
                    text.chars().take(1000).collect::<String>(),
                    Some(report),
                ))
            }
            None => {
                let code = match status {
                    Ok(Ok(s)) => s.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into()),
                    _ => "unknown".into(),
                };
                let tail: String = stderr
                    .chars()
                    .rev()
                    .take(1000)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let class = match classify_error(&stderr) {
                    FailureClass::ToolFailure => FailureClass::Crashed,
                    c => c,
                };
                Err(fail(
                    class,
                    format!(
                        "Claude Code ended without a result (exit {code}): {}",
                        tail.trim()
                    ),
                    Some(report),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_login_is_recognised_and_api_keys_refused() {
        let sub = json!({"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty", "subscriptionType": "pro"});
        assert_eq!(
            auth_from_status(&sub),
            AuthState::Subscription {
                plan: Some("pro".into())
            }
        );
        let key = json!({"loggedIn": true, "authMethod": "api_key", "apiProvider": "firstParty"});
        assert!(matches!(auth_from_status(&key), AuthState::ApiKeyBilling { .. }));
        let bedrock = json!({"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "bedrock"});
        assert!(matches!(
            auth_from_status(&bedrock),
            AuthState::ApiKeyBilling { .. }
        ));
        assert_eq!(
            auth_from_status(&json!({"loggedIn": false})),
            AuthState::NotLoggedIn
        );
        assert!(matches!(
            auth_from_status(&json!({"loggedIn": true, "apiProvider": "firstParty"})),
            AuthState::Unknown { .. }
        ));
    }

    #[test]
    fn error_texts_are_classified() {
        assert_eq!(
            classify_error("Claude AI usage limit reached|1700000000"),
            FailureClass::UsageLimit
        );
        assert_eq!(
            classify_error("You've hit your session limit · resets 3pm"),
            FailureClass::UsageLimit
        );
        assert_eq!(
            classify_error("Invalid API key · Please run /login"),
            FailureClass::AuthUnavailable
        );
        assert_eq!(
            classify_error("OAuth token has expired"),
            FailureClass::AuthUnavailable
        );
        assert_eq!(
            classify_error("Credit balance is too low"),
            FailureClass::BillingUnsafe
        );
        assert_eq!(classify_error("tool crashed"), FailureClass::ToolFailure);
    }

    #[test]
    fn session_has_no_builtin_tools_and_only_task_scoped_mcp_tools() {
        let c = ClaudeCodeCli::new(None, Some("sonnet".into()));
        let req = ExecutionRequest {
            task_id: "t".into(),
            prompt: "p".into(),
            workdir: PathBuf::from("/tmp"),
            mcp: super::super::McpLaunch {
                program: PathBuf::from("/bin/eaeh"),
                args: vec!["mcp".into(), "--task".into(), "t".into()],
                env: vec![("EAEH_PROFILE".into(), "developer".into())],
            },
            timeout: Duration::from_secs(60),
        };
        let a = c.session_args(&req);
        let pos = |f: &str| a.iter().position(|x| x == f).unwrap();
        assert_eq!(a[pos("--tools") + 1], "");
        assert_eq!(a[pos("--allowedTools") + 1], ALLOWED_MCP_TOOLS.join(","));
        assert_eq!(a[pos("--permission-prompts") + 1], "none");
        assert!(a.contains(&"--strict-mcp-config".to_string()));
        assert!(!a.iter().any(|x| x == "--bare" || x.contains("dangerously")));
        let mcp: Value = serde_json::from_str(&a[pos("--mcp-config") + 1]).unwrap();
        assert_eq!(mcp["mcpServers"]["eaeh"]["args"][1], "--task");
        assert_eq!(mcp["mcpServers"]["eaeh"]["env"]["EAEH_PROFILE"], "developer");
        assert_eq!(a[pos("--model") + 1], "sonnet");
    }

    #[test]
    fn the_environment_filter_drops_api_keys_tokens_and_secrets() {
        let vars = [
            ("PATH", "/bin"),
            ("HOME", "/h"),
            ("LC_ALL", "C"),
            ("ANTHROPIC_API_KEY", "sk-ant-x"),
            ("ANTHROPIC_AUTH_TOKEN", "t"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("EAEH_TOKEN", "eaeh_x"),
            ("EAEH_SECRET_ANTHROPIC_API_KEY", "sk"),
            ("AWS_SECRET_ACCESS_KEY", "a"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let kept: Vec<String> = filter_env(vars).into_iter().map(|(k, _)| k).collect();
        assert_eq!(kept, vec!["PATH", "HOME", "LC_ALL"]);
    }

    #[test]
    fn the_client_environment_withholds_billing_and_eaeh_secrets() {
        for k in BILLING_ENV {
            assert!(!ENV_ALLOW.contains(&k));
        }
        assert!(!ENV_ALLOW.contains(&"EAEH_TOKEN"));
        assert!(!ENV_ALLOW.iter().any(|k| k.starts_with("EAEH_")));
    }

    // ---- process-level tests with a fake `claude` program (test doubles, not Claude Code)

    #[cfg(unix)]
    mod fake_cli {
        use super::*;
        use crate::coordinator::McpLaunch;
        use std::os::unix::fs::PermissionsExt;

        const HELP: &str = "--output-format --strict-mcp-config --mcp-config --allowedTools --permission-prompts --no-session-persistence";

        /// Write a fake `claude` that answers --version/--help/auth status and runs `session`
        /// for a `-p` session (stdin = prompt).
        fn fake(dir: &Path, auth: &str, session: &str) -> PathBuf {
            let p = dir.join("claude");
            let script = format!(
                "#!/bin/sh\ncase \"$1\" in\n  --version) echo '9.9.9 (Claude Code)';;\n  --help) echo '{HELP}';;\n  auth) echo '{auth}';;\n  -p) cat >/dev/null\n{session}\n;;\nesac\n"
            );
            std::fs::write(&p, script).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        const SUB: &str = r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","subscriptionType":"pro"}"#;
        const INIT_OK: &str = r#"echo '{"type":"system","subtype":"init","apiKeySource":"none","mcp_servers":[{"name":"eaeh","status":"connected"}]}'"#;

        fn req(dir: &Path, secs: u64) -> ExecutionRequest {
            ExecutionRequest {
                task_id: "t".into(),
                prompt: "do it".into(),
                workdir: dir.to_path_buf(),
                mcp: McpLaunch {
                    program: PathBuf::from("/bin/true"),
                    args: vec![],
                    env: vec![],
                },
                timeout: Duration::from_secs(secs),
            }
        }

        fn cli(p: &Path) -> ClaudeCodeCli {
            ClaudeCodeCli::new(Some(p.to_str().unwrap()), None)
        }

        #[tokio::test]
        async fn preflight_detects_missing_cli_old_versions_and_billing() {
            let d = tempfile::tempdir().unwrap();
            let missing = ClaudeCodeCli::new(Some(d.path().join("nope").to_str().unwrap()), None);
            let pf = missing.preflight().await;
            assert!(!pf.available);
            assert_eq!(pf.usable().unwrap_err().0, FailureClass::CliUnavailable);

            let ok = cli(&fake(d.path(), SUB, "true")).preflight().await;
            assert!(ok.usable().is_ok(), "{ok:?}");
            assert_eq!(ok.version.as_deref(), Some("9.9.9"));

            let key = cli(&fake(
                d.path(),
                r#"{"loggedIn":true,"authMethod":"api_key","apiProvider":"firstParty"}"#,
                "true",
            ))
            .preflight()
            .await;
            assert_eq!(key.usable().unwrap_err().0, FailureClass::BillingUnsafe);

            let out = cli(&fake(d.path(), r#"{"loggedIn":false}"#, "true"))
                .preflight()
                .await;
            assert_eq!(out.usable().unwrap_err().0, FailureClass::AuthUnavailable);

            // A version without the required options is not used.
            let p = d.path().join("claude");
            std::fs::write(
                &p,
                "#!/bin/sh\ncase \"$1\" in --version) echo '0.1.0';; *) echo 'old';; esac\n",
            )
            .unwrap();
            let old = cli(&p).preflight().await;
            assert_eq!(old.usable().unwrap_err().0, FailureClass::CliUnavailable);
            assert!(old.detail.contains("lacks required options"), "{}", old.detail);
        }

        #[tokio::test]
        async fn a_successful_session_reports_metadata() {
            let d = tempfile::tempdir().unwrap();
            let session = format!(
                "{INIT_OK}\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"EAEH_RESULT accepted\",\"num_turns\":7,\"duration_ms\":1234,\"total_cost_usd\":0.5}}'"
            );
            let r = cli(&fake(d.path(), SUB, &session))
                .execute(&req(d.path(), 30), CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(r.num_turns, Some(7));
            assert_eq!(r.auth_source.as_deref(), Some("none"));
            assert_eq!(r.notional_cost_usd, Some(0.5));
        }

        #[tokio::test]
        async fn a_session_on_api_key_credentials_is_stopped() {
            let d = tempfile::tempdir().unwrap();
            let session = r#"echo '{"type":"system","subtype":"init","apiKeySource":"ANTHROPIC_API_KEY","mcp_servers":[]}'; sleep 30"#;
            let t = std::time::Instant::now();
            let e = cli(&fake(d.path(), SUB, session))
                .execute(&req(d.path(), 60), CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(e.class, FailureClass::BillingUnsafe);
            assert!(t.elapsed() < Duration::from_secs(20), "stopped at once");
        }

        #[tokio::test]
        async fn usage_limits_auth_errors_and_crashes_are_classified() {
            let d = tempfile::tempdir().unwrap();
            let limit = format!(
                "{INIT_OK}\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"Claude AI usage limit reached|1700000000\"}}'"
            );
            let e = cli(&fake(d.path(), SUB, &limit))
                .execute(&req(d.path(), 30), CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(e.class, FailureClass::UsageLimit);
            let auth = format!(
                "{INIT_OK}\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"Invalid API key · Please run /login\"}}'"
            );
            let e = cli(&fake(d.path(), SUB, &auth))
                .execute(&req(d.path(), 30), CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(e.class, FailureClass::AuthUnavailable);
            let crash = format!("{INIT_OK}\necho 'segfault' >&2; exit 139");
            let e = cli(&fake(d.path(), SUB, &crash))
                .execute(&req(d.path(), 30), CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(e.class, FailureClass::Crashed);
            assert!(e.detail.contains("exit 139"), "{}", e.detail);
            let no_mcp = r#"echo '{"type":"system","subtype":"init","apiKeySource":"none","mcp_servers":[{"name":"eaeh","status":"failed"}]}'; sleep 30"#;
            let e = cli(&fake(d.path(), SUB, no_mcp))
                .execute(&req(d.path(), 30), CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(e.class, FailureClass::ToolFailure);
        }

        #[tokio::test]
        async fn timeouts_and_cancellation_stop_the_process_group() {
            let d = tempfile::tempdir().unwrap();
            let pidfile = d.path().join("child.pid");
            // The fake starts a grandchild (like an MCP server) and hangs.
            let hang = format!("{INIT_OK}\nsleep 300 & echo $! > {}\nwait", pidfile.display());
            let p = fake(d.path(), SUB, &hang);
            let t = std::time::Instant::now();
            let e = cli(&p)
                .execute(&req(d.path(), 4), CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(e.class, FailureClass::Timeout);
            assert!(t.elapsed() < Duration::from_secs(15));
            tokio::time::sleep(Duration::from_millis(300)).await;
            let gpid = std::fs::read_to_string(&pidfile).unwrap();
            let alive = std::process::Command::new("kill")
                .args(["-0", gpid.trim()])
                .status()
                .unwrap()
                .success();
            assert!(!alive, "the grandchild was stopped with the group");

            let cancel = CancellationToken::new();
            let c2 = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(500)).await;
                c2.cancel();
            });
            let e = cli(&p).execute(&req(d.path(), 60), cancel).await.unwrap_err();
            assert_eq!(e.class, FailureClass::Cancelled);
        }

        #[tokio::test]
        async fn the_session_gets_the_filtered_environment_and_no_shell_tools() {
            let d = tempfile::tempdir().unwrap();
            let dump = d.path().join("env.txt");
            let args = d.path().join("args.txt");
            let session = format!(
                "env > {}\nfor a in \"$@\"; do echo \"[$a]\"; done > {}\n{INIT_OK}\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\"}}'",
                dump.display(),
                args.display()
            );
            cli(&fake(d.path(), SUB, &session))
                .execute(&req(d.path(), 30), CancellationToken::new())
                .await
                .unwrap();
            let env = std::fs::read_to_string(&dump).unwrap();
            for line in env.lines() {
                let k = line.split('=').next().unwrap_or("");
                assert!(
                    ENV_ALLOW.contains(&k)
                        || k.starts_with("LC_")
                        || k == "MAX_MCP_OUTPUT_TOKENS"
                        || k == "PWD"
                        || k == "SHLVL"
                        || k == "_"
                        || k == "OLDPWD",
                    "unexpected variable {k} passed to the client"
                );
            }
            assert!(
                env.lines()
                    .any(|l| l == format!("MAX_MCP_OUTPUT_TOKENS={MAX_MCP_OUTPUT_TOKENS}")),
                "large task packages must fit in one tool result"
            );
            let a = std::fs::read_to_string(&args).unwrap();
            assert!(a.contains("[--tools]\n[]\n"), "{a}");
            assert!(!a.contains("--bare") && !a.contains("dangerously"));
        }
    }
}
