//! `eaeh` – EAEH command-line interface.
//!
//! Local administrative commands (`init`, `admin bootstrap`, `audit verify`, `health`)
//! require file-system access to the data directory, which is the trust root. All other
//! commands are governed: they authenticate the caller from a stored profile
//! (`eaeh auth login`, `--as <profile>`) or the `EAEH_TOKEN` environment variable (never a
//! command-line argument, to keep tokens out of shell history) and run through the same
//! authorization, policy and approval services as the HTTP API.

use clap::{Parser, Subcommand};
use eaeh_core::{
    ApprovalId, HarnessError, Principal, PrincipalId, PrincipalKind, ProjectAccess, ProjectId, Result, Role,
    Scope, TokenId, TraceContext, WorkflowDefinitionId, WorkflowInstanceId,
};
use eaeh_governance::authz::{Action, Target};
use eaeh_governance::{ApprovalDecision, ApprovalStatus, NewPrincipal};
use eaeh_runtime::observability::init_tracing;
use eaeh_runtime::{Harness, HarnessConfig};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod credentials;
mod dx;
mod mcp;
mod portal;
mod setup;
mod worker;

use credentials::Need;

/// Write an artifact (a single file, or every file of a bundle manifest, hash-checked) into
/// `out`. Returns the artifact metadata and the written relative paths.
pub(crate) async fn export_artifact(
    h: &Harness,
    scope: Scope,
    id: eaeh_core::ArtifactId,
    out: &Path,
) -> Result<(eaeh_runtime::ArtifactMeta, Vec<String>)> {
    let (meta, bytes) = h.artifacts.read(scope, id).await?;
    std::fs::create_dir_all(out)?;
    let mut written = vec![];
    let manifest: Option<Value> = serde_json::from_slice(&bytes).ok();
    match manifest
        .as_ref()
        .and_then(|m| m.get("files"))
        .and_then(Value::as_object)
    {
        Some(files) => {
            for (path, f) in files {
                let safe = !path.is_empty()
                    && !path.starts_with('/')
                    && path.split('/').all(|c| !c.is_empty() && c != "." && c != "..");
                if !safe {
                    return Err(HarnessError::integrity(format!("unsafe path in bundle: {path}")));
                }
                let fid = f.get("artifact_id").and_then(Value::as_str).unwrap_or_default();
                let (fm, content) = h.artifacts.read(scope, parse(fid)?).await?;
                if Some(fm.content_hash.as_str()) != f.get("hash").and_then(Value::as_str) {
                    return Err(HarnessError::integrity(format!(
                        "{path}: hash differs from the manifest"
                    )));
                }
                let target = out.join(path);
                if let Some(dir) = target.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&target, content)?;
                written.push(path.clone());
            }
        }
        None => {
            let name = meta.name.rsplit('/').next().unwrap_or("artifact").to_string();
            std::fs::write(out.join(&name), &bytes)?;
            written.push(name);
        }
    }
    Ok((meta, written))
}

/// Set when a readiness check reports NOT READY (exit code 7).
static NOT_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Parser)]
#[command(name = "eaeh", version, about = "Enterprise Architecture Engineering Harness")]
struct Cli {
    /// Configuration file.
    #[arg(long, global = true, default_value = "config/eaeh.toml", env = "EAEH_CONFIG")]
    config: PathBuf,
    /// Act as this stored identity profile (see `eaeh auth status`).
    #[arg(long = "as", global = true, env = "EAEH_PROFILE")]
    as_profile: Option<String>,
    /// Machine-readable JSON output for developer-experience commands.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

// Parsed once per process; variant size is irrelevant here.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Cmd {
    /// Create the data directory, apply migrations and load definitions.
    Init,
    /// One-command setup: initialise the database, bootstrap and verify the identities
    /// (admin, developer, approver, steward), register skills (pending approval), run health
    /// checks and start the Governance Portal. Idempotent; repeat runs repair safe problems.
    /// Exit code 7 when anything mandatory is not ready.
    Setup {
        /// Diagnose only: change nothing, start nothing.
        #[arg(long)]
        check: bool,
        /// Also repair credentials that map to the wrong principal or another installation
        /// (the credential file is backed up first) and re-enable disabled standard principals.
        #[arg(long)]
        repair: bool,
        /// Back up and reset the database, then configure from the beginning (needs confirmation).
        #[arg(long)]
        reset: bool,
        /// Non-interactive confirmation of --reset / --restore: the exact phrase `RESET EAEH`.
        #[arg(long = "confirm-reset")]
        confirm_reset: Option<String>,
        /// Restore the database from a backup directory written by setup (needs confirmation).
        #[arg(long)]
        restore: Option<PathBuf>,
        /// Open the Portal in the default browser.
        #[arg(long)]
        open: bool,
        /// Do not start the Governance Portal.
        #[arg(long = "no-portal")]
        no_portal: bool,
        /// Stop the background Governance Portal of this installation and exit.
        #[arg(long = "stop-portal")]
        stop_portal: bool,
        /// Loopback address of the Portal.
        #[arg(long, default_value = "127.0.0.1:8090")]
        bind: String,
        /// Directory with the built web UI.
        #[arg(long, default_value = "portal/dist", env = "EAEH_PORTAL_DIR")]
        static_dir: PathBuf,
        /// Tenant name for a first-time setup.
        #[arg(long, default_value = "local")]
        tenant: String,
        /// Project name for a first-time setup.
        #[arg(long, default_value = "default")]
        project: String,
        /// Lifetime of issued tokens in days.
        #[arg(long, default_value_t = 30)]
        ttl_days: i64,
    },
    /// Local health report.
    Health,
    /// Run the HTTP API server with background dispatch and supervision.
    Serve,
    /// Local Governance Portal (web UI) for dashboards and approval review. Loopback only,
    /// no login: reads as `--as` (or EAEH_TOKEN / default profile) and records approval
    /// decisions only as the profile named with `--approver`. Prints a launch URL.
    Portal {
        /// Loopback address to listen on.
        #[arg(long, default_value = "127.0.0.1:8090")]
        bind: String,
        /// Stored approver profile used for Approve / Request changes / Reject. Omit for a
        /// read-only portal.
        #[arg(long)]
        approver: Option<String>,
        /// Stored steward profile (skill.manage) used only for "Register skill files" on the
        /// Skills page. Omit to disable registration in the portal.
        #[arg(long)]
        steward: Option<String>,
        /// Stored operator profile (workflow.start) for Start / Cancel / Resume / Restart /
        /// Retry. Omit for a portal without workflow operations.
        #[arg(long)]
        operator: Option<String>,
        /// Stored administrator profile (principal.manage) for "Register existing project".
        #[arg(long)]
        admin: Option<String>,
        /// Folder under which "Register existing project" may browse (repeatable; default:
        /// the parent folder of the EAEH repository).
        #[arg(long = "workspace-root")]
        workspace_roots: Vec<PathBuf>,
        /// Directory with the built web UI (`npm --prefix portal run build`).
        #[arg(long, default_value = "portal/dist", env = "EAEH_PORTAL_DIR")]
        static_dir: PathBuf,
        /// Additional allowed browser origin for the Vite dev server (e.g. http://localhost:5173).
        #[arg(long = "dev-origin")]
        dev_origins: Vec<String>,
        /// Open the launch link in the default browser once the portal is listening.
        #[arg(long)]
        open: bool,
    },
    #[command(subcommand)]
    Admin(AdminCmd),
    #[command(subcommand)]
    Audit(AuditCmd),
    #[command(subcommand)]
    Workflow(WorkflowCmd),
    #[command(subcommand)]
    Approval(ApprovalCmd),
    #[command(subcommand)]
    Token(TokenCmd),
    #[command(subcommand)]
    Principal(PrincipalCmd),
    #[command(subcommand)]
    Project(ProjectCmd),
    #[command(subcommand)]
    Plugin(PluginCmd),
    #[command(subcommand)]
    Knowledge(KnowledgeCmd),
    #[command(subcommand)]
    Skill(SkillCmd),
    /// Business requirements documents (Phase 3).
    #[command(subcommand)]
    Brd(BrdCmd),
    /// Traceability links recorded from approved artifacts (Phase 3).
    Trace {
        #[arg(long)]
        project: Option<String>,
        /// Item id (BRD citation, requirement, component, API operation …); all links if omitted.
        #[arg(long)]
        id: Option<String>,
        #[arg(long, default_value_t = 3)]
        depth: u32,
    },
    /// Artifacts: approved documents in the project workspace (status, sync) and export.
    #[command(subcommand)]
    Artifact(ArtifactCmd),
    /// Stored identities: login, status, use, renew, logout.
    #[command(subcommand)]
    Auth(AuthCmd),
    /// Agent tasks handed off to an external AI client (`external_ai` mode): list, show the
    /// package, call the agent's tools through EAEH, submit the result for validation.
    #[command(subcommand)]
    Task(TaskCmd),
    /// MCP server (stdio) for AI clients (Claude Code, GitHub Copilot in VS Code); runs as
    /// the developer profile.
    Mcp {
        /// Repository root containing `.eaeh/project.toml`.
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Serve exactly one hand-off: only `eaeh_task_get`, `eaeh_task_tool` and
        /// `eaeh_task_submit`, only for this task id (used by the automated worker).
        #[arg(long)]
        task: Option<String>,
        /// Execution attempt id (set by the worker) to which context deliveries are traced.
        #[arg(long)]
        execution: Option<String>,
        /// Print the tool names a task-scoped server exposes (JSON) and exit. The worker uses
        /// it to verify that it and the MCP server are the same build.
        #[arg(long = "task-tools")]
        task_tools: bool,
    },
    /// Automated execution of AI tasks through Claude Code on your subscription (ADR-0026):
    /// start/stop the background worker, show status, history and log, retry a stopped task.
    #[command(subcommand)]
    Worker(WorkerCmd),
    /// Review the document waiting at a workflow approval gate and decide on it (approvers;
    /// use `--as <approver-profile>`). Shows the document; no manual export needed.
    Review {
        /// Approval id (optional when only one document waits).
        approval: Option<String>,
        /// approve | request_changes | reject (asked interactively when omitted).
        #[arg(long)]
        decision: Option<String>,
        #[arg(long)]
        comments: Option<String>,
        /// Folder for the review copy (default: review/<gate> in the repository).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Open the document with the system viewer instead of printing it.
        #[arg(long)]
        open: bool,
        /// Do not print the document (e.g. you already read it).
        #[arg(long)]
        no_show: bool,
    },
    /// List registered agent definitions.
    Agents,
    /// List registered tools.
    Tools,
}

#[derive(Subcommand)]
enum WorkerCmd {
    /// Start the background worker for this project (runs as your developer profile).
    Start {
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
    /// Stop the worker (the current AI session is stopped; its task runs again later).
    Stop {
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
    /// Execution mode, provider, Claude Code version and sign-in, worker and waiting tasks.
    Status {
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
    /// Run the worker in the foreground (what `start` launches). --until-idle returns when
    /// the project waits for a person or is finished.
    Run {
        #[arg(long)]
        project_root: Option<PathBuf>,
        #[arg(long)]
        until_idle: bool,
    },
    /// Make a blocked or exhausted task eligible for automatic execution again, after the
    /// cause was fixed. Shows the chain (workflow, stage, package version, earlier results);
    /// accepted results and submissions are never touched.
    Retry {
        task: String,
        /// Refuse unless the task belongs to this workflow (guards against a wrong task id).
        #[arg(long)]
        workflow: Option<String>,
    },
    /// Execution attempts (provider, auth, outcome, failure class, duration).
    History {
        #[arg(long)]
        task: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    /// The last lines of the background worker's log.
    Log {
        #[arg(long, default_value_t = 60)]
        lines: usize,
    },
}

#[derive(Subcommand)]
enum TaskCmd {
    /// Hand-offs of the current project (open ones unless --all).
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// The task package: instructions, data, input, output schema, allowed tools, citable
    /// sources and feedback. Contains project data; --out writes it to a 0600 file.
    Show {
        id: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Call one of the agent's allowed tools for this task (runs as the agent, through the
    /// tool gateway). Input is JSON, or @file.
    Tool {
        id: String,
        tool: String,
        #[arg(long, default_value = "{}")]
        input: String,
    },
    /// Check a draft result exactly as a submission would be checked, without submitting
    /// it or using a correction attempt (file or `-` for stdin). Lists all problems.
    Validate {
        id: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Submit the result (one JSON object, from a file or `-` for stdin). EAEH validates it
    /// and continues the workflow, or reopens the task with feedback.
    Submit {
        id: String,
        #[arg(long)]
        output: PathBuf,
        /// Label of the AI client that produced the result (provenance).
        #[arg(long, default_value = "cli")]
        client: String,
    },
}

#[derive(Subcommand)]
enum AuthCmd {
    /// Store a token as a named profile (read from --token-file, or one line on stdin).
    /// The token is validated and never displayed.
    Login {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        token_file: Option<PathBuf>,
        /// Make this the default profile.
        #[arg(long)]
        default: bool,
    },
    /// Show stored profiles (principal, roles, expiry, validity) – never the tokens.
    Status,
    /// Set the default profile.
    Use { profile: String },
    /// Rotate the profile's token (the old one is revoked) and store the new one.
    Renew {
        #[arg(long)]
        profile: String,
        #[arg(long, default_value_t = 30)]
        ttl_days: i64,
    },
    /// Remove a profile (optionally revoking its token).
    Logout {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        revoke: bool,
    },
}

#[derive(Subcommand)]
enum BrdCmd {
    /// Ingest a Markdown/text BRD as project-scoped governed knowledge (tag `brd`). The
    /// source is created (kind reference) if it does not exist yet.
    Ingest {
        #[arg(long)]
        file: PathBuf,
        /// Defaults to the project of the current repository (.eaeh/project.toml).
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "brd")]
        source: String,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long, default_value = "confidential")]
        classification: String,
    },
}

#[derive(Subcommand)]
enum ArtifactCmd {
    /// Approved documents of a run (latest if no id): version, hash, approval, project
    /// workspace folder and the verified state of the files there.
    Status {
        id: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Write the approved documents of a run (latest if no id) into this repository
    /// (docs/architecture/…), hash-verified and audited. Run inside the project repository.
    /// Never overwrites files changed locally unless --replace-modified is given.
    Sync {
        id: Option<String>,
        #[arg(long)]
        project: Option<String>,
        /// Replace files that were modified locally with the approved version (audited).
        #[arg(long)]
        replace_modified: bool,
    },
    /// Write an artifact to a directory; a bundle manifest exports every file it binds.
    Export {
        #[arg(long)]
        project: Option<String>,
        id: String,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum AdminCmd {
    /// One-time creation of the first tenant, project and administrator. Fails if any
    /// principal exists. The token is printed once, or written to --token-file (0600).
    Bootstrap {
        #[arg(long)]
        tenant: String,
        #[arg(long)]
        project: String,
        #[arg(long)]
        admin_id: String,
        #[arg(long, default_value = "Administrator")]
        display_name: String,
        #[arg(long, default_value_t = 30)]
        ttl_days: i64,
        #[arg(long)]
        token_file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AuditCmd {
    /// Verify the hash chain of the local audit log.
    Verify,
    /// List audit records (requires AuditRead).
    List {
        #[arg(long, default_value_t = 0)]
        after_seq: i64,
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
}

#[derive(Subcommand)]
enum WorkflowCmd {
    /// Start a workflow and run it until it completes or waits for approval.
    Start {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "{}")]
        input: String,
    },
    /// Run an engineering activity (`requirements`, `design`) for the current project:
    /// resolves the BRD and workflow, checks prerequisites, explains approvals; starts
    /// only with --yes.
    Run {
        activity: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        yes: bool,
        /// After starting, show live progress until a person is needed.
        #[arg(long)]
        follow: bool,
        /// Do not start the background worker (AI tasks then wait for `eaeh worker start`
        /// or an interactive assistant).
        #[arg(long)]
        no_worker: bool,
    },
    /// Live progress of a run (latest if no id) until it needs a person or finishes.
    Watch {
        id: Option<String>,
    },
    /// Stages, current gate, failure stage and next actions (latest run if no id).
    Status {
        id: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Continue an interrupted (non-terminal) run.
    Resume {
        id: String,
    },
    /// Restart, retry or reinitiate a run from a stage as a new, linked run. Shows the
    /// preview (carried / re-executed stages, artifact versions, approvals kept or repeated);
    /// changes nothing without --yes.
    Restart {
        id: String,
        /// Restart point (default: the failed or current stage).
        #[arg(long)]
        from: Option<String>,
        /// New run input (JSON or @file), e.g. a new BRD version; only from the first stage.
        #[arg(long)]
        input: Option<String>,
        /// Why the run is restarted (recorded in lineage and audit).
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    Show {
        id: String,
    },
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    Cancel {
        id: String,
    },
    /// List registered workflow definitions.
    Definitions,
}

#[derive(Subcommand)]
enum ApprovalCmd {
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    /// Approver queue for skills and standards: shows each item in full and asks for an
    /// explicit confirmation per item (or --yes after listing). Never auto-approves.
    Review {
        /// Items to decide (skill reference prefix, citation prefix or approval id).
        items: Vec<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        reject: bool,
        #[arg(long, default_value = "")]
        comments: String,
    },
    /// Decide an approval (approve|reject|request_changes) and resume the workflow.
    Decide {
        id: String,
        #[arg(long)]
        decision: String,
        #[arg(long, default_value = "")]
        comments: String,
        #[arg(long)]
        artifact_hash: Option<String>,
    },
}

#[derive(Subcommand)]
enum TokenCmd {
    Issue {
        #[arg(long)]
        principal: Option<String>,
        #[arg(long)]
        label: String,
        #[arg(long, default_value_t = 30)]
        ttl_days: i64,
        /// Write the token to a new 0600 file (hand-over to its owner).
        #[arg(long)]
        token_file: Option<PathBuf>,
        /// Store the token as a profile in your own credential store.
        #[arg(long)]
        store_profile: Option<String>,
        /// Display the token (avoid: it ends up in terminal scroll-back).
        #[arg(long)]
        print: bool,
    },
    List {
        #[arg(long)]
        principal: Option<String>,
    },
    Revoke {
        id: String,
    },
    Rotate {
        id: String,
        #[arg(long, default_value_t = 30)]
        ttl_days: i64,
        #[arg(long)]
        token_file: Option<PathBuf>,
        #[arg(long)]
        store_profile: Option<String>,
        #[arg(long)]
        print: bool,
    },
}

#[derive(Subcommand)]
enum PrincipalCmd {
    Create {
        #[arg(long)]
        id: String,
        #[arg(long)]
        display_name: String,
        /// Comma separated: admin,operator,approver,viewer,auditor,knowledge_steward
        #[arg(long)]
        roles: String,
        /// Comma separated project ids, or `all`.
        #[arg(long, default_value = "all")]
        projects: String,
        #[arg(long, default_value = "human")]
        kind: String,
    },
    Disable {
        id: String,
    },
    /// Give an existing principal access to a project (current project by default).
    Grant {
        #[arg(long)]
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
    /// Set team memberships (comma separated; empty string clears).
    SetTeams {
        id: String,
        #[arg(long)]
        teams: String,
    },
}

#[derive(Subcommand)]
enum KnowledgeCmd {
    /// Register a knowledge source (kind: standard|reference|api_spec|adr|process).
    SourceCreate {
        #[arg(long)]
        name: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        description: String,
    },
    Sources,
    /// Ingest one file. --scope: enterprise | domain:<name> | project:<id> | team:<name> | workflow:<id>
    Ingest {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, default_value = "enterprise")]
        scope: String,
        #[arg(long, default_value = "internal")]
        classification: String,
        #[arg(long)]
        mandatory: bool,
        #[arg(long)]
        review_by: Option<String>,
        /// Governance project (approval scope).
        #[arg(long)]
        project: String,
    },
    /// Ingest every .md/.txt/.json file of a directory (subject = file stem).
    Import {
        dir: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long, default_value = "enterprise")]
        scope: String,
        #[arg(long, default_value = "internal")]
        classification: String,
        #[arg(long)]
        mandatory: bool,
        #[arg(long)]
        project: String,
    },
    List {
        #[arg(long)]
        status: Option<String>,
    },
    Search {
        #[arg(long)]
        project: String,
        #[arg(long)]
        query: String,
        #[arg(long, default_value = "hybrid")]
        mode: String,
        #[arg(long, default_value_t = 5)]
        k: usize,
    },
    Retire {
        id: String,
    },
    Delete {
        id: String,
    },
    Conflicts,
}

#[derive(Subcommand)]
enum SkillCmd {
    /// Register a skill definition (JSON file, or Markdown `.md` with TOML front matter);
    /// requests human approval.
    Register {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        project: String,
    },
    List,
    /// Register the skills required by the project's workflows from the repository
    /// definitions (`definitions.skills_dir`); each needs human approval afterwards.
    Sync {
        #[arg(long)]
        project: Option<String>,
    },
    /// Required skills of the project and their approval state.
    Pending {
        #[arg(long)]
        project: Option<String>,
    },
    /// approved (reinstate) | deprecated | retired
    SetStatus {
        id: String,
        version: String,
        status: String,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum ProjectCmd {
    /// One-command bootstrap (administrator; idempotent): project, repository association,
    /// stack, knowledge/BRD, people and responsibilities, required skills, workflows,
    /// Claude Code files, readiness.
    Init {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, default_value = "")]
        language: String,
        #[arg(long, default_value = "")]
        framework: String,
        #[arg(long, default_value = "")]
        build_tool: String,
        /// Technical constraint (repeatable).
        #[arg(long = "constraint")]
        constraints: Vec<String>,
        /// BRD file (Markdown/text), relative to the workspace.
        #[arg(long)]
        brd: Option<PathBuf>,
        /// Project knowledge file (repeatable).
        #[arg(long = "knowledge")]
        knowledge: Vec<PathBuf>,
        /// Project standard file (repeatable; mandatory for the project's domain after approval).
        #[arg(long = "standard")]
        standards: Vec<PathBuf>,
        /// Principal who runs workflows (operator).
        #[arg(long)]
        developer: String,
        /// Principal who curates knowledge (knowledge_steward).
        #[arg(long)]
        steward: String,
        /// Principal who decides approvals (repeatable; never the developer).
        #[arg(long = "approver")]
        approvers: Vec<String>,
        /// Execution mode the project expects: external_ai (your Claude Code / Copilot
        /// subscription does the AI work; default) | direct_provider | deterministic.
        #[arg(long)]
        mode: Option<String>,
        /// Deprecated alias of --mode: anthropic = direct_provider, replay = deterministic,
        /// none = external_ai.
        #[arg(long, hide = true)]
        provider: Option<String>,
        /// AI client integration to generate (repeatable): claude-code, github-copilot
        /// (default: both on first run; kept on re-runs).
        #[arg(long = "client")]
        clients: Vec<String>,
        /// Software delivery: required (EAEH runs the build and tests, isolated) | deferred
        /// (EAEH does not run them; the delivery is marked unverified).
        #[arg(long)]
        build_verification: Option<String>,
        /// Build/test program for delivery (must be in [isolation] exec_allowed_programs).
        #[arg(long)]
        build_program: Option<String>,
        /// Build/test argument (repeatable).
        #[arg(long = "build-arg", allow_hyphen_values = true)]
        build_args: Vec<String>,
        /// Workflows to enable (repeatable; default: requirements, design and delivery).
        #[arg(long = "workflow")]
        workflows: Vec<String>,
        /// Single-person local setup: store the issued tokens as profiles
        /// (developer, steward, approver) in your credential store.
        #[arg(long)]
        store_profiles: bool,
        /// Where hand-over token files for newly created people are written.
        #[arg(long)]
        handover_dir: Option<PathBuf>,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
        #[arg(long, default_value_t = 30)]
        ttl_days: i64,
    },
    /// Ready / Not Ready report with blocking issues, warnings and fixes (exit code 7 if not ready).
    Readiness {
        #[arg(long)]
        project: Option<String>,
        /// Check only these workflows (default: the project's workflows).
        #[arg(long = "workflow")]
        workflow: Vec<String>,
    },
    Create {
        #[arg(long)]
        name: String,
    },
    List,
    /// Assign the project to a knowledge domain (or clear it with --domain none).
    SetDomain {
        id: String,
        #[arg(long)]
        domain: String,
    },
}

#[derive(Subcommand)]
enum PluginCmd {
    List,
    Enable { id: String, version: String },
    Disable { id: String, version: String },
    Retire { id: String, version: String },
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn parse<T: std::str::FromStr<Err = HarnessError>>(s: &str) -> Result<T> {
    s.parse()
}

static REDACTOR: std::sync::OnceLock<eaeh_core::Redactor> = std::sync::OnceLock::new();

async fn harness(cfg: &HarnessConfig) -> Result<Arc<Harness>> {
    let r = REDACTOR.get_or_init(eaeh_core::Redactor::new).clone();
    Ok(Arc::new(Harness::builder(cfg.clone()).redactor(r).build().await?))
}

/// Any authenticated identity (`--as`, `EAEH_TOKEN`, default profile).
async fn caller(h: &Harness) -> Result<Principal> {
    dx::caller_need(h, Need::Any).await
}

async fn caller_for(h: &Harness, need: Need) -> Result<Principal> {
    dx::caller_need(h, need).await
}

fn project_id(explicit: Option<&str>) -> Result<ProjectId> {
    dx::project_context(explicit).map(|(id, _)| id)
}

/// Actionable guidance for common failures (shown as `hint` with the error).
fn hint_for(e: &HarnessError) -> Option<String> {
    let m = e.message.as_str();
    let h = if m.contains("no registered model satisfies") {
        "No model provider is enabled, so agent stages cannot run. Enable [providers.anthropic] in the configuration (key via EAEH_SECRET_ANTHROPIC_API_KEY), or for deterministic offline runs enable [providers.replay] with a cassette (recorded responses – not AI). Tool-only workflows such as demo.approval run without a model. Check with `eaeh project readiness`."
    } else if m.contains("no approved version of skill")
        || m.contains("references skills but no skills registry")
    {
        "A required skill is not approved. See `eaeh skill pending`; register missing ones with `eaeh skill sync` (steward/admin); an approver decides with `eaeh approval review --as <approver-profile>`."
    } else if m.contains("workflow input has no") {
        "The workflow input is missing a field. For BRD workflows use `eaeh workflow run requirements` (inputs are resolved for you), or pass e.g. --input '{\"brd\":\"<source>/<subject>@v1\"}' (the citation is printed by `eaeh brd ingest`)."
    } else if m.contains("artifact_hash confirmation required") {
        "Approvals bind to the exact artifact you reviewed: take the hash from `eaeh workflow status` (or `eaeh approval list`), review with `eaeh artifact export <id> --out <dir>`, then pass --artifact-hash <hash>."
    } else if m.contains("artifact hash mismatch") {
        "The artifact changed after you reviewed it. Re-run `eaeh workflow status`, review the current version and decide with its hash."
    } else if m.contains("separation of duties") {
        "Authors cannot approve their own work and approvals need a human approver. Use a different approver profile: `--as <approver-profile>`."
    } else if m.contains("quality gate") {
        "A deterministic quality gate rejected the generated output (nothing was stored). Inspect with `eaeh workflow status`; fix inputs (BRD, standards, skills) and start a new run."
    } else if m.contains("no active project") || m.contains(".eaeh/project.toml") {
        "Run the command inside a repository bootstrapped with `eaeh project init`, or pass --project <id>."
    } else {
        match e.kind {
            eaeh_core::ErrorKind::Unauthenticated => {
                "Check your identities with `eaeh auth status`; store a token with `eaeh auth login --profile <name> --token-file <file>`; choose one with `--as <profile>`."
            }
            eaeh_core::ErrorKind::Forbidden if m.contains("roles that grant it") => {
                "Use a profile that holds one of these roles (`eaeh auth status`, then `--as <profile>`), or ask an administrator to grant it. Administrators deliberately cannot operate workflows or approve."
            }
            eaeh_core::ErrorKind::Forbidden => {
                "Your identity is not authorized for this project or action. `eaeh auth status` shows your profiles; an administrator can grant project access with `eaeh principal grant --id <principal>`."
            }
            _ => return None,
        }
    };
    Some(h.to_string())
}

fn write_token_file(path: &Path, token: &str) -> Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o
        .open(path)
        .map_err(|e| HarnessError::conflict(format!("token file {}: {e}", path.display())))?;
    f.write_all(token.as_bytes())?;
    Ok(())
}

async fn run(cli: Cli) -> Result<()> {
    let cfg = HarnessConfig::load(&cli.config)?;
    match cli.cmd {
        Cmd::Init => {
            let h = harness(&cfg).await?;
            print(&json!({"initialized": cfg.data_dir(), "health": h.health().await}));
        }
        Cmd::Setup {
            check,
            repair,
            reset,
            confirm_reset,
            restore,
            open,
            no_portal,
            stop_portal,
            bind,
            static_dir,
            tenant,
            project,
            ttl_days,
        } => {
            setup::run(
                &cfg,
                setup::SetupArgs {
                    check,
                    repair,
                    reset,
                    confirm_reset,
                    restore,
                    open,
                    no_portal,
                    stop_portal,
                    bind,
                    static_dir,
                    tenant,
                    project,
                    ttl_days,
                    config_path: cli.config.clone(),
                },
            )
            .await?;
        }
        Cmd::Health => {
            let h = harness(&cfg).await?;
            print(&serde_json::to_value(h.health().await)?);
        }
        Cmd::Serve => {
            let h = harness(&cfg).await?;
            let recovered = h.start_background().await?;
            tracing::info!(recovered = recovered.len(), "background services started");
            // New or changed skill directories are registered (pending approval) automatically.
            let _discovery = eaeh_runtime::project::spawn_skill_discovery(
                h.clone(),
                eaeh_runtime::project::skill_discovery_interval(),
            );
            let app = eaeh_api::router(eaeh_api::AppState::new(h.clone()));
            let listener = tokio::net::TcpListener::bind(&cfg.api.bind)
                .await
                .map_err(|e| HarnessError::unavailable(format!("bind {}: {e}", cfg.api.bind)))?;
            tracing::info!(bind = %cfg.api.bind, "EAEH API listening");
            eprintln!("EAEH API listening on http://{}", cfg.api.bind);
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await
                .map_err(|e| HarnessError::internal(format!("server: {e}")))?;
            h.shutdown().await;
        }
        Cmd::Portal {
            bind,
            approver,
            steward,
            operator,
            admin,
            workspace_roots,
            static_dir,
            dev_origins,
            open,
        } => {
            portal::serve(
                &cfg,
                portal::PortalArgs {
                    bind,
                    approver,
                    steward,
                    operator,
                    admin,
                    workspace_roots,
                    config_path: cli.config.clone(),
                    static_dir,
                    dev_origins,
                    open,
                },
            )
            .await?;
        }
        Cmd::Admin(AdminCmd::Bootstrap {
            tenant,
            project,
            admin_id,
            display_name,
            ttl_days,
            token_file,
        }) => {
            let h = harness(&cfg).await?;
            let (admin, project_id, tok) = h
                .identity
                .bootstrap(
                    &tenant,
                    &project,
                    PrincipalId::new(admin_id)?,
                    &display_name,
                    ttl_days,
                )
                .await?;
            let mut out = json!({
                "tenant": admin.tenant, "project_id": project_id, "admin": admin.id,
                "token_id": tok.id, "expires_at": tok.expires_at,
            });
            match token_file {
                Some(p) => {
                    write_token_file(&p, tok.token.expose())?;
                    out["token_file"] = json!(p);
                }
                None => {
                    out["token"] = json!(tok.token.expose());
                    out["warning"] = json!("store this token securely now; it is shown only once");
                }
            }
            print(&out);
        }
        Cmd::Audit(AuditCmd::Verify) => {
            let h = harness(&cfg).await?;
            let r = h.db.verify_audit_chain().await?;
            print(&serde_json::to_value(&r)?);
            if !r.valid {
                return Err(HarnessError::integrity("audit chain verification FAILED"));
            }
        }
        Cmd::Audit(AuditCmd::List { after_seq, limit }) => {
            let h = harness(&cfg).await?;
            let p = caller_for(&h, Need::Action(Action::AuditRead, None)).await?;
            h.authz
                .authorize(&p, Action::AuditRead, Target::Tenant(p.tenant))?;
            print(&json!({"items": h.db.audit_records(Some(p.tenant), after_seq, limit.min(500)).await?}));
        }
        Cmd::Workflow(WorkflowCmd::Run {
            activity,
            project,
            yes,
            follow,
            no_worker,
        }) => {
            if !dx::workflow_run(&cfg, &activity, project.as_deref(), yes, !no_worker).await? {
                NOT_READY.store(true, std::sync::atomic::Ordering::SeqCst);
            } else if yes && follow {
                worker::watch(&cfg, None).await?;
            }
        }
        Cmd::Workflow(WorkflowCmd::Watch { id }) => worker::watch(&cfg, id.as_deref()).await?,
        Cmd::Worker(c) => match c {
            WorkerCmd::Start { project_root } => worker::worker_start(&cfg, project_root).await?,
            WorkerCmd::Stop { project_root } => worker::worker_stop(&cfg, project_root).await?,
            WorkerCmd::Status { project_root } => worker::worker_status(&cfg, project_root).await?,
            WorkerCmd::Run {
                project_root,
                until_idle,
            } => worker::worker_run(&cfg, project_root, until_idle).await?,
            WorkerCmd::Retry { task, workflow } => {
                worker::worker_retry(&cfg, &task, workflow.as_deref()).await?
            }
            WorkerCmd::History { task, limit } => {
                worker::worker_history(&cfg, task.as_deref(), limit).await?
            }
            WorkerCmd::Log { lines } => worker::worker_log(&cfg, lines).await?,
        },
        Cmd::Review {
            approval,
            decision,
            comments,
            out,
            open,
            no_show,
        } => {
            worker::review(
                &cfg,
                approval.as_deref(),
                decision.as_deref(),
                comments,
                out,
                open,
                no_show,
            )
            .await?
        }
        Cmd::Workflow(WorkflowCmd::Status { id, project }) => {
            dx::workflow_status(&cfg, id.as_deref(), project.as_deref()).await?;
        }
        Cmd::Workflow(WorkflowCmd::Resume { id }) => dx::workflow_resume(&cfg, &id).await?,
        Cmd::Workflow(WorkflowCmd::Restart {
            id,
            from,
            input,
            reason,
            yes,
        }) => {
            dx::workflow_restart(
                &cfg,
                &id,
                from.as_deref(),
                input.as_deref(),
                reason.as_deref(),
                yes,
            )
            .await?
        }
        Cmd::Workflow(c) => {
            let h = harness(&cfg).await?;
            let need = match &c {
                WorkflowCmd::Start { project, .. } => {
                    Need::Action(Action::WorkflowStart, Some(parse(project)?))
                }
                WorkflowCmd::Cancel { .. } => Need::Action(Action::WorkflowCancel, None),
                _ => Need::Any,
            };
            let p = caller_for(&h, need).await?;
            match c {
                WorkflowCmd::Run { .. }
                | WorkflowCmd::Status { .. }
                | WorkflowCmd::Resume { .. }
                | WorkflowCmd::Restart { .. }
                | WorkflowCmd::Watch { .. } => {
                    unreachable!("handled above")
                }
                WorkflowCmd::Start {
                    definition,
                    project,
                    input,
                } => {
                    let input: Value = serde_json::from_str(&input).map_err(|e| {
                        HarnessError::invalid_input(format!(
                            "--input must be a JSON object, e.g. '{{\"brd\":\"<source>/<subject>@v1\"}}': {e}"
                        ))
                    })?;
                    let scope = Scope::new(p.tenant, parse::<ProjectId>(&project)?);
                    h.identity.ensure_project(scope.tenant, scope.project).await?;
                    let id = h
                        .engine
                        .start(
                            &p,
                            &WorkflowDefinitionId::new(definition)?,
                            scope,
                            input,
                            TraceContext::new_root(),
                        )
                        .await?;
                    let status = h.engine.drive(id).await?;
                    print(&json!({"id": id, "status": status}));
                }
                WorkflowCmd::Show { id } => {
                    let id: WorkflowInstanceId = parse(&id)?;
                    let inst = h.engine.get_for(&p, id).await?;
                    print(&json!({"instance": inst, "history": h.engine.history(id).await?}));
                }
                WorkflowCmd::List { project, limit } => {
                    let scope = Scope::new(p.tenant, project_id(project.as_deref())?);
                    let items = h.engine.list(&p, scope, None, limit).await?;
                    print(
                        &json!({"items": items.iter().map(|i| json!({"id": i.id, "definition": i.definition_id, "state": i.state, "status": i.status})).collect::<Vec<_>>()}),
                    );
                }
                WorkflowCmd::Cancel { id } => {
                    let id: WorkflowInstanceId = parse(&id)?;
                    h.engine.get_for(&p, id).await?;
                    print(&json!({"id": id, "status": h.engine.cancel(&p, id).await?}));
                }
                WorkflowCmd::Definitions => {
                    h.authz
                        .authorize(&p, Action::WorkflowRead, Target::Tenant(p.tenant))?;
                    let defs = h.engine.definitions().await?;
                    print(
                        &json!({"items": defs.iter().map(|d| json!({"id": d.id, "version": d.version.to_string(), "description": d.description})).collect::<Vec<_>>()}),
                    );
                }
            }
        }
        Cmd::Approval(ApprovalCmd::Review {
            items,
            project,
            all,
            yes,
            reject,
            comments,
        }) => dx::approval_review(&cfg, project.as_deref(), all, yes, items, reject, comments).await?,
        Cmd::Approval(c) => {
            let h = harness(&cfg).await?;
            let need = if matches!(c, ApprovalCmd::Decide { .. }) {
                Need::Decision
            } else {
                Need::Any
            };
            let p = caller_for(&h, need).await?;
            match c {
                ApprovalCmd::Review { .. } => unreachable!("handled above"),
                ApprovalCmd::List { project, status } => {
                    let scope = Scope::new(p.tenant, project_id(project.as_deref())?);
                    h.authz
                        .authorize(&p, Action::ApprovalRead, Target::Scope(scope))?;
                    let status = status
                        .map(|s| {
                            serde_json::from_value::<ApprovalStatus>(json!(s))
                                .map_err(|_| HarnessError::invalid_input("invalid status"))
                        })
                        .transpose()?;
                    print(&json!({"items": h.approvals.list(scope, status, None, 100).await?}));
                }
                ApprovalCmd::Decide {
                    id,
                    decision,
                    comments,
                    artifact_hash,
                } => {
                    let decision = ApprovalDecision::parse(&decision)?;
                    let id: ApprovalId = parse(&id)?;
                    let rec = h.approvals.get(id).await?;
                    if rec.workflow_instance_id.is_some() {
                        let status = h
                            .engine
                            .decide_approval(
                                &p,
                                id,
                                decision,
                                &comments,
                                artifact_hash.as_deref(),
                                TraceContext::new_root(),
                            )
                            .await?;
                        // The run moved on; if it now waits for the AI client, make sure the
                        // background worker continues it (auto_start).
                        let worker = match dx::manifest_required(None) {
                            Ok((root, m)) if m.scope().project == rec.scope.project => {
                                worker::ensure_worker(&cfg, &h, &root, &m).await
                            }
                            _ => None,
                        };
                        print(
                            &json!({"approval": h.approvals.get(id).await?, "workflow_status": status, "worker": worker}),
                        );
                    } else {
                        h.approvals
                            .decide(
                                &p,
                                id,
                                decision,
                                &comments,
                                artifact_hash.as_deref(),
                                TraceContext::new_root(),
                            )
                            .await?;
                        // Governed subjects follow the decision (each ignores other types).
                        let doc = h.knowledge.apply_approval(id, TraceContext::new_root()).await?;
                        let skill = h.skills.apply_approval(id, TraceContext::new_root()).await?;
                        print(
                            &json!({"approval": h.approvals.get(id).await?, "knowledge_document": doc, "skill": skill}),
                        );
                    }
                }
            }
        }
        Cmd::Token(c) => {
            let h = harness(&cfg).await?;
            let need = match &c {
                TokenCmd::Issue {
                    principal: Some(_), ..
                } => Need::Action(Action::TokenManageAny, None),
                _ => Need::Any,
            };
            let p = caller_for(&h, need).await?;
            match c {
                TokenCmd::Issue {
                    principal,
                    label,
                    ttl_days,
                    token_file,
                    store_profile,
                    print: show,
                } => {
                    let target = principal
                        .map(PrincipalId::new)
                        .transpose()?
                        .unwrap_or_else(|| p.id.clone());
                    dx::issue_token_to(
                        &h,
                        &p,
                        &target,
                        &label,
                        ttl_days,
                        token_file.as_deref(),
                        store_profile.as_deref(),
                        show,
                    )
                    .await?;
                }
                TokenCmd::List { principal } => {
                    let target = principal
                        .map(PrincipalId::new)
                        .transpose()?
                        .unwrap_or_else(|| p.id.clone());
                    print(&json!({"items": h.identity.list_tokens(&p, &target).await?}));
                }
                TokenCmd::Revoke { id } => {
                    h.identity.revoke_token(&p, parse::<TokenId>(&id)?).await?;
                    print(&json!({"revoked": id}));
                }
                TokenCmd::Rotate {
                    id,
                    ttl_days,
                    token_file,
                    store_profile,
                    print: show,
                } => {
                    if token_file.is_none() && store_profile.is_none() && !show {
                        return Err(HarnessError::invalid_input(
                            "choose where the new token goes: --token-file <file>, --store-profile <name> or --print (avoid); for your own stored profile use `eaeh auth renew --profile <name>`",
                        ));
                    }
                    let t = h
                        .identity
                        .rotate_token(&p, parse::<TokenId>(&id)?, ttl_days)
                        .await?;
                    let mut res = json!({"id": t.id, "expires_at": t.expires_at, "replaces": id});
                    if let Some(f) = &token_file {
                        write_token_file(f, t.token.expose())?;
                        res["token_file"] = json!(f);
                    }
                    if let Some(name) = &store_profile {
                        dx::store_rotated(&h, name, t.token.expose()).await?;
                        res["stored_profile"] = json!(name);
                    }
                    if show {
                        res["token"] = json!(t.token.expose());
                    }
                    print(&res);
                }
            }
        }
        Cmd::Principal(PrincipalCmd::Grant { id, project }) => {
            dx::principal_grant(&cfg, &id, project.as_deref()).await?
        }
        Cmd::Principal(c) => {
            let h = harness(&cfg).await?;
            let p = caller_for(&h, Need::Action(Action::PrincipalManage, None)).await?;
            match c {
                PrincipalCmd::Grant { .. } => unreachable!("handled above"),
                PrincipalCmd::Create {
                    id,
                    display_name,
                    roles,
                    projects,
                    kind,
                } => {
                    let roles = roles
                        .split(',')
                        .filter(|r| !r.is_empty())
                        .map(|r| {
                            serde_json::from_value::<Role>(json!(r.trim()))
                                .map_err(|_| HarnessError::invalid_input(format!("unknown role {r}")))
                        })
                        .collect::<Result<_>>()?;
                    let projects = if projects == "all" {
                        ProjectAccess::All
                    } else {
                        ProjectAccess::Only(
                            projects
                                .split(',')
                                .map(parse::<ProjectId>)
                                .collect::<Result<_>>()?,
                        )
                    };
                    let kind = match kind.as_str() {
                        "human" => PrincipalKind::Human,
                        "service" => PrincipalKind::Service,
                        _ => return Err(HarnessError::invalid_input("kind must be human or service")),
                    };
                    let created = h
                        .identity
                        .create_principal(
                            &p,
                            NewPrincipal {
                                id: PrincipalId::new(id)?,
                                display_name,
                                kind,
                                roles,
                                projects,
                            },
                        )
                        .await?;
                    print(&json!({"id": created.id, "roles": created.roles(), "projects": created.projects}));
                }
                PrincipalCmd::SetTeams { id, teams } => {
                    let teams: Vec<String> = teams
                        .split(',')
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(String::from)
                        .collect();
                    h.identity
                        .set_principal_teams(&p, &PrincipalId::new(id.clone())?, &teams)
                        .await?;
                    print(&json!({"principal": id, "teams": teams}));
                }
                PrincipalCmd::Disable { id } => {
                    h.identity
                        .set_principal_status(&p, &PrincipalId::new(id.clone())?, false)
                        .await?;
                    print(&json!({"disabled": id}));
                }
            }
        }
        Cmd::Project(ProjectCmd::Init {
            name,
            description,
            language,
            framework,
            build_tool,
            constraints,
            brd,
            knowledge,
            standards,
            developer,
            steward,
            approvers,
            mode,
            provider,
            clients,
            build_verification,
            build_program,
            build_args,
            workflows,
            store_profiles,
            handover_dir,
            workspace,
            ttl_days,
        }) => {
            let ready = dx::project_init(
                &cfg,
                dx::InitArgs {
                    name,
                    description,
                    language,
                    framework,
                    build_tool,
                    constraints,
                    brd,
                    knowledge,
                    standards,
                    developer,
                    steward,
                    approvers,
                    mode,
                    provider,
                    clients,
                    build_verification,
                    build_program,
                    build_args,
                    workflows,
                    store_profiles,
                    handover_dir,
                    workspace,
                    ttl_days,
                    config_path: cli.config.clone(),
                },
            )
            .await?;
            if !ready {
                NOT_READY.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        Cmd::Project(ProjectCmd::Readiness { project, workflow }) => {
            if !dx::readiness_cmd(&cfg, project.as_deref(), workflow).await? {
                NOT_READY.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        Cmd::Project(c) => {
            let h = harness(&cfg).await?;
            let need = match &c {
                ProjectCmd::List => Need::Any,
                _ => Need::Action(Action::PrincipalManage, None),
            };
            let p = caller_for(&h, need).await?;
            match c {
                ProjectCmd::Init { .. } | ProjectCmd::Readiness { .. } => unreachable!("handled above"),
                ProjectCmd::Create { name } => {
                    let id = h.identity.create_project(&p, p.tenant, &name).await?;
                    print(&json!({"id": id, "name": name}));
                }
                ProjectCmd::SetDomain { id, domain } => {
                    let d = if domain == "none" {
                        None
                    } else {
                        Some(domain.as_str())
                    };
                    h.identity
                        .set_project_domain(&p, parse::<ProjectId>(&id)?, d)
                        .await?;
                    print(&json!({"project": id, "domain": d}));
                }
                ProjectCmd::List => {
                    let items: Vec<Value> = h
                        .identity
                        .projects(p.tenant)
                        .await?
                        .into_iter()
                        .filter(|(id, _)| p.can_access(&Scope::new(p.tenant, *id)))
                        .map(|(id, n)| json!({"id": id, "name": n}))
                        .collect();
                    // Which installation answered: the portal shows the same projects only
                    // when it runs on the same data directory.
                    print(
                        &json!({"items": items, "installation": {"data_dir": dx::data_dir(&h), "config_dir": cfg.base_dir}}),
                    );
                }
            }
        }
        Cmd::Plugin(c) => {
            let h = harness(&cfg).await?;
            let p = caller(&h).await?;
            let v =
                |s: &str| semver::Version::parse(s).map_err(|e| HarnessError::invalid_input(e.to_string()));
            let rec = match c {
                PluginCmd::List => {
                    h.authz.authorize(&p, Action::PluginManage, Target::Platform)?;
                    print(&json!({"items": h.plugins.list().await?}));
                    return Ok(());
                }
                PluginCmd::Enable { id, version } => {
                    h.plugins
                        .enable(&p, &eaeh_core::PluginId::new(id)?, &v(&version)?)
                        .await?
                }
                PluginCmd::Disable { id, version } => {
                    h.plugins
                        .disable(&p, &eaeh_core::PluginId::new(id)?, &v(&version)?)
                        .await?
                }
                PluginCmd::Retire { id, version } => {
                    h.plugins
                        .retire(&p, &eaeh_core::PluginId::new(id)?, &v(&version)?)
                        .await?
                }
            };
            print(&json!({"plugin": rec, "note": "takes effect on next start"}));
        }
        Cmd::Knowledge(c) => {
            let h = harness(&cfg).await?;
            let need = match &c {
                KnowledgeCmd::SourceCreate { .. }
                | KnowledgeCmd::Ingest { .. }
                | KnowledgeCmd::Import { .. }
                | KnowledgeCmd::Retire { .. }
                | KnowledgeCmd::Delete { .. } => Need::Action(Action::KnowledgeManage, None),
                _ => Need::Any,
            };
            let p = caller_for(&h, need).await?;
            run_knowledge(&h, &p, c).await?;
        }
        Cmd::Skill(SkillCmd::Sync { project }) => dx::skill_sync(&cfg, project.as_deref()).await?,
        Cmd::Skill(SkillCmd::Pending { project }) => dx::skill_pending(&cfg, project.as_deref()).await?,
        Cmd::Skill(c) => {
            let h = harness(&cfg).await?;
            let need = match &c {
                SkillCmd::List => Need::Any,
                _ => Need::Action(Action::SkillManage, None),
            };
            let p = caller_for(&h, need).await?;
            match c {
                SkillCmd::Sync { .. } | SkillCmd::Pending { .. } => unreachable!("handled above"),
                SkillCmd::Register { file, project } => {
                    // JSON or Markdown (`.md`, TOML front matter + instructions body).
                    let def = eaeh_skills::SkillDefinition::from_file_text(
                        &file,
                        &std::fs::read_to_string(&file)?,
                    )?;
                    let rec = h
                        .skills
                        .register(&p, &def, parse::<ProjectId>(&project)?, TraceContext::new_root())
                        .await?;
                    print(&serde_json::to_value(rec)?);
                }
                SkillCmd::List => print(&json!({"items": h.skills.list(&p).await?})),
                SkillCmd::SetStatus { id, version, status } => {
                    let to = match status.as_str() {
                        "approved" => eaeh_skills::SkillStatus::Approved,
                        "deprecated" => eaeh_skills::SkillStatus::Deprecated,
                        "retired" => eaeh_skills::SkillStatus::Retired,
                        _ => {
                            return Err(HarnessError::invalid_input(
                                "status must be approved, deprecated or retired",
                            ));
                        }
                    };
                    let v = semver::Version::parse(&version)
                        .map_err(|e| HarnessError::invalid_input(e.to_string()))?;
                    print(&serde_json::to_value(
                        h.skills
                            .set_status(&p, &eaeh_core::SkillId::new(id)?, &v, to)
                            .await?,
                    )?);
                }
            }
        }
        Cmd::Auth(c) => match c {
            AuthCmd::Login {
                profile,
                token_file,
                default,
            } => dx::auth_login(&cfg, &profile, token_file.as_deref(), default).await?,
            AuthCmd::Status => dx::auth_status(&cfg).await?,
            AuthCmd::Use { profile } => dx::auth_use(&profile)?,
            AuthCmd::Renew { profile, ttl_days } => dx::auth_renew(&cfg, &profile, ttl_days).await?,
            AuthCmd::Logout { profile, revoke } => dx::auth_logout(&cfg, &profile, revoke).await?,
        },
        Cmd::Mcp {
            project_root,
            task,
            execution,
            task_tools,
        } => {
            if task_tools {
                println!(
                    "{}",
                    serde_json::to_string(&eaeh_runtime::coordinator::TASK_MCP_TOOLS)?
                );
            } else {
                mcp::serve(&cfg, project_root, task, execution).await?
            }
        }
        Cmd::Task(TaskCmd::List { project, all }) => dx::task_list(&cfg, project.as_deref(), all).await?,
        Cmd::Task(TaskCmd::Show { id, out }) => dx::task_show(&cfg, &id, out.as_deref()).await?,
        Cmd::Task(TaskCmd::Tool { id, tool, input }) => dx::task_tool(&cfg, &id, &tool, &input).await?,
        Cmd::Task(TaskCmd::Validate { id, output }) => dx::task_validate(&cfg, &id, &output).await?,
        Cmd::Task(TaskCmd::Submit { id, output, client }) => {
            dx::task_submit(&cfg, &id, &output, &client).await?
        }
        Cmd::Agents => {
            let h = harness(&cfg).await?;
            let p = caller(&h).await?;
            h.authz
                .authorize(&p, Action::AgentRead, Target::Tenant(p.tenant))?;
            let items: Vec<Value> = h
                .agent_registry
                .list()
                .await?
                .into_iter()
                .map(|(d, s)| json!({"id": d.id, "version": d.version.to_string(), "status": s, "capabilities": d.capabilities, "autonomy": d.autonomy, "tools": d.allowed_tools}))
                .collect();
            print(&json!({"items": items}));
        }
        Cmd::Brd(BrdCmd::Ingest {
            file,
            project,
            source,
            subject,
            classification,
        }) => {
            let h = harness(&cfg).await?;
            let (pid, manifest) = dx::project_context(project.as_deref())?;
            let project = pid.to_string();
            let p = caller_for(&h, Need::Action(Action::KnowledgeManage, Some(pid))).await?;
            let t = TraceContext::new_root();
            if !h.knowledge.sources(&p).await?.iter().any(|s| s.name == source) {
                h.knowledge
                    .register_source(
                        &p,
                        &source,
                        eaeh_knowledge::SourceKind::Reference,
                        "Business requirements documents",
                    )
                    .await?;
            }
            let fmt = format_of(&file)?;
            if !matches!(
                fmt,
                eaeh_knowledge::Format::Markdown | eaeh_knowledge::Format::Text
            ) {
                return Err(HarnessError::invalid_input(
                    "BRDs must be Markdown (.md) or text (.txt)",
                ));
            }
            let mut req = ingest_request(
                &file,
                &source,
                subject,
                None,
                &format!("project:{project}"),
                &classification,
                false,
                None,
                &project,
            )?;
            req.tags = vec!["brd".into()];
            let doc = h.knowledge.ingest(&p, req, t).await?;
            let citation = doc.citation_base();
            // Associate the BRD with the repository's project manifest.
            let mut associated = None;
            if let Some((root, mut m)) = manifest {
                let abs = std::fs::canonicalize(&file).unwrap_or(file.clone());
                let rel = abs.strip_prefix(&root).map(Path::to_path_buf).unwrap_or(abs);
                m.brd = Some(eaeh_runtime::project::BrdSection {
                    path: rel.display().to_string(),
                    citation: citation.clone(),
                });
                associated = Some(m.save(&root)?);
            }
            print(&json!({"document": doc, "citation": citation, "manifest_updated": associated}));
        }
        Cmd::Trace { project, id, depth } => {
            let h = harness(&cfg).await?;
            let p = caller(&h).await?;
            let scope = Scope::new(p.tenant, project_id(project.as_deref())?);
            let items = h.engine.trace(&p, scope, id.as_deref(), depth).await?;
            print(&json!({"items": items}));
        }
        Cmd::Artifact(ArtifactCmd::Status { id, project }) => {
            dx::artifact_status(&cfg, id.as_deref(), project.as_deref()).await?;
        }
        Cmd::Artifact(ArtifactCmd::Sync {
            id,
            project,
            replace_modified,
        }) => {
            if !dx::artifact_sync(&cfg, id.as_deref(), project.as_deref(), replace_modified).await? {
                std::process::exit(8);
            }
        }
        Cmd::Artifact(ArtifactCmd::Export { project, id, out }) => {
            let h = harness(&cfg).await?;
            let p = caller(&h).await?;
            let scope = Scope::new(p.tenant, project_id(project.as_deref())?);
            h.authz
                .authorize(&p, Action::ArtifactRead, Target::Scope(scope))?;
            let (meta, written) = export_artifact(&h, scope, parse(&id)?, &out).await?;
            // Was exactly this version approved at a workflow gate? (Exports also serve
            // pre-approval review, so this informs rather than blocks.)
            let approval = h
                .approvals
                .list(scope, Some(eaeh_governance::ApprovalStatus::Approved), None, 1000)
                .await?
                .into_iter()
                .find(|a| {
                    a.artifact_id.as_deref() == Some(meta.id.to_string().as_str())
                        && a.artifact_hash.as_deref() == Some(meta.content_hash.as_str())
                });
            if approval.is_none() {
                eprintln!(
                    "eaeh: note: this artifact version has NOT been approved at a workflow gate; use it for review only"
                );
            }
            print(&json!({
                "artifact": meta.name, "version": meta.version, "hash": meta.content_hash, "written": written,
                "approved": approval.as_ref().map(|a| json!({"approval": a.id, "gate": a.gate, "decided_at": a.decided_at})),
            }));
        }
        Cmd::Tools => {
            let h = harness(&cfg).await?;
            let p = caller(&h).await?;
            h.authz
                .authorize(&p, Action::ToolRead, Target::Tenant(p.tenant))?;
            let items: Vec<Value> = h
                .tools
                .list()
                .into_iter()
                .map(|d| json!({"id": d.id, "version": d.version.to_string(), "risk": d.risk, "needs_execution": d.needs_execution, "isolation": d.isolation}))
                .collect();
            print(&json!({"items": items}));
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let (format, filter) = HarnessConfig::load(&cli.config)
        .map(|c| (c.log.format, c.log.filter))
        .unwrap_or_else(|_| ("pretty".into(), "warn".into()));
    // CLI commands log warnings and above unless serving.
    let filter = if matches!(cli.cmd, Cmd::Serve | Cmd::Portal { .. }) {
        filter
    } else {
        "warn".into()
    };
    init_tracing(
        &format,
        &filter,
        REDACTOR.get_or_init(eaeh_core::Redactor::new).clone(),
    );
    let _ = dx::AS_PROFILE.set(cli.as_profile.clone());
    let _ = worker::CONFIG_PATH.set(cli.config.clone());
    let _ = dx::JSON.set(cli.json);
    if let Err(e) = run(cli).await {
        let mut err = json!({"code": e.kind.as_str(), "message": e.message});
        if let Some(h) = hint_for(&e) {
            err["hint"] = json!(h);
        }
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&json!({"error": err})).unwrap_or_default()
        );
        std::process::exit(match e.kind {
            eaeh_core::ErrorKind::Unauthenticated => 3,
            eaeh_core::ErrorKind::Forbidden | eaeh_core::ErrorKind::PolicyDenied => 4,
            eaeh_core::ErrorKind::NotFound => 5,
            eaeh_core::ErrorKind::Integrity => 6,
            _ => 1,
        });
    }
    if NOT_READY.load(std::sync::atomic::Ordering::SeqCst) {
        std::process::exit(7);
    }
}

fn parse_scope(s: &str) -> Result<eaeh_knowledge::KnowledgeScope> {
    use eaeh_knowledge::KnowledgeScope;
    Ok(match s.split_once(':') {
        None if s == "enterprise" => KnowledgeScope::Enterprise,
        Some(("domain", d)) => KnowledgeScope::Domain(d.into()),
        Some(("team", t)) => KnowledgeScope::Team(t.into()),
        Some(("project", p)) => KnowledgeScope::Project(p.parse()?),
        Some(("workflow", w)) => KnowledgeScope::Workflow(w.parse()?),
        _ => {
            return Err(HarnessError::invalid_input(
                "scope must be enterprise | domain:<n> | project:<id> | team:<n> | workflow:<id>",
            ));
        }
    })
}

fn format_of(path: &Path) -> Result<eaeh_knowledge::Format> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("md") | Some("markdown") => Ok(eaeh_knowledge::Format::Markdown),
        Some("txt") => Ok(eaeh_knowledge::Format::Text),
        Some("json") => Ok(eaeh_knowledge::Format::Json),
        _ => Err(HarnessError::invalid_input(format!(
            "unsupported file type: {}",
            path.display()
        ))),
    }
}

fn title_of(content: &str, fallback: &str) -> String {
    content
        .lines()
        .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
        .or_else(|| {
            serde_json::from_str::<Value>(content)
                .ok()
                .and_then(|v| v.pointer("/info/title").and_then(Value::as_str).map(String::from))
        })
        .unwrap_or_else(|| fallback.to_string())
}

#[allow(clippy::too_many_arguments)]
fn ingest_request(
    file: &Path,
    source: &str,
    subject: Option<String>,
    title: Option<String>,
    scope: &str,
    classification: &str,
    mandatory: bool,
    review_by: Option<String>,
    project: &str,
) -> Result<eaeh_knowledge::IngestRequest> {
    let content = std::fs::read_to_string(file)?;
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document")
        .to_lowercase();
    Ok(eaeh_knowledge::IngestRequest {
        source: source.into(),
        subject: subject.unwrap_or(stem.clone()),
        title: title.unwrap_or_else(|| title_of(&content, &stem)),
        format: format_of(file)?,
        content,
        scope: parse_scope(scope)?,
        classification: eaeh_core::Classification::parse(classification)?,
        tags: vec![],
        mandatory,
        review_by,
        governance_project: parse(project)?,
    })
}

async fn run_knowledge(h: &Harness, p: &Principal, c: KnowledgeCmd) -> Result<()> {
    let kb = &h.knowledge;
    let t = TraceContext::new_root();
    match c {
        KnowledgeCmd::SourceCreate {
            name,
            kind,
            description,
        } => {
            let kind: eaeh_knowledge::SourceKind = serde_json::from_value(json!(kind))
                .map_err(|_| HarnessError::invalid_input("invalid source kind"))?;
            print(&serde_json::to_value(
                kb.register_source(p, &name, kind, &description).await?,
            )?);
        }
        KnowledgeCmd::Sources => print(&json!({"items": kb.sources(p).await?})),
        KnowledgeCmd::Ingest {
            file,
            source,
            subject,
            title,
            scope,
            classification,
            mandatory,
            review_by,
            project,
        } => {
            let req = ingest_request(
                &file,
                &source,
                subject,
                title,
                &scope,
                &classification,
                mandatory,
                review_by,
                &project,
            )?;
            print(&serde_json::to_value(kb.ingest(p, req, t).await?)?);
        }
        KnowledgeCmd::Import {
            dir,
            source,
            scope,
            classification,
            mandatory,
            project,
        } => {
            let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| format_of(p).is_ok())
                .collect();
            files.sort();
            let mut out = vec![];
            for f in files {
                let req = ingest_request(
                    &f,
                    &source,
                    None,
                    None,
                    &scope,
                    &classification,
                    mandatory,
                    None,
                    &project,
                )?;
                let d = kb.ingest(p, req, t).await?;
                out.push(json!({"file": f, "id": d.id, "citation": d.citation_base(), "status": d.status, "approval_id": d.approval_id}));
            }
            print(&json!({"imported": out}));
        }
        KnowledgeCmd::List { status } => {
            let status = status
                .as_deref()
                .map(eaeh_knowledge::DocStatus::parse)
                .transpose()?;
            print(&json!({"items": kb.list(p, status, 200).await?}));
        }
        KnowledgeCmd::Search {
            project,
            query,
            mode,
            k,
        } => {
            let mode: eaeh_knowledge::SearchMode = serde_json::from_value(json!(mode))
                .map_err(|_| HarnessError::invalid_input("mode must be lexical|vector|hybrid"))?;
            let ctx = kb.context_for_human(p, parse(&project)?).await?;
            let hits = kb
                .search(&ctx, &eaeh_knowledge::SearchQuery::new(&query, mode, k), t)
                .await?;
            print(
                &json!({"items": hits.iter().map(|h| json!({"citation": h.citation, "title": h.title, "heading": h.heading, "score": h.score, "stale": h.stale, "conflict": h.conflict, "text": h.text})).collect::<Vec<_>>()}),
            );
        }
        KnowledgeCmd::Retire { id } => print(&serde_json::to_value(kb.retire(p, &id).await?)?),
        KnowledgeCmd::Delete { id } => print(&serde_json::to_value(kb.delete(p, &id).await?)?),
        KnowledgeCmd::Conflicts => print(&json!({"items": kb.conflicts(p).await?})),
    }
    Ok(())
}
