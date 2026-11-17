//! `eaeh setup` – one-command, idempotent local environment setup, verification, repair and
//! Governance Portal start (docs/implementation/stages/SETUP-AUTOMATION-PLAN.md).
//!
//! Orchestration only: every identity, token, project, skill and approval operation goes
//! through the existing governed services (`IdentityService`, `discover_skills`,
//! `sync_skills`, the credential store, `eaeh portal`). Nothing here bypasses authorisation:
//! when no valid administrator credential is available the command fails safely and says how
//! to recover. Tokens are never printed or logged.

use crate::credentials::Store;
use crate::{dx, harness};
use eaeh_core::{
    HarnessError, Principal, PrincipalId, PrincipalKind, ProjectAccess, ProjectId, Result, Role, Scope,
};
use eaeh_governance::NewPrincipal;
use eaeh_governance::authz::{Action, RbacAuthorizer};
use eaeh_runtime::maintenance;
use eaeh_runtime::project as proj;
use eaeh_runtime::{Harness, HarnessConfig};
use eaeh_skills::SkillStatus;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const RESET_PHRASE: &str = "RESET EAEH";

#[derive(Debug, Clone)]
pub struct SetupArgs {
    pub check: bool,
    pub repair: bool,
    pub reset: bool,
    pub confirm_reset: Option<String>,
    pub restore: Option<PathBuf>,
    pub open: bool,
    pub no_portal: bool,
    pub stop_portal: bool,
    pub bind: String,
    pub static_dir: PathBuf,
    pub tenant: String,
    pub project: String,
    pub ttl_days: i64,
    pub config_path: PathBuf,
}

// ------------------------------------------------------------------ the identity matrix

/// One logical profile of the default local-development configuration.
struct Spec {
    profile: &'static str,
    principal: &'static str,
    display: &'static str,
    role: Role,
    /// Actions the principal must be permitted / refused (verified against its real roles).
    must: &'static [Action],
    must_not: &'static [Action],
}

const MATRIX: [Spec; 4] = [
    Spec {
        profile: "admin",
        principal: "admin",
        display: "Administrator",
        role: Role::Admin,
        must: &[Action::PrincipalManage, Action::SkillManage],
        must_not: &[Action::WorkflowStart, Action::ApprovalDecide],
    },
    Spec {
        profile: "developer",
        principal: "dev",
        display: "Developer",
        role: Role::Operator,
        must: &[Action::WorkflowStart],
        must_not: &[Action::PrincipalManage, Action::ApprovalDecide],
    },
    Spec {
        profile: "approver",
        principal: "approver",
        display: "Approver",
        role: Role::Approver,
        must: &[Action::ApprovalDecide],
        must_not: &[Action::PrincipalManage, Action::WorkflowStart],
    },
    Spec {
        profile: "steward",
        principal: "steward",
        display: "Knowledge steward",
        role: Role::KnowledgeSteward,
        must: &[Action::SkillManage, Action::KnowledgeManage],
        must_not: &[Action::PrincipalManage, Action::ApprovalDecide],
    },
];

// ------------------------------------------------------------------ report

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum St {
    Ok,
    Repaired,
    Warning,
    Blocked,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
struct Item {
    area: String,
    status: St,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
}

#[derive(Default)]
struct Report {
    items: Vec<Item>,
    /// Repairs applied (also audited).
    repairs: Vec<String>,
    skills: Option<SkillSummary>,
    portal: Option<Value>,
}

impl Report {
    fn add(&mut self, area: &str, status: St, detail: impl Into<String>, fix: Option<String>) {
        self.items.push(Item {
            area: area.into(),
            status,
            detail: detail.into(),
            fix,
        });
    }
    fn ok(&mut self, area: &str, d: impl Into<String>) {
        self.add(area, St::Ok, d, None);
    }
    fn repaired(&mut self, area: &str, d: impl Into<String>) {
        let d = d.into();
        self.repairs.push(format!("{area}: {d}"));
        self.add(area, St::Repaired, d, None);
    }
    fn warn(&mut self, area: &str, d: impl Into<String>, fix: impl Into<String>) {
        self.add(area, St::Warning, d, Some(fix.into()));
    }
    fn block(&mut self, area: &str, d: impl Into<String>, fix: impl Into<String>) {
        self.add(area, St::Blocked, d, Some(fix.into()));
    }
    fn blocked(&self) -> bool {
        self.items.iter().any(|i| i.status == St::Blocked)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
struct SkillSummary {
    discovered: usize,
    registered: usize,
    changed: usize,
    already_registered: usize,
    invalid: usize,
    pending_approval: usize,
}

// ------------------------------------------------------------------ small file helpers

#[cfg(unix)]
fn private_dir(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(p)?;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}
#[cfg(not(unix))]
fn private_dir(p: &Path) -> Result<()> {
    std::fs::create_dir_all(p)?;
    Ok(())
}

fn private_file(p: &Path, bytes: &[u8]) -> Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(p)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

fn timestamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// A new, empty, private directory `<root>/<timestamp><suffix>` (never reuses a backup).
fn new_backup_dir(suffix: &str) -> Result<PathBuf> {
    let root = backups_root()?;
    private_dir(&root)?;
    for n in 0..1000 {
        let name = if n == 0 {
            format!("{}{suffix}", timestamp())
        } else {
            format!("{}-{n}{suffix}", timestamp())
        };
        let d = root.join(name);
        if !d.exists() {
            private_dir(&d)?;
            return Ok(d);
        }
    }
    Err(HarnessError::conflict("could not allocate a backup directory"))
}

fn prompt(q: &str) -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    eprint!("{q}");
    let _ = std::io::stderr().flush();
    let mut s = String::new();
    std::io::stdin().lock().read_line(&mut s).ok()?;
    Some(s.trim().to_string())
}

fn valid_name(what: &str, v: &str) -> Result<()> {
    if v.trim().is_empty() || v.len() > 64 || v.chars().any(char::is_control) {
        return Err(HarnessError::invalid_input(format!(
            "--{what} must be 1–64 printable characters"
        )));
    }
    Ok(())
}

// ------------------------------------------------------------------ backup / restore

#[derive(Debug, Serialize, Deserialize)]
struct BackupManifest {
    created_at: String,
    reason: String,
    data_dir: String,
    files: Vec<(String, String)>,
}

fn backups_root() -> Result<PathBuf> {
    Ok(crate::credentials::home()?.join("backups"))
}

/// Consistent, verified, protected backup (SQLite `VACUUM INTO` is WAL-safe; a plain copy
/// is the fallback for a database that cannot be opened) of the database, the credential
/// store and the configuration. Returns the backup directory (mode 0700, files 0600).
async fn backup(cfg: &HarnessConfig, cfg_path: &Path, reason: &str) -> Result<PathBuf> {
    let dir = new_backup_dir("")?;
    let mut files = vec![];
    let db = cfg.database_path();
    if db.is_file() {
        let dest = dir.join("eaeh.db");
        maintenance::backup_database(&db, &dest).await?;
        files.push((
            "eaeh.db".to_string(),
            eaeh_core::hash::sha256_hex(&std::fs::read(&dest)?),
        ));
        protect(&dest);
    }
    let creds = crate::credentials::home()?.join("credentials.json");
    if creds.is_file() {
        let b = std::fs::read(&creds)?;
        private_file(&dir.join("credentials.json"), &b)?;
        files.push(("credentials.json".into(), eaeh_core::hash::sha256_hex(&b)));
    }
    if let Ok(b) = std::fs::read(cfg_path) {
        private_file(&dir.join("eaeh.toml"), &b)?;
        files.push(("eaeh.toml".into(), eaeh_core::hash::sha256_hex(&b)));
    }
    let m = BackupManifest {
        created_at: chrono::Utc::now().to_rfc3339(),
        reason: reason.into(),
        data_dir: dx_data_dir(cfg),
        files,
    };
    private_file(&dir.join("MANIFEST.json"), &serde_json::to_vec_pretty(&m)?)?;
    Ok(dir)
}

#[cfg(unix)]
fn protect(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn protect(_p: &Path) {}

/// Canonical data directory string, as the credential store records it.
fn dx_data_dir(cfg: &HarnessConfig) -> String {
    let d = cfg.data_dir();
    std::fs::canonicalize(&d).unwrap_or(d).display().to_string()
}

fn remove_database(cfg: &HarnessConfig) -> Result<()> {
    let db = cfg.database_path();
    for p in [
        db.clone(),
        PathBuf::from(format!("{}-wal", db.display())),
        PathBuf::from(format!("{}-shm", db.display())),
    ] {
        if p.is_file() {
            std::fs::remove_file(&p)?;
        }
    }
    Ok(())
}

/// Replace the database with a verified backup and re-adopt its credential profiles for this
/// installation. The current state is backed up first.
fn restore_files(cfg: &HarnessConfig, from: &Path) -> Result<()> {
    let m: BackupManifest =
        serde_json::from_slice(&std::fs::read(from.join("MANIFEST.json")).map_err(|e| {
            HarnessError::invalid_input(format!("{} is not an EAEH backup: {e}", from.display()))
        })?)?;
    for (name, hash) in &m.files {
        let b = std::fs::read(from.join(name))?;
        if &eaeh_core::hash::sha256_hex(&b) != hash {
            return Err(HarnessError::integrity(format!(
                "backup file {name} does not match its recorded hash; refusing to restore"
            )));
        }
    }
    let src = from.join("eaeh.db");
    if !src.is_file() {
        return Err(HarnessError::invalid_input("the backup contains no database"));
    }
    remove_database(cfg)?;
    let dest = cfg.database_path();
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::copy(&src, &dest)?;
    // Profiles of this installation come from the backup (they belong to that database).
    let creds = from.join("credentials.json");
    if creds.is_file() {
        let old: Store = serde_json::from_slice(&std::fs::read(&creds)?)?;
        let dd = dx_data_dir(cfg);
        let mut cur = Store::load()?;
        cur.profiles.retain(|_, p| p.data_dir != dd);
        for (n, p) in old.profiles {
            if p.data_dir == dd {
                cur.profiles.insert(n, p);
            }
        }
        cur.save()?;
    }
    Ok(())
}

// ------------------------------------------------------------------ portal process control

#[derive(Debug, Serialize, Deserialize)]
struct PortalState {
    pid: u32,
    bind: String,
    launch_url: String,
    data_dir: String,
    started_at: String,
}

fn portal_dir() -> Result<PathBuf> {
    Ok(crate::credentials::home()?.join("portal"))
}

fn portal_state_path(cfg: &HarnessConfig) -> Result<PathBuf> {
    let key = &eaeh_core::hash::sha256_hex(dx_data_dir(cfg).as_bytes())[..12];
    Ok(portal_dir()?.join(format!("portal-{key}.json")))
}

fn read_portal_state(cfg: &HarnessConfig) -> Option<PortalState> {
    serde_json::from_slice(&std::fs::read(portal_state_path(cfg).ok()?).ok()?).ok()
}

fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The pid is alive and is an `eaeh … portal` process (guards against pid reuse).
fn is_portal_process(pid: u32) -> bool {
    pid_alive(pid)
        && Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .map(|o| {
                let c = String::from_utf8_lossy(&o.stdout);
                c.contains("eaeh") && c.contains("portal")
            })
            .unwrap_or(false)
}

fn port_open(bind: &str) -> bool {
    bind.parse::<std::net::SocketAddr>()
        .is_ok_and(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_millis(500)).is_ok())
}

/// A running portal answers HTTP.
fn http_alive(bind: &str) -> bool {
    let Ok(addr) = bind.parse::<std::net::SocketAddr>() else {
        return false;
    };
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    if s.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").is_err() {
        return false;
    }
    let mut buf = [0u8; 16];
    s.read(&mut buf)
        .is_ok_and(|n| n >= 5 && buf.starts_with(b"HTTP/"))
}

fn stop_portal(cfg: &HarnessConfig) -> Result<Option<u32>> {
    let Some(st) = read_portal_state(cfg) else {
        return Ok(None);
    };
    let mut stopped = None;
    if is_portal_process(st.pid) {
        let _ = Command::new("kill").arg(st.pid.to_string()).status();
        let t = Instant::now();
        while pid_alive(st.pid) && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(100));
        }
        if pid_alive(st.pid) {
            return Err(HarnessError::unavailable(format!(
                "portal process {} did not stop; stop it manually with `kill {}`",
                st.pid, st.pid
            )));
        }
        stopped = Some(st.pid);
    }
    let _ = std::fs::remove_file(portal_state_path(cfg)?);
    Ok(stopped)
}

fn find_static(cfg: &HarnessConfig, explicit: &Path) -> Option<PathBuf> {
    let beside = cfg.base_dir.parent().map(|r| r.join("portal/dist"));
    std::iter::once(explicit.to_path_buf())
        .chain(beside)
        .find(|d| d.join("index.html").is_file())
}

enum PortalOutcome {
    Started(PortalState),
    Reused(PortalState),
}

fn start_portal(cfg: &HarnessConfig, a: &SetupArgs, static_dir: &Path) -> Result<PortalOutcome> {
    // Already running for this installation?
    if let Some(st) = read_portal_state(cfg) {
        if is_portal_process(st.pid) && http_alive(&st.bind) {
            return Ok(PortalOutcome::Reused(st));
        }
        let _ = std::fs::remove_file(portal_state_path(cfg)?);
    }
    if port_open(&a.bind) {
        return Err(HarnessError::unavailable(format!(
            "port conflict: {} is already in use by another process (not this installation's portal). \
             Find it with `lsof -nP -iTCP:{} -sTCP:LISTEN`, stop it, or run `eaeh setup --bind 127.0.0.1:<free-port>`",
            a.bind,
            a.bind.rsplit(':').next().unwrap_or("")
        )));
    }
    let dir = portal_dir()?;
    private_dir(&dir)?;
    let key = &eaeh_core::hash::sha256_hex(dx_data_dir(cfg).as_bytes())[..12];
    let out_path = dir.join(format!("portal-{key}.out"));
    let log_path = dir.join(format!("portal-{key}.log"));
    private_file(&out_path, b"")?;
    private_file(&log_path, b"")?;
    let cfg_abs = std::fs::canonicalize(&a.config_path).unwrap_or(a.config_path.clone());
    let mut c = Command::new(std::env::current_exe()?);
    c.arg("--config")
        .arg(&cfg_abs)
        .args(["--as", "admin", "portal", "--bind", &a.bind])
        .args(["--approver", "approver", "--steward", "steward"])
        .args(["--operator", "developer", "--admin", "admin"])
        .arg("--static-dir")
        .arg(static_dir)
        .env_remove("EAEH_TOKEN")
        .env_remove("EAEH_PROFILE")
        .stdin(Stdio::null())
        .stdout(std::fs::OpenOptions::new().append(true).open(&out_path)?)
        .stderr(std::fs::OpenOptions::new().append(true).open(&log_path)?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0); // survive the terminal that ran setup
    }
    let mut child = c.spawn()?;
    let pid = child.id();
    let t = Instant::now();
    loop {
        let out = std::fs::read_to_string(&out_path).unwrap_or_default();
        if let Some(url) = out.lines().find(|l| l.starts_with("http://")) {
            let bind = url
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or(&a.bind)
                .to_string();
            if http_alive(&bind) {
                let st = PortalState {
                    pid,
                    bind,
                    launch_url: url.to_string(),
                    data_dir: dx_data_dir(cfg),
                    started_at: chrono::Utc::now().to_rfc3339(),
                };
                private_file(&portal_state_path(cfg)?, &serde_json::to_vec_pretty(&st)?)?;
                return Ok(PortalOutcome::Started(st));
            }
        }
        if let Ok(Some(status)) = child.try_wait() {
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            let tail: Vec<&str> = log.lines().rev().take(6).collect();
            let why = tail.into_iter().rev().collect::<Vec<_>>().join(" | ");
            return Err(HarnessError::unavailable(format!(
                "the portal exited during start ({status}): {why}. Log: {}",
                log_path.display()
            )));
        }
        if t.elapsed() > Duration::from_secs(45) {
            let _ = child.kill();
            return Err(HarnessError::unavailable(format!(
                "the portal did not become ready within 45 s. Log: {}",
                log_path.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn open_in_browser(url: &str) -> bool {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(opener).arg(url).status().is_ok_and(|s| s.success())
}

// ------------------------------------------------------------------ profile diagnosis

enum Diag {
    Missing,
    /// Bound to another EAEH installation (data directory).
    Foreign,
    /// Token expired / revoked / principal disabled.
    Dead {
        expired: bool,
    },
    /// Token valid but it authenticates as someone else.
    WrongPrincipal(Box<Principal>),
    WrongRoles(Box<Principal>),
    Valid(Box<Principal>),
}

async fn diagnose(h: &Harness, store: &Store, dd: &str, s: &Spec) -> Diag {
    let Some(p) = store.profiles.get(s.profile) else {
        return Diag::Missing;
    };
    if p.data_dir != dd {
        return Diag::Foreign;
    }
    match h.identity.authenticate_detailed(p.token.trim()).await {
        Err(_) => Diag::Dead { expired: p.expired() },
        Ok((pr, _, _)) if pr.id.as_str() != s.principal => Diag::WrongPrincipal(Box::new(pr)),
        Ok((pr, _, _)) if pr.roles().len() != 1 || !pr.has_role(s.role) => Diag::WrongRoles(Box::new(pr)),
        Ok((pr, _, _)) => Diag::Valid(Box::new(pr)),
    }
}

fn roles_str(p: &Principal) -> String {
    p.roles().iter().map(|r| r.as_str()).collect::<Vec<_>>().join(",")
}

/// Permissions come from the authenticated principal's real roles, never from the profile
/// name or the stored metadata.
fn permission_problem(p: &Principal, s: &Spec) -> Option<String> {
    let permits = |a: Action| p.roles().iter().any(|r| RbacAuthorizer::role_permits(*r, a));
    for a in s.must {
        if !permits(*a) {
            return Some(format!("lacks required permission {}", a.as_str()));
        }
    }
    for a in s.must_not {
        if permits(*a) {
            return Some(format!(
                "holds forbidden permission {} (separation of duties)",
                a.as_str()
            ));
        }
    }
    None
}

/// Back up the credential store once per run before any stored profile is replaced.
struct CredBackup(Option<PathBuf>);
impl CredBackup {
    fn ensure(&mut self, cfg: &HarnessConfig, cfg_path: &Path) -> Result<()> {
        if self.0.is_none() && crate::credentials::home()?.join("credentials.json").is_file() {
            let d = new_backup_dir("-credentials")?;
            let b = std::fs::read(crate::credentials::home()?.join("credentials.json"))?;
            private_file(&d.join("credentials.json"), &b)?;
            let _ = cfg;
            let _ = cfg_path;
            self.0 = Some(d);
        }
        Ok(())
    }
}

/// Issue a new token for `pid` as `actor` and store it under `profile` (never displayed).
async fn issue_and_store(
    h: &Harness,
    actor: &Principal,
    pid: &PrincipalId,
    profile: &str,
    ttl: i64,
) -> Result<()> {
    let t = h
        .identity
        .issue_token(actor, pid, &format!("{profile} (setup)"), ttl)
        .await?;
    let (p, id, exp) = h.identity.authenticate_detailed(t.token.expose()).await?;
    dx::store_issued(h, profile, &p, t.token.expose(), &id, &exp)
}

// ------------------------------------------------------------------ the run

pub async fn run(cfg: &HarnessConfig, mut a: SetupArgs) -> Result<()> {
    if a.check && (a.reset || a.restore.is_some() || a.stop_portal) {
        return Err(HarnessError::invalid_input(
            "--check is read-only and cannot be combined with --reset, --restore or --stop-portal",
        ));
    }
    if a.reset && a.restore.is_some() {
        return Err(HarnessError::invalid_input(
            "use either --reset or --restore, not both",
        ));
    }
    valid_name("tenant", &a.tenant)?;
    valid_name("project", &a.project)?;
    if !(1..=3650).contains(&a.ttl_days) {
        return Err(HarnessError::invalid_input("--ttl-days must be 1–3650"));
    }
    if a.bind
        .parse::<std::net::SocketAddr>()
        .map_or(true, |s| !s.ip().is_loopback())
    {
        return Err(HarnessError::invalid_input(
            "--bind must be a loopback ip:port (the portal has no login), e.g. 127.0.0.1:8090",
        ));
    }

    if a.stop_portal {
        match stop_portal(cfg)? {
            Some(pid) => dx::out(
                &json!({"portal": "stopped", "pid": pid}),
                &format!("Governance Portal (pid {pid}) stopped. Start it again with `eaeh setup`."),
            ),
            None => dx::out(
                &json!({"portal": "not_running"}),
                "No Governance Portal is running for this installation.",
            ),
        }
        return Ok(());
    }

    let mut rep = Report::default();
    let db_exists = cfg.database_path().is_file();

    // Existing installation: offer A / B when interactive and nothing was chosen.
    if db_exists
        && !a.check
        && !a.repair
        && !a.reset
        && a.restore.is_none()
        && let Some(ans) = prompt(
            "EAEH is already configured here.\n  A) Continue with the existing database and repair configuration (default)\n  B) Reset and configure EAEH from the beginning (database is backed up first)\nChoose [A/b]: ",
        )
        && ans.eq_ignore_ascii_case("b")
    {
        a.reset = true;
    }

    // Reset / restore: warn, confirm, back up, only then touch the database.
    let mut backup_dir: Option<PathBuf> = None;
    if a.reset || a.restore.is_some() {
        let what = if a.reset { "RESET" } else { "RESTORE" };
        eprintln!(
            "WARNING: {what} replaces the EAEH database {} .\n  Projects, workflows, approvals, audit records and knowledge in it will be LOST \
             (a verified backup is written first). Skills, configuration, source-code workspaces and files are never deleted.\n  \
             Credential profiles of this installation are replaced by newly issued ones (the old file is backed up).",
            cfg.database_path().display()
        );
        let typed = match a.confirm_reset.clone() {
            Some(t) => Some(t),
            None => prompt(&format!("Type {RESET_PHRASE} to continue: ")),
        };
        if typed.as_deref() != Some(RESET_PHRASE) {
            return Err(HarnessError::invalid_input(format!(
                "not confirmed – nothing was changed. Confirm with `--confirm-reset \"{RESET_PHRASE}\"` (or type it when prompted)"
            )));
        }
        if let Some(pid) = stop_portal(cfg)? {
            eprintln!("stopped the running portal (pid {pid}) before touching the database");
        }
        if db_exists {
            let d = backup(cfg, &a.config_path, if a.reset { "reset" } else { "restore" }).await?;
            eprintln!("backup written and verified: {}", d.display());
            backup_dir = Some(d);
        }
        if let Some(from) = a.restore.clone() {
            restore_files(cfg, &from)?;
            rep.repaired("Database", format!("restored from {}", from.display()));
        } else {
            remove_database(cfg)?;
            rep.repaired(
                "Database",
                format!(
                    "reset; previous database backed up to {}",
                    backup_dir
                        .as_ref()
                        .map_or("(none existed)".into(), |d| d.display().to_string())
                ),
            );
        }
    }

    if a.check && !db_exists {
        rep.block(
            "Database",
            format!(
                "not initialised ({} does not exist)",
                cfg.database_path().display()
            ),
            "run `eaeh setup`",
        );
        return finish(cfg, &a, rep, None).await;
    }

    // 1–3. Installation, configuration, database.
    let h = match harness(cfg).await {
        Ok(h) => h,
        Err(e) => {
            rep.block(
                "Database",
                format!("initialisation failed: {}", e.message),
                "fix the reported problem (configuration paths, file permissions, free disk space); nothing else was changed, so `eaeh setup` can simply be re-run",
            );
            return finish(cfg, &a, rep, None).await;
        }
    };
    let mut audit_scope: Option<Scope> = None;
    let result = orchestrate(cfg, &h, &a, &mut rep, backup_dir, &mut audit_scope).await;
    let _ = maintenance::audit_system(
        &h,
        "setup",
        "setup.run",
        "installation",
        &dx_data_dir(cfg),
        result.is_ok() && !rep.blocked(),
        audit_scope,
        json!({
            "mode": if a.check { "check" } else if a.repair { "repair" } else { "normal" },
            "reset": a.reset,
            "repairs": rep.repairs,
            "blocked": rep.items.iter().filter(|i| i.status == St::Blocked).map(|i| &i.area).collect::<Vec<_>>(),
            "skills": rep.skills,
        }),
    )
    .await;
    if let Err(e) = result {
        rep.block(
            "Setup",
            e.message.clone(),
            "see the message; `eaeh setup --check` shows the full diagnosis",
        );
    }
    h.shutdown().await;
    drop(h);
    let wants_portal = !a.check && !a.no_portal && !rep.blocked();
    finish(cfg, &a, rep, wants_portal.then_some(())).await
}

async fn orchestrate(
    cfg: &HarnessConfig,
    h: &Harness,
    a: &SetupArgs,
    rep: &mut Report,
    backup_dir: Option<PathBuf>,
    audit_scope: &mut Option<Scope>,
) -> Result<()> {
    let _ = backup_dir;
    let dd = dx::data_dir(h);
    let health = h.health().await;
    if health.status == "ok" && !health.migrations.is_empty() {
        rep.ok(
            "Database",
            format!(
                "healthy ({} migrations applied, {})",
                health.migrations.len(),
                cfg.database_path().display()
            ),
        );
    } else {
        rep.block(
            "Database",
            format!("unhealthy: status {}, database {}", health.status, health.database),
            "check disk space and permissions of the data directory; if it is corrupted restore a backup: `eaeh setup --restore <backup-dir>`",
        );
        return Ok(());
    }
    match h.db.verify_audit_chain().await {
        Ok(r) if r.valid => rep.ok("Audit log", format!("hash chain valid ({} records)", r.records_checked)),
        Ok(r) => rep.block(
            "Audit log",
            format!("hash chain INVALID at seq {:?}: {}", r.first_invalid_seq, r.reason.unwrap_or_default()),
            "the audit trail was tampered with or corrupted; do not continue – restore a backup (`eaeh setup --restore <backup-dir>`)",
        ),
        Err(e) => rep.warn("Audit log", format!("could not be verified: {}", e.message), "eaeh audit verify"),
    }

    let mut creds = CredBackup(None);
    let mut acting: Option<Principal> = None;
    let mut store = Store::load()?;

    // 4–5. First-time bootstrap of tenant, project and administrator.
    if h.identity.principal_count().await? == 0 {
        if a.check {
            rep.block("Tenant", "not configured (no principals)", "run `eaeh setup`");
            return Ok(());
        }
        // The only copy of the admin token is written to the store: make sure it can be.
        store.save()?;
        creds.ensure(cfg, &a.config_path)?;
        let (admin, project, tok) = h
            .identity
            .bootstrap(
                &a.tenant,
                &a.project,
                PrincipalId::new("admin")?,
                "Administrator",
                a.ttl_days,
            )
            .await?;
        let (p, id, exp) = h.identity.authenticate_detailed(tok.token.expose()).await?;
        dx::store_issued(h, "admin", &p, tok.token.expose(), &id, &exp)?;
        rep.repaired(
            "Tenant",
            format!(
                "created tenant '{}' and project '{}' ({project})",
                a.tenant, a.project
            ),
        );
        rep.repaired(
            "admin",
            "administrator principal 'admin' created; credential stored as profile 'admin' (not displayed)",
        );
        acting = Some(admin);
        store = Store::load()?;
    }

    // Acting administrator: the stored `admin` profile, verified – never invented.
    let admin_spec = &MATRIX[0];
    let mut admin_note: Option<String> = None;
    if acting.is_none() {
        match diagnose(h, &store, &dd, admin_spec).await {
            Diag::Valid(p) => acting = Some(*p),
            Diag::WrongPrincipal(p) if p.has_role(Role::Admin) => {
                admin_note = Some(format!(
                    "profile 'admin' holds a valid administrator token of principal '{}', not '{}'",
                    p.id, admin_spec.principal
                ));
                acting = Some(*p);
            }
            _ => {}
        }
    }
    let Some(actor) = acting.clone() else {
        // Fail safely: diagnose what can be read, change nothing.
        let msg = match diagnose(h, &store, &dd, admin_spec).await {
            Diag::Missing => "no credential profile 'admin' exists for this installation",
            Diag::Foreign => "profile 'admin' belongs to another EAEH installation",
            Diag::Dead { expired: true } => "the token of profile 'admin' has expired",
            Diag::Dead { expired: false } => {
                "the token of profile 'admin' was revoked or its principal disabled"
            }
            Diag::WrongPrincipal(_) | Diag::WrongRoles(_) => {
                "profile 'admin' does not resolve to an administrator"
            }
            Diag::Valid(_) => "administrator unavailable",
        };
        rep.block(
            "admin",
            format!("administrator credential unavailable: {msg}. Repairing identities needs an administrator; setup never bypasses authorisation or invents credentials"),
            "if you still have an administrator token: `eaeh auth login --profile admin --token-file <file>` then re-run `eaeh setup`. \
             If it is lost, the only recovery is a clean installation: `eaeh setup --reset` (backs up the database and credentials first)",
        );
        // Read-only diagnosis of the other profiles so the report is complete.
        for s in &MATRIX[1..] {
            let d = match (
                h.identity.get_principal(&PrincipalId::new(s.principal)?).await,
                diagnose(h, &store, &dd, s).await,
            ) {
                (Err(_), _) => format!("principal '{}' does not exist", s.principal),
                (Ok(_), Diag::Valid(_)) => "valid".to_string(),
                (Ok(_), _) => "needs repair (requires the administrator)".to_string(),
            };
            rep.add(s.profile, St::Skipped, d, None);
        }
        return Ok(());
    };
    let tenant = actor.tenant;

    // Project context.
    let projects = h.identity.projects(tenant).await?;
    let project: Option<ProjectId> = match projects.iter().find(|(_, n)| *n == a.project) {
        Some((id, _)) => {
            rep.ok("Project", format!("'{}' registered ({id})", a.project));
            Some(*id)
        }
        None if a.check => {
            rep.block(
                "Project",
                format!("project '{}' is not registered", a.project),
                "run `eaeh setup`",
            );
            None
        }
        None => {
            let id = h.identity.create_project(&actor, tenant, &a.project).await?;
            rep.repaired("Project", format!("project '{}' created ({id})", a.project));
            Some(id)
        }
    };
    if rep.items.iter().all(|i| i.area != "Tenant") {
        rep.ok("Tenant", format!("configured ({tenant})"));
    }
    let Some(project) = project else {
        return Ok(());
    };
    let scope = Scope::new(tenant, project);
    *audit_scope = Some(scope);

    // 6–8. Principals, roles, credential profiles, project access (every project of the tenant:
    // the portal and CLI act as these people on whichever registered project you open).
    let all_projects: Vec<ProjectId> = h
        .identity
        .projects(tenant)
        .await?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let may_change = !a.check;
    for s in &MATRIX {
        identity_step(
            h,
            cfg,
            a,
            rep,
            &mut creds,
            &actor,
            admin_note.as_deref(),
            s,
            scope,
            &all_projects,
            may_change,
        )
        .await;
    }
    if let Ok(mut st) = Store::load()
        && st.default.is_none()
        && st.profiles.get("developer").is_some_and(|p| p.data_dir == dd)
        && may_change
    {
        st.default = Some("developer".into());
        let _ = st.save();
    }
    // Project permissions summary (verified from the database, not from stored metadata).
    let mut access_bad = vec![];
    for s in &MATRIX[1..] {
        match h.identity.get_principal(&PrincipalId::new(s.principal)?).await {
            Ok((p, true))
                if p.tenant == tenant
                    && all_projects
                        .iter()
                        .all(|pr| p.can_access(&Scope::new(tenant, *pr))) => {}
            _ => access_bad.push(s.principal),
        }
    }
    if access_bad.is_empty() {
        rep.ok(
            "Project permissions",
            format!(
                "dev, approver and steward can access all {} project(s) of the tenant",
                all_projects.len()
            ),
        );
    } else {
        rep.block(
            "Project permissions",
            format!("missing project access for: {}", access_bad.join(", ")),
            "run `eaeh setup` (grants project access) or `eaeh principal grant --id <principal>`",
        );
    }

    // 9. Workflow definitions and agents.
    let defs = h.engine.definitions().await?;
    let ids: Vec<String> = defs.iter().map(|d| d.id.to_string()).collect();
    let missing: Vec<&str> = ["requirements.analysis", proj::DESIGN, proj::DELIVERY]
        .into_iter()
        .filter(|w| !ids.iter().any(|i| i == w))
        .collect();
    if missing.is_empty() {
        rep.ok(
            "Workflow definitions",
            format!(
                "{} registered (requirements, design, delivery present)",
                ids.len()
            ),
        );
    } else {
        rep.block(
            "Workflow definitions",
            format!("missing: {}", missing.join(", ")),
            format!(
                "check `definitions.workflows_dir` in the configuration ({})",
                cfg.resolve(&cfg.definitions.workflows_dir).display()
            ),
        );
    }
    match h.agent_registry.list().await {
        Ok(v) if !v.is_empty() => rep.ok("Agents", format!("{} registered", v.len())),
        Ok(_) => rep.block(
            "Agents",
            "no agent definitions registered",
            format!(
                "check `definitions.agents_dir` ({})",
                cfg.resolve(&cfg.definitions.agents_dir).display()
            ),
        ),
        Err(e) => rep.block(
            "Agents",
            e.message,
            "check `definitions.agents_dir` in the configuration",
        ),
    }

    // 10. Skills (never auto-approved).
    skills_step(h, a, rep, &actor, project, may_change).await;

    // 11. External AI, worker, MCP.
    external_ai_step(h, cfg, rep, &actor, scope).await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn identity_step(
    h: &Harness,
    cfg: &HarnessConfig,
    a: &SetupArgs,
    rep: &mut Report,
    creds: &mut CredBackup,
    actor: &Principal,
    admin_note: Option<&str>,
    s: &Spec,
    scope: Scope,
    all_projects: &[ProjectId],
    may_change: bool,
) {
    let area = s.profile;
    let Ok(pid) = PrincipalId::new(s.principal) else {
        rep.block(area, "invalid principal id", "internal error");
        return;
    };
    let dd = dx::data_dir(h);

    // Principal: exists, enabled, same tenant, exactly the required role, project access.
    let mut created_or_fixed = false;
    let principal = match h.identity.get_principal(&pid).await {
        Err(_) if !may_change => {
            rep.block(
                area,
                format!("principal '{}' does not exist", s.principal),
                "run `eaeh setup`",
            );
            return;
        }
        Err(_) if s.profile == "admin" && !a.repair => {
            rep.block(
                area,
                format!(
                    "principal 'admin' does not exist; {}",
                    admin_note.unwrap_or("the acting administrator has another id")
                ),
                "run `eaeh setup --repair` to create the standard 'admin' principal (the current credential is backed up first)",
            );
            return;
        }
        Err(_) => {
            let spec = NewPrincipal {
                id: pid.clone(),
                display_name: s.display.into(),
                kind: PrincipalKind::Human,
                roles: [s.role].into(),
                projects: if s.role == Role::Admin {
                    ProjectAccess::All
                } else {
                    ProjectAccess::Only(all_projects.iter().copied().collect())
                },
            };
            match h.identity.create_principal(actor, spec).await {
                Ok(p) => {
                    rep.repaired(
                        area,
                        format!(
                            "principal '{}' created with role {}",
                            s.principal,
                            s.role.as_str()
                        ),
                    );
                    created_or_fixed = true;
                    p
                }
                Err(e) => {
                    rep.block(
                        area,
                        format!("could not create principal '{}': {}", s.principal, e.message),
                        "check administrator rights (`eaeh auth status`)",
                    );
                    return;
                }
            }
        }
        Ok((p, active)) => {
            if p.tenant != scope.tenant {
                rep.block(
                    area,
                    format!("principal '{}' belongs to another tenant", s.principal),
                    "choose a clean installation: `eaeh setup --reset`",
                );
                return;
            }
            if p.roles().len() != 1 || !p.has_role(s.role) {
                let extra = if s.role != Role::Admin && p.has_role(Role::Admin) {
                    " – it holds ADMIN privileges, which this identity must never have"
                } else {
                    ""
                };
                rep.block(
                    area,
                    format!(
                        "principal '{}' has roles [{}] but must have exactly [{}]{extra}",
                        s.principal,
                        roles_str(&p),
                        s.role.as_str()
                    ),
                    "EAEH never changes an existing principal's roles implicitly, so setup will not either. Disable the principal (`eaeh principal disable <id>`) and re-run setup under a clean installation (`eaeh setup --reset`)",
                );
                return;
            }
            if !active {
                if a.repair && may_change {
                    match h.identity.set_principal_status(actor, &pid, true).await {
                        Ok(()) => {
                            rep.repaired(area, format!("principal '{}' re-enabled (--repair)", s.principal));
                            created_or_fixed = true;
                        }
                        Err(e) => {
                            rep.block(area, e.message, "check administrator rights");
                            return;
                        }
                    }
                } else {
                    rep.block(
                        area,
                        format!("principal '{}' is disabled", s.principal),
                        "re-enable it deliberately with `eaeh setup --repair`",
                    );
                    return;
                }
            }
            let lacking: Vec<ProjectId> = if s.role == Role::Admin {
                vec![]
            } else {
                all_projects
                    .iter()
                    .copied()
                    .filter(|pr| !p.can_access(&Scope::new(scope.tenant, *pr)))
                    .collect()
            };
            if !lacking.is_empty() {
                if may_change {
                    for pr in &lacking {
                        if let Err(e) = h.identity.grant_project_access(actor, &pid, *pr).await {
                            rep.block(
                                area,
                                format!("project access could not be granted: {}", e.message),
                                "check administrator rights",
                            );
                            return;
                        }
                    }
                    rep.repaired(
                        area,
                        format!("granted '{}' access to {} project(s)", s.principal, lacking.len()),
                    );
                    created_or_fixed = true;
                } else {
                    rep.block(
                        area,
                        format!(
                            "principal '{}' has no access to {} project(s)",
                            s.principal,
                            lacking.len()
                        ),
                        "run `eaeh setup`",
                    );
                    return;
                }
            }
            h.identity.get_principal(&pid).await.map(|x| x.0).unwrap_or(p)
        }
    };
    let _ = principal;

    // Credential profile: must authenticate AS this principal with exactly this role.
    let store = Store::load().unwrap_or_default();
    let diag = diagnose(h, &store, &dd, s).await;
    let replace_reason: Option<String> = match &diag {
        Diag::Valid(p) => {
            if let Some(problem) = permission_problem(p, s) {
                rep.block(
                    area,
                    format!("principal '{}' {problem}", s.principal),
                    "do not use this installation for approvals; `eaeh setup --reset`",
                );
                return;
            }
            if !created_or_fixed {
                rep.ok(
                    area,
                    format!(
                        "valid – profile '{}' → principal '{}' ({})",
                        s.profile,
                        s.principal,
                        s.role.as_str()
                    ),
                );
                return;
            }
            None
        }
        Diag::Missing => Some("no credential stored".into()),
        Diag::Dead { expired } => Some(
            if *expired {
                "stored token expired"
            } else {
                "stored token is no longer valid (revoked, from a previous database, or principal disabled)"
            }
            .into(),
        ),
        Diag::WrongPrincipal(p) => {
            if !(a.repair && may_change) {
                rep.block(
                    area,
                    format!(
                        "IDENTITY MISMATCH: profile '{}' holds a valid token of principal '{}' (roles: {}), expected '{}'",
                        s.profile,
                        p.id,
                        roles_str(p),
                        s.principal
                    ),
                    "setup will not overwrite a profile that maps to another principal silently. Review `eaeh auth status`, then `eaeh setup --repair` replaces it with a token for the correct principal (the credential file is backed up first)",
                );
                return;
            }
            Some(format!("profile mapped to '{}' (repaired by --repair)", p.id))
        }
        Diag::WrongRoles(p) => {
            rep.block(
                area,
                format!(
                    "profile '{}' token resolves to '{}' with roles [{}]",
                    s.profile,
                    p.id,
                    roles_str(p)
                ),
                "run `eaeh setup --repair`",
            );
            return;
        }
        Diag::Foreign => {
            if !(a.repair && may_change) {
                rep.block(
                    area,
                    format!("profile '{}' is bound to another EAEH installation", s.profile),
                    "setup will not overwrite another installation's credential silently; `eaeh setup --repair` replaces it (backed up first), or use a separate EAEH_HOME for this installation",
                );
                return;
            }
            Some("profile bound to another installation (replaced by --repair)".into())
        }
    };
    if let Some(reason) = replace_reason {
        if !may_change {
            rep.block(area, format!("credential problem: {reason}"), "run `eaeh setup`");
            return;
        }
        if let Err(e) = creds.ensure(cfg, &a.config_path) {
            rep.block(
                area,
                format!("could not back up the credential store: {}", e.message),
                "check permissions of the EAEH home directory",
            );
            return;
        }
        // One attempt, then verify (no retry loop).
        if let Err(e) = issue_and_store(h, actor, &pid, s.profile, a.ttl_days).await {
            rep.block(
                area,
                format!("credential could not be issued: {}", e.message),
                "check administrator rights; `eaeh auth status`",
            );
            return;
        }
        rep.repaired(
            area,
            format!(
                "credential issued and stored as profile '{}' ({reason}; token not displayed)",
                s.profile
            ),
        );
        let _ = maintenance::audit_actor(
            h,
            actor,
            "setup.credential_profile",
            "credential_profile",
            s.profile,
            json!({"principal": s.principal, "reason": reason}),
        )
        .await;
    }
    // Verify the final state.
    let store = Store::load().unwrap_or_default();
    match diagnose(h, &store, &dd, s).await {
        Diag::Valid(p) => {
            if let Some(problem) = permission_problem(&p, s) {
                rep.block(
                    area,
                    format!("verification failed: '{}' {problem}", s.principal),
                    "`eaeh setup --reset`",
                );
            }
        }
        _ => rep.block(
            area,
            "repair applied but the profile could not be verified",
            "run `eaeh auth status`; re-run `eaeh setup --check`",
        ),
    }
}

async fn skills_step(
    h: &Harness,
    _a: &SetupArgs,
    rep: &mut Report,
    actor: &Principal,
    project: ProjectId,
    may_change: bool,
) {
    let mut sum = SkillSummary::default();
    let pending = |all: &[eaeh_skills::SkillRecord]| {
        all.iter()
            .filter(|r| r.status == SkillStatus::PendingApproval)
            .count()
    };
    if !may_change {
        // Read-only: count what is registered.
        match h.skills.list(actor).await {
            Ok(all) => {
                sum.already_registered = all.len();
                sum.pending_approval = pending(&all);
                rep.ok(
                    "Skills",
                    format!(
                        "{} registered, {} awaiting approval (check mode: nothing registered)",
                        all.len(),
                        sum.pending_approval
                    ),
                );
            }
            Err(e) => rep.warn("Skills", e.message, "eaeh skill list"),
        }
        rep.skills = Some(sum);
        return;
    }
    let before: std::collections::BTreeSet<String> = h
        .skills
        .list(actor)
        .await
        .map(|v| v.iter().map(|r| r.definition.reference()).collect())
        .unwrap_or_default();
    let found = match proj::discover_skills(h, Some(project)).await {
        Ok(f) => f,
        Err(e) => {
            rep.block(
                "Skills",
                format!("discovery failed: {}", e.message),
                "check `definitions.skills_dir` and the skill files (`eaeh skill list`)",
            );
            return;
        }
    };
    let mut invalid = vec![];
    for o in &found {
        match o.action.as_str() {
            "changed" => sum.changed += 1,
            "new" | "unchanged" => {}
            _ => {
                sum.invalid += 1;
                invalid.push(format!("{}: {}", o.dir, o.detail));
            }
        }
    }
    // Skills that workflows require and that exist as repository files (JSON / front-matter files).
    let file_skills = proj::repository_skills(h).map(|v| v.len()).unwrap_or(0);
    let workflows: Vec<String> = ["requirements.analysis", proj::DESIGN, proj::DELIVERY]
        .into_iter()
        .map(String::from)
        .collect();
    let mut file_note = String::new();
    match proj::project_skill_references(h, &workflows).await {
        Ok(refs) => match proj::sync_skills(h, actor, project, &refs).await {
            Ok(res) => {
                let missing: Vec<&str> = res
                    .iter()
                    .filter(|o| o.action == "missing_definition" || o.action == "failed")
                    .map(|o| o.reference.as_str())
                    .collect();
                if !missing.is_empty() {
                    file_note = format!(
                        "workflow skills without a usable definition: {}",
                        missing.join(", ")
                    );
                }
            }
            Err(e) => file_note = format!("workflow skill sync failed: {}", e.message),
        },
        Err(e) => file_note = format!("workflow skill references unavailable: {}", e.message),
    }
    let after = h.skills.list(actor).await.unwrap_or_default();
    let new_versions = after
        .iter()
        .filter(|r| !before.contains(&r.definition.reference()))
        .count();
    sum.discovered = found.len() + file_skills;
    sum.registered = new_versions.saturating_sub(sum.changed);
    sum.already_registered = sum
        .discovered
        .saturating_sub(sum.invalid + sum.registered + sum.changed);
    sum.pending_approval = pending(&after);
    let detail = format!(
        "{} discovered, {} registered, {} changed, {} already registered, {} invalid; {} awaiting human approval",
        sum.discovered,
        sum.registered,
        sum.changed,
        sum.already_registered,
        sum.invalid,
        sum.pending_approval
    );
    let mut problems = vec![];
    if !invalid.is_empty() {
        problems.push(format!("invalid: {}", invalid.join("; ")));
    }
    if !file_note.is_empty() {
        problems.push(file_note);
    }
    if !problems.is_empty() {
        rep.warn("Skills", format!("{detail} ({})", problems.join("; ")), "fix or remove the invalid skill files; `eaeh skill list`; `eaeh skill sync` shows missing workflow skills");
    } else if sum.registered + sum.changed > 0 {
        rep.repaired("Skills", detail);
    } else {
        rep.ok("Skills", detail);
    }
    if sum.pending_approval > 0 {
        rep.add(
            "Skill approval",
            St::Warning,
            format!("{} skill version(s) need a human approver; unapproved skills are NOT available to agents or workflows", sum.pending_approval),
            Some("approve in the Governance Portal (Skills page) or `eaeh approval review --as approver`".into()),
        );
    }
    rep.skills = Some(sum);
}

async fn external_ai_step(
    h: &Harness,
    cfg: &HarnessConfig,
    rep: &mut Report,
    actor: &Principal,
    scope: Scope,
) {
    use eaeh_runtime::coordinator as co;
    let mode = h.execution_mode;
    match mode {
        eaeh_runtime::ExecutionMode::ExternalAi => match co::executor_from_config(&cfg.execution.coordinator)
        {
            Ok(e) => {
                let pf = e.preflight().await;
                match pf.usable() {
                    Ok(()) => rep.ok(
                        "External AI",
                        format!(
                            "ready – mode external_ai, client {} {} ({}); sign-in is not verified without a model call (ADR-0025)",
                            pf.program.clone().unwrap_or_default(),
                            pf.version.clone().unwrap_or_default(),
                            pf.auth.summary()
                        ),
                    ),
                    Err((c, d)) => rep.warn("External AI", format!("not ready [{}]: {d}", c.as_str()), c.remedy()),
                }
            }
            Err(e) => rep.warn(
                "External AI",
                format!("not configured: {}", e.message),
                "set `[execution.coordinator] provider = \"claude_code\"`",
            ),
        },
        eaeh_runtime::ExecutionMode::DirectProvider => {
            if h.models.models().is_empty() {
                rep.warn(
                    "External AI",
                    "mode direct_provider but no model provider is enabled",
                    "enable [providers.anthropic] and export EAEH_SECRET_ANTHROPIC_API_KEY",
                );
            } else {
                rep.ok(
                    "External AI",
                    format!(
                        "mode direct_provider, {} model(s) registered",
                        h.models.models().len()
                    ),
                );
            }
        }
        eaeh_runtime::ExecutionMode::Deterministic => {
            rep.ok("External AI", "mode deterministic (recorded fixtures, no AI)");
        }
    }
    match co::workers(h, actor, scope).await {
        Ok(w) if w.iter().any(|x| x.live) => rep.ok("Worker", "running"),
        Ok(_) => rep.ok(
            "Worker",
            if cfg.execution.coordinator.auto_start {
                "not running (starts automatically when a run needs the AI)"
            } else {
                "not running (start with `eaeh worker start`)"
            },
        ),
        Err(e) => rep.warn(
            "Worker",
            format!("status unavailable: {}", e.message),
            "eaeh worker status",
        ),
    }
    match std::env::current_exe() {
        Ok(p) => rep.ok(
            "MCP",
            format!(
                "`eaeh mcp` server available ({}); per-project .mcp.json is written by `eaeh project init`",
                p.display()
            ),
        ),
        Err(_) => rep.warn("MCP", "cannot locate the eaeh binary", "ensure `eaeh` is on PATH"),
    }
}

// ------------------------------------------------------------------ output

async fn finish(cfg: &HarnessConfig, a: &SetupArgs, mut rep: Report, start: Option<()>) -> Result<()> {
    // 12–14. Portal, only when everything mandatory is verified.
    let mut launch: Option<String> = None;
    if start.is_some() {
        match find_static(cfg, &a.static_dir) {
            None => rep.block(
                "Governance Portal",
                "the web UI is not built (portal/dist/index.html not found)",
                "run `npm --prefix portal ci && npm --prefix portal run build`, then `eaeh setup` again",
            ),
            Some(dir) => match start_portal(cfg, a, &dir) {
                Ok(o) => {
                    let (st, verb) = match o {
                        PortalOutcome::Started(s) => (s, "started"),
                        PortalOutcome::Reused(s) => (s, "already running – reused"),
                    };
                    rep.ok(
                        "Governance Portal",
                        format!("running ({verb}), pid {}, http://{}", st.pid, st.bind),
                    );
                    rep.portal = Some(
                        json!({"url": format!("http://{}", st.bind), "pid": st.pid, "launch_url": st.launch_url}),
                    );
                    if a.open {
                        if open_in_browser(&st.launch_url) {
                            rep.items.push(Item {
                                area: "Browser".into(),
                                status: St::Ok,
                                detail: "opened in your default browser".into(),
                                fix: None,
                            });
                        } else {
                            rep.warn(
                                "Browser",
                                "could not open a browser",
                                "open the Portal URL manually",
                            );
                        }
                    }
                    launch = Some(st.launch_url);
                }
                Err(e) => rep.block(
                    "Governance Portal",
                    e.message,
                    "fix the reported problem and re-run `eaeh setup`",
                ),
            },
        }
    } else if !a.check && !a.no_portal {
        rep.add(
            "Governance Portal",
            St::Skipped,
            "not started: mandatory checks failed",
            None,
        );
    } else if a.no_portal {
        rep.add("Governance Portal", St::Skipped, "--no-portal", None);
    } else if let Some(st) = read_portal_state(cfg)
        && is_portal_process(st.pid)
        && http_alive(&st.bind)
    {
        rep.ok(
            "Governance Portal",
            format!("running, pid {}, http://{}", st.pid, st.bind),
        );
    } else {
        rep.warn("Governance Portal", "not running", "run `eaeh setup`");
    }

    let blocked = rep.blocked();
    let warnings = rep.items.iter().any(|i| i.status == St::Warning);
    let verdict = if blocked {
        "FAILED"
    } else if warnings {
        "SUCCESS (with warnings)"
    } else {
        "SUCCESS"
    };
    let mut text = format!(
        "EAEH Setup{}: {verdict}\n",
        if a.check { " (check, read-only)" } else { "" }
    );
    for i in &rep.items {
        let tag = match i.status {
            St::Ok => "ok      ",
            St::Repaired => "repaired",
            St::Warning => "WARNING ",
            St::Blocked => "FAILED  ",
            St::Skipped => "skipped ",
        };
        text.push_str(&format!("  [{tag}] {}: {}\n", i.area, i.detail));
    }
    let fixes: Vec<&Item> = rep
        .items
        .iter()
        .filter(|i| i.fix.is_some() && i.status != St::Ok)
        .collect();
    if !fixes.is_empty() {
        text.push_str("\nWhat to do:\n");
        for i in fixes {
            text.push_str(&format!("  • {}: {}\n", i.area, i.fix.as_deref().unwrap_or("")));
        }
    }
    if let Some(u) = &launch {
        text.push_str(&format!(
            "\nPortal URL: {}\nLaunch link (contains a one-time secret – do not share): {u}\nThe portal keeps running in the background. Stop it: `eaeh setup --stop-portal`. Restart: `eaeh setup`.\n",
            rep.portal.as_ref().and_then(|p| p["url"].as_str()).unwrap_or("")
        ));
    }
    dx::out(
        &json!({"status": if blocked {"failed"} else if warnings {"success_with_warnings"} else {"success"},
                "items": rep.items, "repairs": rep.repairs, "skills": rep.skills, "portal": rep.portal}),
        text.trim_end(),
    );
    if blocked {
        crate::NOT_READY.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    Ok(())
}
