//! Automated external AI execution from the CLI (ADR-0026): the background worker that runs
//! hand-offs through Claude Code, its control commands, the live progress view
//! (`workflow watch`) and the document review command (`eaeh review`).

use crate::credentials::{Need, Store, select};
use crate::dx;
use crate::harness;
use eaeh_core::{ApprovalId, HarnessError, Principal, Result, Scope, WorkflowInstanceId};
use eaeh_governance::authz::Action;
use eaeh_governance::{ApprovalDecision, ApprovalStatus};
use eaeh_runtime::coordinator::{
    self as co, CancellationToken, Coordinator, DesktopNotifier, McpLaunch, NoNotifier, Notifier, Until,
};
use eaeh_runtime::project::{self as proj, ProjectManifest};
use eaeh_runtime::{ExecutionMode, Harness, HarnessConfig};
use serde_json::json;
use std::collections::BTreeSet;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Absolute path of the configuration file in use (set by `main`).
pub static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();

fn config_path() -> Result<PathBuf> {
    let p = CONFIG_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("config/eaeh.toml"));
    std::fs::canonicalize(&p)
        .map_err(|e| HarnessError::invalid_input(format!("configuration file {}: {e}", p.display())))
}

fn root_and_manifest(project_root: Option<PathBuf>) -> Result<(PathBuf, ProjectManifest)> {
    match project_root {
        Some(r) => {
            let root = proj::find_root(&r).ok_or_else(|| {
                HarnessError::invalid_input(format!(
                    "{} is not inside a bootstrapped repository",
                    r.display()
                ))
            })?;
            let m = ProjectManifest::load(&root)?;
            Ok((root, m))
        }
        None => dx::manifest_required(None),
    }
}

/// The stored *operator* profile the worker runs as (never an approver-only identity, and
/// never `EAEH_TOKEN`: the worker passes only the profile *name* to its client sessions).
async fn operator_profile(h: &Harness, explicit: Option<&str>, scope: Scope) -> Result<(String, Principal)> {
    let store = Store::load()?;
    let dd = dx::data_dir(h);
    let Some((name, _)) = select(
        &store,
        &dd,
        explicit,
        Need::Action(Action::TaskExecuteExternal, Some(scope.project)),
    )?
    else {
        return Err(HarnessError::unauthenticated(
            "the worker runs as a stored operator (developer) profile: store one with `eaeh auth login --profile developer --token-file <file>` (EAEH_TOKEN is not used by the worker)",
        ));
    };
    let prof = &store.profiles[&name];
    let p = h.identity.authenticate(prof.token.trim()).await.map_err(|_| {
        HarnessError::unauthenticated(format!(
            "the token of profile '{name}' is invalid, expired or revoked; renew it (`eaeh auth renew --profile {name}`) or log in again"
        ))
    })?;
    Ok((name, p))
}

fn log_path(h: &Harness, scope: Scope) -> PathBuf {
    h.config
        .data_dir()
        .join("workers")
        .join(format!("{}.log", scope.project))
}

fn stamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

// ------------------------------------------------------------------ worker run

/// `eaeh worker run`: the coordinator loop in the foreground (the background worker is this
/// command started by `eaeh worker start`).
pub async fn worker_run(cfg: &HarnessConfig, project_root: Option<PathBuf>, until_idle: bool) -> Result<()> {
    let (root, m) = root_and_manifest(project_root)?;
    let h = harness(cfg).await?;
    let explicit = dx::AS_PROFILE.get().cloned().flatten();
    let (profile, p) = operator_profile(&h, explicit.as_deref(), m.scope()).await?;
    let exe = std::env::current_exe()?;
    let mcp = McpLaunch {
        program: exe,
        args: vec![
            "--config".into(),
            config_path()?.display().to_string(),
            "mcp".into(),
            "--task".into(),
            "{task}".into(),
            "--execution".into(),
            "{execution}".into(),
            "--project-root".into(),
            root.display().to_string(),
        ],
        // Non-secret only: the profile *name* and, when set, where the profile store lives.
        env: std::iter::once(("EAEH_PROFILE".to_string(), profile.clone()))
            .chain(
                std::env::var("EAEH_HOME")
                    .ok()
                    .map(|v| ("EAEH_HOME".to_string(), v)),
            )
            .collect(),
    };
    let ccfg = &cfg.execution.coordinator;
    let executor = co::executor_from_config(ccfg)?;
    let notifier: Arc<dyn Notifier> = if ccfg.notify {
        Arc::new(DesktopNotifier)
    } else {
        Arc::new(NoNotifier)
    };
    let progress: co::ProgressFn = Arc::new(|l: &str| {
        println!("{} {l}", stamp());
        let _ = std::io::stdout().flush();
    });
    let c = Coordinator::register(h.clone(), p.clone(), m.scope(), executor, mcp, notifier, progress).await?;
    let pf = c.preflight();
    println!(
        "{} worker {} started for project '{}' as '{}' (profile {profile}); provider {} – {}; execution mode {}",
        stamp(),
        c.worker_id(),
        m.project.name,
        p.id,
        cfg.execution.coordinator.provider,
        pf.as_ref().map(|x| x.detail.clone()).unwrap_or_default(),
        h.execution_mode
    );
    let withheld = co::claude::ClaudeCodeCli::billing_env_present();
    if !withheld.is_empty() {
        println!(
            "{} note: {} is set in this environment and is withheld from Claude Code (subscription login only)",
            stamp(),
            withheld.join(", ")
        );
    }
    let stop = CancellationToken::new();
    let s2 = stop.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            if let Ok(mut term) = signal(SignalKind::terminate()) {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            } else {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
        s2.cancel();
    });
    let summary = c
        .run(stop, if until_idle { Until::Idle } else { Until::Stopped })
        .await?;
    println!(
        "{} worker {} stopped: {} ({} attempts, {} accepted, {} not accepted)",
        stamp(),
        c.worker_id(),
        summary.stopped_because,
        summary.attempts,
        summary.succeeded,
        summary.not_accepted
    );
    Ok(())
}

// ------------------------------------------------------------------ start / ensure

/// Start the background worker for the project at `root`. Returns a one-line message.
async fn spawn_worker(h: &Harness, root: &Path, scope: Scope, profile: &str) -> Result<String> {
    let log = log_path(h, scope);
    if let Some(d) = log.parent() {
        std::fs::create_dir_all(d)?;
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&log)?;
    let started_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.args([
        "--config",
        &config_path()?.display().to_string(),
        "--as",
        profile,
        "worker",
        "run",
        "--project-root",
        &root.display().to_string(),
    ])
    .env_remove("EAEH_TOKEN")
    .stdin(std::process::Stdio::null())
    .stdout(file.try_clone()?)
    .stderr(file);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let pid = child.id();
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Ok(Some(status)) = child.try_wait() {
            let tail = std::fs::read_to_string(&log)
                .map(|s| {
                    s.get(usize::try_from(started_len).unwrap_or(0)..)
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();
            return Err(HarnessError::unavailable(format!(
                "the worker did not start ({status}): {}",
                if tail.is_empty() {
                    "see the worker log"
                } else {
                    &tail
                }
            )));
        }
        if co::local_dispatch_state(h, scope).await?.0.is_some() {
            return Ok(format!(
                "Background worker started (pid {pid}); log: {}",
                log.display()
            ));
        }
    }
    Ok(format!(
        "Background worker launched (pid {pid}) but has not reported yet; check `eaeh worker status` and {}",
        log.display()
    ))
}

/// After a run starts or a gate is decided: if hand-offs wait and no worker serves the
/// project, start one (when `auto_start` is on). Never fails the calling command.
pub async fn ensure_worker(
    cfg: &HarnessConfig,
    h: &Harness,
    root: &Path,
    m: &ProjectManifest,
) -> Option<String> {
    if h.execution_mode != ExecutionMode::ExternalAi {
        return None;
    }
    let scope = m.scope();
    let (live, waiting) = co::local_dispatch_state(h, scope).await.ok()?;
    if waiting == 0 {
        return None;
    }
    if let Some((id, pid)) = live {
        return Some(format!(
            "The background worker ({id}, pid {pid}) is executing the AI tasks."
        ));
    }
    if !cfg.execution.coordinator.auto_start {
        return Some("AI tasks are waiting. Start the worker: eaeh worker start".into());
    }
    let profile = match Store::load().ok().and_then(|s| {
        select(
            &s,
            &dx::data_dir(h),
            None,
            Need::Action(Action::TaskExecuteExternal, Some(scope.project)),
        )
        .ok()
        .flatten()
    }) {
        Some((n, _)) => n,
        None => {
            return Some(
                "AI tasks are waiting, but no stored developer profile can run the worker: `eaeh auth login --profile developer --token-file <file>`, then `eaeh worker start`".into(),
            );
        }
    };
    Some(match spawn_worker(h, root, scope, &profile).await {
        Ok(msg) => format!(
            "{msg}\nYou will be notified when a document needs your review (`eaeh workflow watch` shows progress)."
        ),
        Err(e) => format!(
            "Could not start the worker: {}. Fix it, then run `eaeh worker start`.",
            e.message
        ),
    })
}

/// `eaeh worker start`.
pub async fn worker_start(cfg: &HarnessConfig, project_root: Option<PathBuf>) -> Result<()> {
    let (root, m) = root_and_manifest(project_root)?;
    let h = harness(cfg).await?;
    if h.execution_mode != ExecutionMode::ExternalAi {
        return Err(HarnessError::invalid_input(format!(
            "the worker runs only in external_ai mode (this installation is {})",
            h.execution_mode
        )));
    }
    co::validate_config(&cfg.execution.coordinator)?;
    let scope = m.scope();
    let explicit = dx::AS_PROFILE.get().cloned().flatten();
    let (profile, p) = operator_profile(&h, explicit.as_deref(), scope).await?;
    if let Some(w) = co::live_worker(&h, &p, scope).await? {
        dx::out(
            &json!({"started": false, "worker": w}),
            &format!(
                "A worker is already running ({}, pid {} on {}).",
                w.id, w.pid, w.host
            ),
        );
        return Ok(());
    }
    let msg = spawn_worker(&h, &root, scope, &profile).await?;
    dx::out(&json!({"started": true, "message": msg}), &msg);
    Ok(())
}

/// `eaeh worker stop`.
pub async fn worker_stop(cfg: &HarnessConfig, project_root: Option<PathBuf>) -> Result<()> {
    let (_, m) = root_and_manifest(project_root)?;
    let h = harness(cfg).await?;
    let p = dx::caller_need(&h, Need::Action(Action::TaskExecuteExternal, Some(m.project.id))).await?;
    let n = co::request_stop(&h, &p, m.scope()).await?;
    for _ in 0..40 {
        if co::live_worker(&h, &p, m.scope()).await?.is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let still = co::live_worker(&h, &p, m.scope()).await?;
    dx::out(
        &json!({"stop_requested": n, "still_running": still}),
        &match (n, &still) {
            (0, _) => "No worker was running.".to_string(),
            (_, None) => "Worker stopped.".to_string(),
            (_, Some(w)) => format!(
                "Stop requested; worker {} is finishing its current step (it stops the AI client within seconds).",
                w.id
            ),
        },
    );
    Ok(())
}

/// `eaeh worker status`: execution mode, provider and authentication, workers, open tasks.
pub async fn worker_status(cfg: &HarnessConfig, project_root: Option<PathBuf>) -> Result<()> {
    let (_, m) = root_and_manifest(project_root)?;
    let h = harness(cfg).await?;
    let p = dx::caller_need(&h, Need::Any).await?;
    let scope = m.scope();
    let workers = co::workers(&h, &p, scope).await?;
    let tasks = h.agents.external_tasks(&p, scope, true).await?;
    let pf = match co::executor_from_config(&cfg.execution.coordinator) {
        Ok(e) => Some(e.preflight().await),
        Err(_) => None,
    };
    let withheld = co::claude::ClaudeCodeCli::billing_env_present();
    let mut t = format!(
        "Execution mode: {}\n  {}\nProvider: {}\n",
        h.execution_mode,
        h.execution_mode.billing(),
        cfg.execution.coordinator.provider
    );
    match &pf {
        Some(pf) => {
            t.push_str(&format!(
                "  client: {} {}\n  authentication: {}\n",
                pf.program.clone().unwrap_or_else(|| "not found".into()),
                pf.version.clone().unwrap_or_default(),
                pf.auth.summary()
            ));
            if let Err((c, d)) = pf.usable() {
                t.push_str(&format!(
                    "  NOT USABLE [{}]: {d}\n  fix: {}\n",
                    c.as_str(),
                    c.remedy()
                ));
            }
        }
        None => t.push_str("  provider not supported (see `[execution.coordinator] provider`)\n"),
    }
    if !withheld.is_empty() {
        t.push_str(&format!(
            "  note: {} set in your environment – withheld from Claude Code; only the subscription login is used\n",
            withheld.join(", ")
        ));
    }
    match workers.iter().find(|w| w.live) {
        Some(w) => t.push_str(&format!(
            "Worker: running ({}, pid {} on {}, as {}, last heartbeat {})\n",
            w.id, w.pid, w.host, w.principal, w.heartbeat_at
        )),
        None => t.push_str(&format!(
            "Worker: not running{}\n",
            workers
                .first()
                .map(|w| format!(
                    " (last: {} {} – {})",
                    w.id,
                    w.status,
                    w.detail.clone().unwrap_or_default()
                ))
                .unwrap_or_default()
        )),
    }
    if tasks.is_empty() {
        t.push_str("AI tasks waiting: none\n");
    } else {
        t.push_str("AI tasks waiting:\n");
        for x in &tasks {
            t.push_str(&format!(
                "  {} {} ({}) – {} attempt(s) {}{}\n",
                x.task_id,
                x.state,
                x.agent,
                x.dispatch_attempts,
                x.dispatch_state,
                x.last_failure
                    .as_ref()
                    .map(|f| format!(": {f}"))
                    .unwrap_or_default()
            ));
        }
    }
    dx::out(
        &json!({"execution_mode": h.execution_mode.as_str(), "billing": h.execution_mode.billing(),
                "provider": cfg.execution.coordinator.provider, "client": pf.as_ref().map(co::preflight_json),
                "billing_env_withheld": withheld, "workers": workers, "tasks": tasks}),
        t.trim_end(),
    );
    Ok(())
}

/// `eaeh worker retry <task>`.
pub async fn worker_retry(cfg: &HarnessConfig, task: &str, workflow: Option<&str>) -> Result<()> {
    let h = harness(cfg).await?;
    let p = dx::caller_need(&h, Need::Any).await?;
    let r = co::retry_task_checked(&h, &p, task.parse()?, workflow).await?;
    let mut msg = format!(
        "Task {task} will be executed again.\n  workflow {} (now {} at '{}'), stage {}\n  package version {} (unchanged), {} submission(s) kept\n",
        r.workflow, r.workflow_status, r.workflow_stage, r.stage, r.package_hash, r.submissions
    );
    if let Some(f) = &r.previous_failure {
        msg.push_str(&format!("  previous failure: {f}\n"));
    }
    for (id, stage, status) in &r.predecessors {
        msg.push_str(&format!(
            "  other task of this workflow: {id} {stage} {status} (untouched)\n"
        ));
    }
    if let Ok((root, m)) = dx::manifest_required(None)
        && m.scope() == r.scope
        && let Some(x) = ensure_worker(cfg, &h, &root, &m).await
    {
        msg.push_str(&x);
    }
    dx::out(&json!({"retry": r}), msg.trim_end());
    Ok(())
}

/// `eaeh worker history`.
pub async fn worker_history(cfg: &HarnessConfig, task: Option<&str>, limit: i64) -> Result<()> {
    let (pid, _) = dx::project_context(None)?;
    let h = harness(cfg).await?;
    let p = dx::caller_need(&h, Need::Any).await?;
    let list = co::executions(&h, &p, Scope::new(p.tenant, pid), task, limit).await?;
    let mut t = String::new();
    for e in &list {
        t.push_str(&format!(
            "{}  {:<26} attempt {} {:<11} {}{}{}{}\n",
            e.started_at.get(..19).unwrap_or(&e.started_at),
            e.stage,
            e.attempt,
            e.outcome,
            e.failure_class
                .as_ref()
                .map(|c| format!("[{c}] "))
                .unwrap_or_default(),
            e.duration_ms
                .map(|d| format!("{}s ", d / 1000))
                .unwrap_or_default(),
            e.auth_status,
            {
                let mut x = match (&e.package_bytes, &e.delivery_status, &e.retrieval_status) {
                    (Some(b), Some(d), Some(r)) => {
                        format!("\n      context: package {b} bytes, delivery {d} ({r})")
                    }
                    (Some(b), Some(d), None) => format!("\n      context: package {b} bytes, delivery {d}"),
                    _ => String::new(),
                };
                if e.denied_calls.unwrap_or(0) > 0 {
                    x.push_str(&format!("; {} access denial(s)", e.denied_calls.unwrap_or(0)));
                }
                if e.acknowledged_at.is_some() {
                    x.push_str("; acknowledged");
                }
                if let Some(r) = &e.remedy {
                    x.push_str(&format!("\n      fix: {r}"));
                }
                x
            }
        ));
    }
    if t.is_empty() {
        t = "No automated executions yet.".into();
    }
    dx::out(&json!({"executions": list}), t.trim_end());
    Ok(())
}

/// `eaeh worker log`.
pub async fn worker_log(cfg: &HarnessConfig, lines: usize) -> Result<()> {
    let (_, m) = dx::manifest_required(None)?;
    let h = harness(cfg).await?;
    let path = log_path(&h, m.scope());
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    let from = all.len().saturating_sub(lines);
    println!("{}", path.display());
    for l in &all[from..] {
        println!("{l}");
    }
    Ok(())
}

// ------------------------------------------------------------------ watch

fn latest_instance_id(
    h: &Harness,
    p: &Principal,
    scope: Scope,
) -> impl std::future::Future<Output = Result<WorkflowInstanceId>> {
    let engine = h.engine.clone();
    let p = p.clone();
    async move {
        engine
            .list(&p, scope, None, 1)
            .await?
            .into_iter()
            .next()
            .map(|i| i.id)
            .ok_or_else(|| HarnessError::not_found("no workflow runs in this project yet"))
    }
}

/// `eaeh workflow watch`: live progress until the run needs a person or finishes.
pub async fn watch(cfg: &HarnessConfig, id: Option<&str>) -> Result<()> {
    let (pid, found) = dx::project_context(None)?;
    let h = harness(cfg).await?;
    let p = dx::caller_need(&h, Need::Any).await?;
    let scope = Scope::new(p.tenant, pid);
    let id: WorkflowInstanceId = match id {
        Some(i) => i.parse()?,
        None => latest_instance_id(&h, &p, scope).await?,
    };
    let inst = h.engine.get_for(&p, id).await?;
    println!(
        "Watching {} ({}) – Ctrl-C to stop watching (the work continues).",
        id, inst.definition_id
    );
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut started_worker = false;
    loop {
        let inst = h.engine.get_for(&p, id).await?;
        for t in h.engine.history(id).await? {
            let key = format!("t{}", t.seq);
            if seen.insert(key) {
                println!(
                    "{}  {} → {}  [{}]",
                    t.occurred_at.get(11..19).unwrap_or(""),
                    t.from_state,
                    t.to_state,
                    t.trigger
                );
            }
        }
        let mut execs = co::executions(&h, &p, scope, None, 200).await?;
        execs.retain(|e| e.workflow_instance == id.to_string());
        execs.reverse();
        for e in &execs {
            let key = format!("e{}{}", e.id, e.outcome);
            if seen.insert(key) {
                let what = match e.outcome.as_str() {
                    "running" => format!(
                        "AI ({}) working on '{}' – attempt {}",
                        e.provider, e.stage, e.attempt
                    ),
                    "succeeded" => format!("AI result for '{}' accepted by EAEH validation", e.stage),
                    o => format!(
                        "AI attempt {} for '{}': {o}{} {}",
                        e.attempt,
                        e.stage,
                        e.failure_class
                            .as_ref()
                            .map(|c| format!(" [{c}]"))
                            .unwrap_or_default(),
                        e.detail.clone().unwrap_or_default()
                    ),
                };
                println!(
                    "{}  {what}",
                    e.ended_at
                        .as_ref()
                        .unwrap_or(&e.started_at)
                        .get(11..19)
                        .unwrap_or("")
                );
            }
        }
        match inst.status.as_str() {
            "waiting_approval" => {
                println!(
                    "\nApproval required at '{}'. Review it: eaeh review --as <approver-profile>",
                    inst.state
                );
                return Ok(());
            }
            "succeeded" | "failed" | "cancelled" | "escalated" => {
                let (_, text) = dx::status_report(&h, &p, id, None).await?;
                println!("\n{text}");
                return Ok(());
            }
            "waiting_external" => {
                let open: Vec<_> = h
                    .agents
                    .external_tasks(&p, scope, true)
                    .await?
                    .into_iter()
                    .filter(|t| t.workflow_instance == id.to_string())
                    .collect();
                if !open.is_empty()
                    && open
                        .iter()
                        .all(|t| matches!(t.dispatch_state.as_str(), "blocked" | "exhausted"))
                {
                    for t in &open {
                        println!(
                            "\nAI task '{}' stopped ({}): {}\n  After fixing the cause: eaeh worker retry {}",
                            t.state,
                            t.dispatch_state,
                            t.last_failure.clone().unwrap_or_default(),
                            t.task_id
                        );
                    }
                    return Ok(());
                }
                if co::live_worker(&h, &p, scope).await?.is_none() {
                    if !started_worker && let Some((root, m)) = &found {
                        started_worker = true;
                        if let Some(msg) = ensure_worker(cfg, &h, root, m).await {
                            println!("{msg}");
                        }
                    } else if !cfg.execution.coordinator.auto_start || found.is_none() {
                        println!("\nAI tasks are waiting and no worker is running: eaeh worker start");
                        return Ok(());
                    }
                }
            }
            _ => {}
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// ------------------------------------------------------------------ review

struct GateReview {
    approval: ApprovalId,
    instance: String,
    gate: String,
    artifact: String,
    hash: String,
}

async fn pending_gates(h: &Harness, scope: Scope) -> Result<Vec<GateReview>> {
    Ok(h.approvals
        .list(scope, Some(ApprovalStatus::Pending), None, 100)
        .await?
        .into_iter()
        .filter(|a| a.workflow_instance_id.is_some() && a.artifact_id.is_some())
        .map(|a| GateReview {
            approval: a.id,
            instance: a.workflow_instance_id.clone().unwrap_or_default(),
            gate: a.gate.clone().unwrap_or_default(),
            artifact: a.artifact_id.clone().unwrap_or_default(),
            hash: a.artifact_hash.clone().unwrap_or_default(),
        })
        .collect())
}

fn primary_document(written: &[String]) -> Option<String> {
    for pref in ["requirements.md", "hld.md", "lld.md", "implementation-plan.md"] {
        if let Some(w) = written.iter().find(|w| w.ends_with(pref)) {
            return Some(w.clone());
        }
    }
    written.iter().find(|w| w.ends_with(".md")).cloned()
}

/// `eaeh review`: show the document waiting at a workflow gate and record the approver's
/// decision. Only a human approver identity can decide (`--as <approver-profile>`); the
/// decision is bound to the hash of exactly the version shown.
#[allow(clippy::too_many_arguments)]
pub async fn review(
    cfg: &HarnessConfig,
    approval: Option<&str>,
    decision: Option<&str>,
    comments: Option<String>,
    out: Option<PathBuf>,
    open: bool,
    no_show: bool,
) -> Result<()> {
    let (pid, found) = dx::project_context(None)?;
    let h = harness(cfg).await?;
    let p = dx::caller_need(&h, Need::Decision).await?;
    let scope = Scope::new(p.tenant, pid);
    let gates = pending_gates(&h, scope).await?;
    if gates.is_empty() {
        dx::out(
            &json!({"pending": []}),
            "No document waits for review. Progress: `eaeh workflow watch`; skills/standards: `eaeh approval review`.",
        );
        return Ok(());
    }
    let g = match approval {
        Some(a) => gates
            .iter()
            .find(|g| g.approval.to_string() == a)
            .ok_or_else(|| HarnessError::not_found(format!("no pending workflow approval {a}")))?,
        None if gates.len() == 1 => &gates[0],
        None => {
            let mut t = String::from(
                "Several documents wait for review; choose one (`eaeh review <approval-id>`):\n",
            );
            for g in &gates {
                t.push_str(&format!(
                    "  {}  gate '{}' of workflow {}\n",
                    g.approval, g.gate, g.instance
                ));
            }
            dx::out(
                &json!({"pending": gates.iter().map(|g| json!({"approval": g.approval, "gate": g.gate, "workflow": g.instance})).collect::<Vec<_>>()}),
                t.trim_end(),
            );
            return Ok(());
        }
    };
    let base = found
        .as_ref()
        .map(|(r, _)| r.join("review"))
        .unwrap_or_else(|| PathBuf::from("review"));
    let dir = out.unwrap_or_else(|| base.join(&g.gate));
    let (meta, written) = crate::export_artifact(&h, scope, g.artifact.parse()?, &dir).await?;
    if meta.content_hash != g.hash {
        return Err(HarnessError::integrity(format!(
            "the stored artifact hash ({}) differs from the hash under approval ({}); do not approve",
            meta.content_hash, g.hash
        )));
    }
    let doc = primary_document(&written).map(|d| dir.join(d));
    let interactive = decision.is_none() && std::io::stdin().is_terminal() && !dx::json_out();
    if !no_show && !dx::json_out() {
        println!(
            "Gate '{}' – {} v{} (hash {})\nFiles ({}): {}",
            g.gate,
            meta.name,
            meta.version,
            meta.content_hash,
            dir.display(),
            written.join(", ")
        );
        if let Some(d) = &doc {
            if open {
                let opener = if cfg!(target_os = "macos") {
                    "open"
                } else {
                    "xdg-open"
                };
                let _ = std::process::Command::new(opener).arg(d).status();
            } else {
                let text = std::fs::read_to_string(d).unwrap_or_default();
                println!(
                    "\n──────── {} ────────\n{text}\n──────── end of {} ────────",
                    d.display(),
                    d.display()
                );
            }
        }
    }
    let (decision, comments) = match (decision, interactive) {
        (Some(d), _) => (ApprovalDecision::parse(d)?, comments.unwrap_or_default()),
        (None, true) => {
            eprint!(
                "\nDecision as '{}': [a]pprove, [c]hange request, [r]eject, [s]kip: ",
                p.id
            );
            let mut s = String::new();
            std::io::stdin().read_line(&mut s)?;
            let d = match s.trim() {
                "a" | "approve" => ApprovalDecision::Approve,
                "c" | "changes" | "request_changes" => ApprovalDecision::RequestChanges,
                "r" | "reject" => ApprovalDecision::Reject,
                _ => {
                    println!("Skipped; nothing recorded.");
                    return Ok(());
                }
            };
            let c = match comments {
                Some(c) => c,
                None => {
                    eprint!(
                        "Comments{}: ",
                        if d == ApprovalDecision::Approve {
                            " (optional)"
                        } else {
                            " (what must change)"
                        }
                    );
                    let mut c = String::new();
                    std::io::stdin().read_line(&mut c)?;
                    c.trim().to_string()
                }
            };
            (d, c)
        }
        (None, false) => {
            let next = format!(
                "decide: eaeh review {} --decision approve|request_changes|reject --comments \"…\" --as <approver-profile>",
                g.approval
            );
            dx::out(
                &json!({"approval": g.approval, "gate": g.gate, "artifact": meta.id, "hash": meta.content_hash, "files": written, "dir": dir, "next": next}),
                &format!("→ {next}"),
            );
            return Ok(());
        }
    };
    if decision != ApprovalDecision::Approve && comments.trim().is_empty() {
        return Err(HarnessError::invalid_input(
            "say what must change: --comments \"…\" is required for request_changes and reject",
        ));
    }
    let comments = if comments.trim().is_empty() {
        format!("approved via `eaeh review` by {}", p.id)
    } else {
        comments
    };
    let inst_id: WorkflowInstanceId = g.instance.parse()?;
    // The engine records the decision (approver identity, role, timestamp, comments, hash
    // check, separation of duties) and resumes the run to its next stop.
    h.engine
        .decide_approval(
            &p,
            g.approval,
            decision,
            &comments,
            Some(&meta.content_hash),
            eaeh_core::TraceContext::new_root(),
        )
        .await?;
    let rec = h.approvals.get(g.approval).await?;
    let mut text = format!(
        "Decision recorded: {} by {} for gate '{}' (artifact hash {}).",
        serde_json::to_value(rec.status)?.as_str().unwrap_or(""),
        p.id,
        g.gate,
        meta.content_hash
    );
    let (sv, st) = dx::status_report(&h, &p, inst_id, None).await?;
    text.push('\n');
    text.push_str(&st);
    if let Some((root, m)) = &found
        && let Some(msg) = ensure_worker(cfg, &h, root, m).await
    {
        text.push('\n');
        text.push_str(&msg);
    }
    dx::out(&json!({"approval": rec, "status": sv}), &text);
    Ok(())
}
