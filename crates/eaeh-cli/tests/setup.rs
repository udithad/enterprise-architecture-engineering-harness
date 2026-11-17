//! `eaeh setup` end-to-end tests. Every test drives the real `eaeh` binary against its own
//! isolated installation (temporary data directory, EAEH_HOME and ports) with a real SQLite
//! database and, for the portal tests, a real portal process answering real HTTP. Nothing
//! here touches the developer's own installation or credentials. State that no CLI command
//! can produce (an expired token, a wrongly-roled principal) is written straight into the
//! test database, exactly as a corrupted installation would look.

use eaeh_store::sqlx::{self, ConnectOptions, Connection};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PHRASE: &str = "RESET EAEH";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

struct Env {
    dir: tempfile::TempDir,
    cfg: PathBuf,
    home: PathBuf,
    skills: PathBuf,
    ui: PathBuf,
}

impl Env {
    fn new() -> Self {
        Self::with_skills(&["api-error-handling", "hexagonal-architecture-design"])
    }

    fn with_skills(names: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let skills = p.join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        for n in names {
            copy_dir(&repo().join("config/skills").join(n), &skills.join(n));
        }
        let ui = p.join("ui");
        std::fs::create_dir_all(&ui).unwrap();
        std::fs::write(ui.join("index.html"), "<!doctype html><title>test portal</title>").unwrap();
        let r = repo();
        let cfg = p.join("eaeh.toml");
        std::fs::write(
            &cfg,
            format!(
                "[storage]\ndata_dir = \"{data}\"\n[definitions]\nprompts_dir = \"{r}/prompts\"\nagents_dir = \"{r}/config/agents\"\nworkflows_dir = \"{r}/workflows\"\npolicies_dir = \"{r}/config/policies\"\nskills_dir = \"{skills}\"\n[isolation]\ncontainer_enabled = false\n[execution]\nmode = \"external_ai\"\n[log]\nformat = \"pretty\"\nfilter = \"warn\"\n",
                data = p.join("data").display(),
                r = r.display(),
                skills = skills.display()
            ),
        )
        .unwrap();
        Env {
            home: p.join("home"),
            cfg,
            skills,
            ui,
            dir,
        }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_eaeh"));
        c.arg("--config")
            .arg(&self.cfg)
            .args(args)
            .env("EAEH_HOME", &self.home)
            .env_remove("EAEH_TOKEN")
            .env_remove("EAEH_PROFILE")
            .env_remove("EAEH_CONFIG")
            .env_remove("EAEH_PORTAL_DIR");
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }

    /// `eaeh --json setup …` (no portal unless the arguments say otherwise).
    fn setup(&self, extra: &[&str]) -> (i32, Value, String) {
        let ui = self.ui.display().to_string();
        let mut a = vec!["--json", "setup", "--static-dir", ui.as_str()];
        a.extend_from_slice(extra);
        let o = self.run(&a);
        let v = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
        (
            o.status.code().unwrap_or(-1),
            v,
            String::from_utf8_lossy(&o.stderr).to_string(),
        )
    }

    fn setup_np(&self, extra: &[&str]) -> (i32, Value, String) {
        let mut a = vec!["--no-portal"];
        a.extend_from_slice(extra);
        self.setup(&a)
    }

    fn json(&self, args: &[&str]) -> Value {
        let o = self.run(args);
        assert!(
            o.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }

    fn creds(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.home.join("credentials.json")).unwrap()).unwrap()
    }

    fn write_creds(&self, v: &Value) {
        std::fs::write(self.home.join("credentials.json"), serde_json::to_vec(v).unwrap()).unwrap();
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("data/eaeh.db")
    }

    fn audit_count(&self, action: &str) -> usize {
        self.json(&["--as", "admin", "audit", "list", "--limit", "500"])["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["action"] == action)
            .count()
    }

    fn all_tokens(&self) -> String {
        self.creds()["profiles"]
            .as_object()
            .unwrap()
            .values()
            .map(|p| p["token"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Run SQL against the installation's database (no EAEH process may be using it).
    fn sql(&self, db: &Path, stmt: &str) {
        sql_on(db, stmt);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn sql_on(db: &Path, stmt: &str) {
    rt().block_on(async {
        let mut c = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(db)
            .create_if_missing(false)
            .connect()
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(stmt.to_string()))
            .execute(&mut c)
            .await
            .unwrap();
        let _ = c.close().await;
    });
}

fn query_strings(db: &Path, stmt: &str) -> Vec<String> {
    rt().block_on(async {
        let mut c = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(db)
            .create_if_missing(false)
            .connect()
            .await
            .unwrap();
        let rows: Vec<(String,)> = sqlx::query_as(sqlx::AssertSqlSafe(stmt.to_string()))
            .fetch_all(&mut c)
            .await
            .unwrap();
        let _ = c.close().await;
        rows.into_iter().map(|r| r.0).collect()
    })
}

fn status_of<'a>(v: &'a Value, area: &str) -> Vec<&'a str> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["area"] == area)
        .map(|i| i["status"].as_str().unwrap())
        .collect()
}

fn detail_of(v: &Value, area: &str) -> String {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["area"] == area)
        .map(|i| {
            format!(
                "{} {}",
                i["detail"].as_str().unwrap(),
                i["fix"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Stops the background portal even when an assertion fails.
struct PortalGuard<'a>(&'a Env);
impl Drop for PortalGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.run(&["setup", "--stop-portal"]);
    }
}

fn http_raw(addr: &str, request: &str) -> String {
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn http_get(addr: &str, path: &str) -> String {
    http_raw(addr, &format!("GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n"))
}

fn expected_profiles(e: &Env) {
    let c = e.creds();
    for (profile, principal, role) in [
        ("admin", "admin", "admin"),
        ("developer", "dev", "operator"),
        ("approver", "approver", "approver"),
        ("steward", "steward", "knowledge_steward"),
    ] {
        let p = &c["profiles"][profile];
        assert_eq!(p["principal"], principal, "profile {profile}");
        assert_eq!(p["roles"], serde_json::json!([role]), "profile {profile}");
    }
    // The server agrees with the stored metadata, not just the file.
    let st = e.json(&["--json", "auth", "status"]);
    for p in st["profiles"].as_array().unwrap() {
        assert_eq!(p["valid"], true, "{p}");
    }
}

// 1. First-time setup ------------------------------------------------------------------

#[test]
fn first_time_setup_creates_identities_credentials_skills() {
    let e = Env::new();
    let (code, v, err) = e.setup_np(&[]);
    assert_eq!(code, 0, "{err}\n{v}");
    assert_eq!(
        v["status"], "success_with_warnings",
        "pending skills are a warning: {v}"
    );
    expected_profiles(&e);
    // Skills: both directories registered, pending approval, none auto-approved.
    assert_eq!(v["skills"]["registered"], 2, "{v}");
    assert_eq!(v["skills"]["pending_approval"], 2, "{v}");
    let list = e.json(&["--as", "admin", "skill", "list"]);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|s| s["status"] == "pending_approval"), "{list}");
    // Secrets never appear in output; the credential store is private.
    let o = e.run(&["setup", "--check"]);
    let shown = format!(
        "{}{}{err}{v}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    for t in e.all_tokens().lines() {
        assert!(!shown.contains(t), "a token was printed");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(e.home.join("credentials.json")), 0o600);
        assert_eq!(mode(e.home.clone()), 0o700);
    }
    // The run is audited and the chain is intact.
    assert!(e.audit_count("setup.run") >= 1);
    assert!(e.run(&["audit", "verify"]).status.success());
}

// 2. Repeat run: no duplication ---------------------------------------------------------

#[test]
fn repeated_setup_does_not_duplicate_anything() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    let creds_before = e.creds();
    let principals = e.audit_count("principal.create");
    let tokens = e.audit_count("token.issue");
    let skills = e.json(&["--as", "admin", "skill", "list"])["items"]
        .as_array()
        .unwrap()
        .len();
    let (code, v, err) = e.setup_np(&[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        e.audit_count("principal.create"),
        principals,
        "no principal re-created"
    );
    assert_eq!(e.audit_count("token.issue"), tokens, "no credential re-issued");
    assert_eq!(
        e.json(&["--as", "admin", "skill", "list"])["items"]
            .as_array()
            .unwrap()
            .len(),
        skills,
        "no duplicate skill registration"
    );
    assert_eq!(v["skills"]["registered"], 0, "{v}");
    assert_eq!(v["skills"]["already_registered"], 2, "{v}");
    assert!(
        v["repairs"].as_array().unwrap().is_empty(),
        "nothing to repair: {v}"
    );
    assert_eq!(
        e.creds(),
        creds_before,
        "working credentials are preserved byte for byte"
    );
}

// 3. Existing healthy environment: --check is read-only ---------------------------------

#[test]
fn check_on_healthy_environment_changes_nothing() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    let before = (
        e.creds(),
        e.audit_count("token.issue"),
        e.audit_count("principal.create"),
    );
    let (code, v, err) = e.setup(&["--check"]);
    assert_eq!(code, 0, "{err}\n{v}");
    for area in ["admin", "developer", "approver", "steward", "Project permissions"] {
        assert_eq!(status_of(&v, area), ["ok"], "{area}: {v}");
    }
    assert_eq!(e.creds(), before.0);
    assert_eq!(
        (e.audit_count("token.issue"), e.audit_count("principal.create")),
        (before.1, before.2)
    );
}

#[test]
fn check_on_missing_installation_reports_not_initialised_and_creates_nothing() {
    let e = Env::new();
    let (code, v, _) = e.setup(&["--check"]);
    assert_eq!(code, 7);
    assert_eq!(status_of(&v, "Database"), ["blocked"]);
    assert!(!e.db().exists(), "--check must not create the database");
}

// 4. Missing principals are created from an existing administrator ----------------------

#[test]
fn missing_principals_are_created_by_the_administrator() {
    let e = Env::new();
    // Only the administrator exists (the old manual first step).
    let tf = e.dir.path().join("admin.token");
    assert!(
        e.run(&[
            "admin",
            "bootstrap",
            "--tenant",
            "acme",
            "--project",
            "default",
            "--admin-id",
            "admin",
            "--token-file",
            tf.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        e.run(&[
            "auth",
            "login",
            "--profile",
            "admin",
            "--token-file",
            tf.to_str().unwrap()
        ])
        .status
        .success()
    );
    let (code, v, err) = e.setup_np(&[]);
    assert_eq!(code, 0, "{err}\n{v}");
    expected_profiles(&e);
    // The existing tenant is kept, not replaced.
    assert!(!detail_of(&v, "Tenant").contains("created"), "{v}");
    assert_eq!(e.audit_count("principal.create"), 3);
}

// 5. Incorrect profile → principal mapping ----------------------------------------------

#[test]
fn profile_mapped_to_wrong_principal_is_detected_and_repaired_only_on_request() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    // approver -> admin: the token is perfectly valid, the identity is wrong.
    let mut c = e.creds();
    c["profiles"]["approver"] = c["profiles"]["admin"].clone();
    e.write_creds(&c);
    let tokens = e.audit_count("token.issue");
    let (code, v, _) = e.setup_np(&[]);
    assert_eq!(code, 7, "{v}");
    assert!(detail_of(&v, "approver").contains("IDENTITY MISMATCH"), "{v}");
    assert_eq!(
        e.creds()["profiles"]["approver"]["principal"],
        "admin",
        "never silently overwritten"
    );
    assert_eq!(e.audit_count("token.issue"), tokens);
    // Explicit --repair replaces it with a token for the right principal.
    let (code, v, err) = e.setup_np(&["--repair"]);
    assert_eq!(code, 0, "{err}\n{v}");
    expected_profiles(&e);
    // The admin credential itself is untouched, and the replaced file was backed up.
    assert_eq!(e.creds()["profiles"]["admin"], c["profiles"]["admin"]);
    let backups: Vec<_> = std::fs::read_dir(e.home.join("backups")).unwrap().collect();
    assert!(!backups.is_empty());
}

#[test]
fn developer_can_never_end_up_as_admin() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    let mut c = e.creds();
    c["profiles"]["developer"] = c["profiles"]["admin"].clone();
    e.write_creds(&c);
    let (code, v, _) = e.setup_np(&[]);
    assert_eq!(code, 7);
    assert!(detail_of(&v, "developer").contains("expected 'dev'"), "{v}");
    assert_eq!(e.setup_np(&["--repair"]).0, 0);
    assert_eq!(
        e.creds()["profiles"]["developer"]["roles"],
        serde_json::json!(["operator"])
    );
}

// 6. Expired and revoked credentials -----------------------------------------------------

#[test]
fn revoked_and_expired_credentials_are_reissued() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    let c = e.creds();
    // Revoke the developer token through the real command.
    let tid = c["profiles"]["developer"]["token_id"].as_str().unwrap();
    assert!(e.run(&["--as", "admin", "token", "revoke", tid]).status.success());
    // Expire the steward token in the database (as time passing would).
    let sid = c["profiles"]["steward"]["token_id"].as_str().unwrap();
    e.sql(
        &e.db(),
        &format!("UPDATE api_tokens SET expires_at = '2020-01-01T00:00:00.000000Z' WHERE id = '{sid}'"),
    );
    let (code, v, err) = e.setup_np(&[]);
    assert_eq!(code, 0, "{err}\n{v}");
    assert_eq!(status_of(&v, "developer"), ["repaired"], "{v}");
    assert_eq!(status_of(&v, "steward"), ["repaired"], "{v}");
    assert_eq!(status_of(&v, "approver"), ["ok"]);
    expected_profiles(&e);
    let c2 = e.creds();
    assert_ne!(
        c2["profiles"]["developer"]["token_id"],
        c["profiles"]["developer"]["token_id"]
    );
    assert_eq!(
        c2["profiles"]["approver"], c["profiles"]["approver"],
        "valid credentials are preserved"
    );
}

// 7. Incorrect role assignment ------------------------------------------------------------

#[test]
fn principal_with_wrong_roles_blocks_setup_and_portal() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    e.sql(
        &e.db(),
        "UPDATE principals SET roles = '[\"admin\"]' WHERE id = 'dev'",
    );
    let port = format!("127.0.0.1:{}", free_port());
    let _g = PortalGuard(&e);
    let (code, v, _) = e.setup(&["--bind", &port]);
    assert_eq!(code, 7, "{v}");
    assert_eq!(status_of(&v, "developer"), ["blocked"]);
    assert!(detail_of(&v, "developer").contains("ADMIN privileges"), "{v}");
    assert_eq!(
        status_of(&v, "Governance Portal"),
        ["skipped"],
        "portal must not start: {v}"
    );
    assert!(std::net::TcpStream::connect(&port).is_err());
    // Roles are not changed implicitly.
    assert_eq!(
        query_strings(&e.db(), "SELECT roles FROM principals WHERE id = 'dev'"),
        ["[\"admin\"]"]
    );
}

// 8. Missing project permissions -----------------------------------------------------------

#[test]
fn missing_project_access_is_granted() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    e.sql(
        &e.db(),
        "UPDATE principals SET projects = '{\"type\":\"only\",\"ids\":[]}' WHERE id = 'approver'",
    );
    let (code, v, err) = e.setup_np(&[]);
    assert_eq!(code, 0, "{err}\n{v}");
    assert_eq!(status_of(&v, "approver"), ["repaired"], "{v}");
    assert_eq!(status_of(&v, "Project permissions"), ["ok"]);
    // And --check reports the problem without fixing it.
    e.sql(
        &e.db(),
        "UPDATE principals SET projects = '{\"type\":\"only\",\"ids\":[]}' WHERE id = 'steward'",
    );
    let (code, v, _) = e.setup(&["--check"]);
    assert_eq!(code, 7);
    assert_eq!(status_of(&v, "steward"), ["blocked"]);
}

// 9. Missing administrator credential ------------------------------------------------------

#[test]
fn missing_admin_credential_fails_safely_without_changes() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    let mut c = e.creds();
    c["profiles"].as_object_mut().unwrap().remove("admin");
    // Make a repair necessary, to prove nothing is attempted without an administrator.
    let tid = c["profiles"]["developer"]["token_id"]
        .as_str()
        .unwrap()
        .to_string();
    e.write_creds(&c);
    e.sql(
        &e.db(),
        &format!("UPDATE api_tokens SET revoked_at = '2024-01-01T00:00:00.000000Z' WHERE id = '{tid}'"),
    );
    let before = query_strings(&e.db(), "SELECT CAST(COUNT(*) AS TEXT) FROM api_tokens");
    let (code, v, _) = e.setup_np(&["--repair"]);
    assert_eq!(code, 7, "{v}");
    assert_eq!(status_of(&v, "admin"), ["blocked"]);
    let d = detail_of(&v, "admin");
    assert!(
        d.contains("auth login --profile admin") && d.contains("setup --reset"),
        "{d}"
    );
    assert_eq!(
        query_strings(&e.db(), "SELECT CAST(COUNT(*) AS TEXT) FROM api_tokens"),
        before,
        "no credential was invented"
    );
    assert!(e.creds()["profiles"].get("admin").is_none());
}

// 10 + 11. Reset with confirmation, backup and recovery -------------------------------------

#[test]
fn reset_needs_confirmation_backs_up_and_is_recoverable() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    assert!(
        e.run(&["--as", "admin", "project", "create", "--name", "precious"])
            .status
            .success()
    );

    // No confirmation: refused, nothing changes.
    let (code, _, err) = e.setup_np(&["--reset"]);
    assert_ne!(code, 0, "{err}");
    assert!(err.contains("not confirmed"), "{err}");
    assert_eq!(
        query_strings(&e.db(), "SELECT name FROM projects ORDER BY name"),
        ["default", "precious"]
    );
    // A wrong phrase is refused too.
    assert_ne!(e.setup_np(&["--reset", "--confirm-reset", "yes"]).0, 0);

    // Confirmed: backup first, then a fresh installation.
    let source_marker = e.dir.path().join("my-source.txt");
    std::fs::write(&source_marker, "user file").unwrap();
    let (code, v, err) = e.setup_np(&["--reset", "--confirm-reset", PHRASE]);
    assert_eq!(code, 0, "{err}\n{v}");
    assert!(err.contains("WARNING") && err.contains("LOST"), "{err}");
    assert_eq!(
        query_strings(&e.db(), "SELECT name FROM projects"),
        ["default"],
        "old data is gone"
    );
    expected_profiles(&e);
    assert!(
        source_marker.exists() && e.skills.join("api-error-handling/SKILL.md").exists(),
        "user files and skills are never deleted"
    );

    let backups: Vec<PathBuf> = std::fs::read_dir(e.home.join("backups"))
        .unwrap()
        .map(|d| d.unwrap().path())
        .filter(|p| p.join("MANIFEST.json").is_file())
        .collect();
    assert_eq!(backups.len(), 1);
    let b = &backups[0];
    // The backup is a real, complete, protected database.
    assert_eq!(
        query_strings(&b.join("eaeh.db"), "SELECT name FROM projects ORDER BY name"),
        ["default", "precious"]
    );
    assert_eq!(
        query_strings(&b.join("eaeh.db"), "PRAGMA integrity_check"),
        ["ok"]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for f in ["eaeh.db", "credentials.json", "MANIFEST.json"] {
            assert_eq!(
                std::fs::metadata(b.join(f)).unwrap().permissions().mode() & 0o777,
                0o600,
                "{f}"
            );
        }
        assert_eq!(std::fs::metadata(b).unwrap().permissions().mode() & 0o777, 0o700);
    }

    // Recovery: restore the backup (needs confirmation as well).
    assert_ne!(e.setup_np(&["--restore", b.to_str().unwrap()]).0, 0);
    let (code, v, err) = e.setup_np(&["--restore", b.to_str().unwrap(), "--confirm-reset", PHRASE]);
    assert_eq!(code, 0, "{err}\n{v}");
    assert_eq!(
        query_strings(&e.db(), "SELECT name FROM projects ORDER BY name"),
        ["default", "precious"]
    );
    expected_profiles(&e);
    assert!(e.run(&["audit", "verify"]).status.success());
}

#[test]
fn restore_refuses_a_tampered_backup() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    assert_eq!(e.setup_np(&["--reset", "--confirm-reset", PHRASE]).0, 0);
    let b = std::fs::read_dir(e.home.join("backups"))
        .unwrap()
        .map(|d| d.unwrap().path())
        .find(|p| p.join("MANIFEST.json").is_file())
        .unwrap();
    let mut bytes = std::fs::read(b.join("eaeh.db")).unwrap();
    let n = bytes.len() / 2;
    bytes[n] ^= 0xff;
    std::fs::write(b.join("eaeh.db"), bytes).unwrap();
    let (code, _, err) = e.setup_np(&["--restore", b.to_str().unwrap(), "--confirm-reset", PHRASE]);
    assert_ne!(code, 0);
    assert!(err.contains("does not match its recorded hash"), "{err}");
}

// 12–14. Skill discovery, change detection, pending approval -------------------------------

#[test]
fn skills_are_registered_pending_and_changes_are_detected() {
    let e = Env::new();
    let (_, v, _) = e.setup_np(&[]);
    assert_eq!(v["skills"]["discovered"], 2, "{v}");
    assert_eq!(v["skills"]["registered"], 2);
    assert_eq!(v["skills"]["pending_approval"], 2);
    assert!(
        detail_of(&v, "Skill approval").contains("NOT available to agents"),
        "unapproved skills must be reported as unavailable: {v}"
    );
    // The governance approvals exist (this is what the Portal lists).
    let pid = e.json(&["--as", "admin", "project", "list"])["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let apps = e.json(&[
        "--as",
        "admin",
        "approval",
        "list",
        "--project",
        &pid,
        "--status",
        "pending",
    ]);
    assert!(apps["items"].as_array().unwrap().len() >= 2, "{apps}");

    // New skill directory: picked up on the next run.
    copy_dir(
        &repo().join("config/skills/api-versioning-strategy"),
        &e.skills.join("api-versioning-strategy"),
    );
    let (_, v, _) = e.setup_np(&[]);
    assert_eq!(v["skills"]["registered"], 1, "{v}");
    assert_eq!(v["skills"]["already_registered"], 2, "{v}");
    assert_eq!(v["skills"]["pending_approval"], 3);

    // Changed content: a new version is registered pending; the old record is preserved.
    let f = e.skills.join("api-error-handling/SKILL.md");
    let mut t = std::fs::read_to_string(&f).unwrap();
    t.push_str("\n\nAdditional guidance added by a developer.\n");
    std::fs::write(&f, t).unwrap();
    let before = e.json(&["--as", "admin", "skill", "list"])["items"]
        .as_array()
        .unwrap()
        .len();
    let (_, v, _) = e.setup_np(&[]);
    assert_eq!(v["skills"]["changed"], 1, "{v}");
    assert_eq!(v["skills"]["pending_approval"], 4);
    let after = e.json(&["--as", "admin", "skill", "list"])["items"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(after, before + 1, "history preserved, new version added");
    // Running again does not register it twice.
    let (_, v, _) = e.setup_np(&[]);
    assert_eq!(v["skills"]["changed"], 0, "{v}");
    assert_eq!(
        e.json(&["--as", "admin", "skill", "list"])["items"]
            .as_array()
            .unwrap()
            .len(),
        after
    );
}

#[test]
fn invalid_skill_is_reported_not_registered() {
    let e = Env::new();
    std::fs::create_dir_all(e.skills.join("broken")).unwrap();
    std::fs::write(e.skills.join("broken/SKILL.md"), "no front matter at all").unwrap();
    let (code, v, _) = e.setup_np(&[]);
    assert_eq!(code, 0);
    assert_eq!(v["skills"]["invalid"], 1, "{v}");
    assert_eq!(v["skills"]["registered"], 2);
}

// 15–17. Portal --------------------------------------------------------------------------------

#[test]
fn portal_starts_in_background_is_reused_and_can_be_stopped() {
    let e = Env::new();
    let port = format!("127.0.0.1:{}", free_port());
    let _g = PortalGuard(&e);
    let (code, v, err) = e.setup(&["--bind", &port]);
    assert_eq!(code, 0, "{err}\n{v}");
    let pid = v["portal"]["pid"].as_u64().unwrap();
    assert_eq!(v["portal"]["url"], format!("http://{port}"));
    // Real HTTP: the web UI is served, and the launch link opens a session.
    let home = http_get(&port, "/");
    assert!(
        home.starts_with("HTTP/1.1 200") || home.starts_with("HTTP/1.0 200"),
        "{home}"
    );
    assert!(home.contains("test portal"));
    // setup returned but the portal keeps running.
    assert!(
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );

    // Already running: reused, not started twice.
    let (code, v2, err) = e.setup(&["--bind", &port]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v2["portal"]["pid"].as_u64().unwrap(), pid);
    assert!(detail_of(&v2, "Governance Portal").contains("reused"), "{v2}");

    // Stop, then it is really gone.
    let o = e.run(&["--json", "setup", "--stop-portal"]);
    assert!(o.status.success());
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        std::net::TcpStream::connect(&port).is_err(),
        "portal still listening"
    );
    let o = e.run(&["--json", "setup", "--stop-portal"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("not_running"));
}

#[test]
fn portal_runs_with_the_standard_identities() {
    let e = Env::new();
    let port = format!("127.0.0.1:{}", free_port());
    let _g = PortalGuard(&e);
    let (code, v, err) = e.setup(&["--bind", &port]);
    assert_eq!(code, 0, "{err}");
    let launch = v["portal"]["launch_url"].as_str().unwrap().to_string();
    let secret = launch.split("launch=").nth(1).unwrap().to_string();
    // Exchange the launch secret for a session cookie, as the browser does …
    let body = format!("{{\"launch\":\"{secret}\"}}");
    let out = http_raw(
        &port,
        &format!(
            "POST /portal/session HTTP/1.0\r\nHost: {port}\r\nOrigin: http://{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(out.contains(" 200 "), "{out}");
    assert!(out.contains("eaeh_portal="), "{out}");
    // … then read the session: who views, who decides, who operates.
    let out = http_raw(
        &port,
        &format!("GET /portal/session HTTP/1.0\r\nHost: {port}\r\nCookie: eaeh_portal={secret}\r\n\r\n"),
    );
    assert!(out.contains(" 200 "), "{out}");
    let j: Value = serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(j["viewer"]["principal"], "admin", "{j}");
    assert_eq!(j["approver"]["principal"], "approver", "{j}");
    assert_eq!(j["operator"]["principal"], "dev", "{j}");
    assert_eq!(j["steward"]["principal"], "steward", "{j}");
    assert_eq!(j["can_decide"], true);
    // A wrong secret is refused.
    let bad = http_raw(
        &port,
        &format!(
            "POST /portal/session HTTP/1.0\r\nHost: {port}\r\nOrigin: http://{port}\r\nContent-Type: application/json\r\nContent-Length: 17\r\n\r\n{{\"launch\":\"nope\"}}"
        ),
    );
    assert!(bad.contains(" 401 "), "{bad}");
}

#[test]
fn port_conflict_is_reported_and_not_mistaken_for_the_portal() {
    let e = Env::new();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = format!("127.0.0.1:{}", l.local_addr().unwrap().port());
    let (code, v, _) = e.setup(&["--bind", &port]);
    assert_eq!(code, 7, "{v}");
    assert!(
        detail_of(&v, "Governance Portal").contains("port conflict"),
        "{v}"
    );
    // Everything else was still set up (the conflict does not undo the installation).
    expected_profiles(&e);
    drop(l);
}

// 18. Failed initialisation and safe recovery ------------------------------------------------------

#[test]
fn failed_initialisation_changes_nothing_and_setup_recovers() {
    let e = Env::new();
    // The data directory cannot be created: its parent is a regular file.
    let blocker = e.dir.path().join("blocker");
    std::fs::write(&blocker, "x").unwrap();
    let good = std::fs::read_to_string(&e.cfg).unwrap();
    let bad = good.replace(
        &e.dir.path().join("data").display().to_string(),
        &blocker.join("data").display().to_string(),
    );
    std::fs::write(&e.cfg, bad).unwrap();
    let port = format!("127.0.0.1:{}", free_port());
    let (code, v, _) = e.setup(&["--bind", &port]);
    assert_eq!(code, 7, "{v}");
    assert_eq!(status_of(&v, "Database"), ["blocked"], "{v}");
    assert!(detail_of(&v, "Database").contains("re-run"), "{v}");
    assert!(
        !e.home.join("credentials.json").exists(),
        "no credentials from a failed setup"
    );
    assert!(
        std::net::TcpStream::connect(&port).is_err(),
        "no portal after a failed setup"
    );
    // Fix the cause and run again.
    std::fs::write(&e.cfg, good).unwrap();
    let (code, v, err) = e.setup_np(&[]);
    assert_eq!(code, 0, "{err}\n{v}");
    expected_profiles(&e);
}

#[test]
fn interrupted_reset_recovers_on_the_next_run() {
    // A reset that stopped after the database was removed leaves an empty installation:
    // the next run simply bootstraps again (the backup is still there).
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    assert_eq!(e.setup_np(&["--reset", "--confirm-reset", PHRASE]).0, 0);
    std::fs::remove_file(e.db()).unwrap();
    let _ = std::fs::remove_file(format!("{}-wal", e.db().display()));
    let _ = std::fs::remove_file(format!("{}-shm", e.db().display()));
    let (code, v, err) = e.setup_np(&[]);
    assert_eq!(code, 0, "{err}\n{v}");
    expected_profiles(&e);
}

// Option compatibility ---------------------------------------------------------------------------------

#[test]
fn incompatible_options_are_rejected() {
    let e = Env::new();
    assert_ne!(e.setup(&["--check", "--reset"]).0, 0);
    assert_ne!(
        e.setup(&["--bind", "0.0.0.0:8090"]).0,
        0,
        "the portal is loopback-only"
    );
}

// Regression: the portal reads as `dev`; a project registered after setup (or before it, under
// another name) must be reachable by the standard people, or the portal answers 403
// "principal 'dev' may not access the target of workflow.read".
#[test]
fn standard_people_get_access_to_every_project_of_the_tenant() {
    let e = Env::new();
    assert_eq!(e.setup_np(&[]).0, 0);
    assert!(
        e.run(&["--as", "admin", "project", "create", "--name", "second"])
            .status
            .success()
    );
    let (code, v, _) = e.setup(&["--check"]);
    assert_eq!(code, 7, "{v}");
    assert_eq!(status_of(&v, "developer"), ["blocked"]);
    let (code, v, err) = e.setup_np(&[]);
    assert_eq!(code, 0, "{err}\n{v}");
    for who in ["dev", "approver", "steward"] {
        let ids = query_strings(
            &e.db(),
            &format!("SELECT projects FROM principals WHERE id = '{who}'"),
        );
        let n = ids[0].matches('-').count() / 4; // uuids contain four dashes
        assert_eq!(n, 2, "{who}: {ids:?}");
    }
    // The operator identity can now read the second project's workflows.
    let second = e.json(&["--as", "admin", "project", "list"])["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "second")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let o = e.run(&["--as", "developer", "approval", "list", "--project", &second]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
