//! Built-in Phase 1 tools exercising the gateway end to end. They are generic
//! primitives, not domain integrations.
//!
//! | Tool | Risk | Isolation | Notes |
//! |---|---|---|---|
//! | `fs.read` | read_only | in-process, workspace-confined | size-limited |
//! | `fs.list` | read_only | in-process, workspace-confined | |
//! | `fs.write` | workspace_write | in-process, workspace-confined | acquires a path lock for the caller |
//! | `exec.run` | build_test | **all** isolation capabilities required | program allowlist |

use crate::registry::{Tool, ToolContext, ToolDescriptor};
use async_trait::async_trait;
use eaeh_core::{HarnessError, Result, RiskClass, ToolId};
use eaeh_isolation::{ExecutionSpec, IsolationCapabilities, IsolationRequirements, ResourceLimits};
use serde_json::{Value, json};
use std::time::Duration;

const MAX_READ: usize = 1024 * 1024;

fn desc(id: &str, description: &str, risk: RiskClass, input: Value, output: Value) -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::new(id).unwrap_or_else(|_| unreachable_tool_id()),
        version: semver::Version::new(1, 0, 0),
        description: description.into(),
        risk,
        input_schema: input,
        output_schema: output,
        isolation: IsolationRequirements::default(),
        needs_execution: false,
        needs_workspace: true,
        timeout_ms: 10_000,
        rate_limit_per_minute: Some(600),
        idempotent: false,
        max_retries: 0,
    }
}

fn unreachable_tool_id() -> ToolId {
    // All ids above are static valid slugs; this fallback is never used.
    ToolId::new("invalid").unwrap_or_else(|_| panic!("static tool id"))
}

pub struct FsRead(ToolDescriptor);
pub struct FsList(ToolDescriptor);
pub struct FsWrite(ToolDescriptor);
pub struct ExecRun {
    d: ToolDescriptor,
    allowed_programs: Vec<String>,
    limits: ResourceLimits,
}

impl Default for FsRead {
    fn default() -> Self {
        let mut d = desc(
            "fs.read",
            "Read a UTF-8 text file from the task workspace.",
            RiskClass::ReadOnly,
            json!({"type":"object","additionalProperties":false,"required":["path"],"properties":{"path":{"type":"string","minLength":1,"maxLength":1024}}}),
            json!({"type":"object","required":["content","size"],"properties":{"content":{"type":"string"},"size":{"type":"integer"}}}),
        );
        d.idempotent = true;
        d.max_retries = 1;
        Self(d)
    }
}

impl Default for FsList {
    fn default() -> Self {
        let mut d = desc(
            "fs.list",
            "List entries of a workspace directory.",
            RiskClass::ReadOnly,
            json!({"type":"object","additionalProperties":false,"properties":{"path":{"type":"string","maxLength":1024}}}),
            json!({"type":"object","required":["entries"],"properties":{"entries":{"type":"array"}}}),
        );
        d.idempotent = true;
        Self(d)
    }
}

impl Default for FsWrite {
    fn default() -> Self {
        Self(desc(
            "fs.write",
            "Write a UTF-8 text file in the task workspace (acquires a path lock for the caller).",
            RiskClass::WorkspaceWrite,
            json!({"type":"object","additionalProperties":false,"required":["path","content"],"properties":{"path":{"type":"string","minLength":1,"maxLength":1024},"content":{"type":"string","maxLength":4194304}}}),
            json!({"type":"object","required":["bytes_written"],"properties":{"bytes_written":{"type":"integer"}}}),
        ))
    }
}

impl ExecRun {
    pub fn new(allowed_programs: Vec<String>, limits: ResourceLimits) -> Self {
        let mut d = desc(
            "exec.run",
            "Run an allow-listed program inside an isolated execution environment with the workspace mounted.",
            RiskClass::BuildTest,
            json!({"type":"object","additionalProperties":false,"required":["program"],"properties":{
                "program":{"type":"string","minLength":1,"maxLength":256},
                "args":{"type":"array","maxItems":64,"items":{"type":"string","maxLength":4096}}}}),
            json!({"type":"object","required":["exit_code","stdout","stderr","timed_out"],"properties":{
                "exit_code":{"type":["integer","null"]},"stdout":{"type":"string"},"stderr":{"type":"string"},
                "timed_out":{"type":"boolean"},"truncated":{"type":"boolean"},"provider":{"type":"string"}}}),
        );
        d.isolation = IsolationCapabilities::all();
        d.needs_execution = true;
        d.timeout_ms = u64::try_from(limits.timeout.as_millis())
            .unwrap_or(u64::MAX)
            .saturating_add(30_000);
        d.rate_limit_per_minute = Some(60);
        Self {
            d,
            allowed_programs,
            limits,
        }
    }
}

fn str_arg<'a>(input: &'a Value, k: &str) -> Result<&'a str> {
    input
        .get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| HarnessError::invalid_input(format!("missing '{k}'")))
}

#[async_trait]
impl Tool for FsRead {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.0
    }
    async fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value> {
        let bytes = ctx.workspace()?.read_file(str_arg(&input, "path")?, MAX_READ)?;
        let size = bytes.len();
        let content =
            String::from_utf8(bytes).map_err(|_| HarnessError::invalid_input("file is not UTF-8 text"))?;
        Ok(json!({"content": content, "size": size}))
    }
}

#[async_trait]
impl Tool for FsList {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.0
    }
    async fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value> {
        let path = input
            .get("path")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty() && *p != ".");
        Ok(json!({"entries": ctx.workspace()?.list(path)?}))
    }
}

#[async_trait]
impl Tool for FsWrite {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.0
    }
    async fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value> {
        let ws = ctx.workspace()?;
        let path = str_arg(&input, "path")?;
        let content = str_arg(&input, "content")?;
        let holder = ctx.holder();
        ws.acquire_lock(path, &holder, chrono::Duration::minutes(10))
            .await?;
        ws.write_file(&holder, path, content.as_bytes(), ctx.trace)
            .await?;
        Ok(json!({"bytes_written": content.len()}))
    }
}

#[async_trait]
impl Tool for ExecRun {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.d
    }
    async fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value> {
        let program = str_arg(&input, "program")?;
        if !self.allowed_programs.iter().any(|p| p == program) {
            return Err(HarnessError::forbidden(format!(
                "program '{program}' is not in the exec.run allowlist"
            )));
        }
        let args: Vec<String> = input
            .get("args")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let provider = ctx
            .provider
            .as_ref()
            .ok_or_else(|| HarnessError::isolation("no execution provider selected"))?;
        let ws = ctx.workspace()?;
        let out = provider
            .execute(ExecutionSpec {
                program: program.into(),
                args,
                env: [("EAEH_INVOCATION_ID".to_string(), ctx.invocation_id.to_string())].into(),
                workspace: ws.host_root().to_path_buf(),
                limits: self.limits.clone(),
                requirements: self.d.isolation,
            })
            .await?;
        Ok(json!({
            "exit_code": out.exit_code,
            "stdout": out.stdout,
            "stderr": out.stderr,
            "timed_out": out.timed_out,
            "truncated": out.stdout_truncated || out.stderr_truncated,
            "provider": out.provider,
        }))
    }
}

/// Default limits for `exec.run`.
pub fn default_exec_limits() -> ResourceLimits {
    ResourceLimits {
        timeout: Duration::from_secs(120),
        ..Default::default()
    }
}
