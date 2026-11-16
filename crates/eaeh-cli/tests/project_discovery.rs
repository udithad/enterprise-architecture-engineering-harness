//! Governance Portal project discovery (docs/implementation/EAEH-GOVERNANCE-PORTAL-PROJECT-DISCOVERY-FIX.md).
//!
//! Real binary, isolated temporary installations: `eaeh project init` → persisted → listed by
//! the API and the portal read models in every lifecycle state, without restarting the
//! portal; the portal and the CLI report the same persistence; a portal on another
//! installation warns that the folder's project is not registered there; a port collision
//! is explained. The model is the deterministic replay fixture (NOT AI); approvals are
//! decided by the fixture's human approver identity, as in the other CLI tests.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

struct Env {
    _d: tempfile::TempDir,
    dir: PathBuf,
    cfg: PathBuf,
    home: PathBuf,
}

fn env() -> Env {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap();
    let r = repo();
    let cfg = format!(
        r#"[storage]
data_dir = "{dir}/data"
[definitions]
prompts_dir = "{r}/prompts"
agents_dir = "{r}/config/agents"
workflows_dir = "{r}/workflows"
policies_dir = "{r}/config/policies"
skills_dir = "{r}/config/skills"
[isolation]
container_enabled = false
[log]
format = "pretty"
filter = "warn"
[execution]
mode = "deterministic"
[execution.coordinator]
auto_start = false
claude_path = "{dir}/no-claude"
[providers.replay]
enabled = true
cassette = "{r}/examples/simple-springboot-api/replay-cassette.json"
"#,
        dir = dir.display(),
        r = r.display()
    );
    std::fs::write(dir.join("eaeh.toml"), cfg).unwrap();
    Env {
        cfg: dir.join("eaeh.toml"),
        home: dir.join("home"),
        dir,
        _d: d,
    }
}

fn workspace(e: &Env, name: &str) -> PathBuf {
    let ws = e.dir.join(name);
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::fs::write(ws.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    for f in ["BRD-Simple-Spring-Boot-API.md", "PROJECT-BOOTSTRAP.md"] {
        std::fs::copy(repo().join("examples/simple-springboot-api").join(f), ws.join(f)).unwrap();
    }
    ws
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}
impl Out {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or(Value::Null)
    }
    fn all(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

fn cmd(e: &Env, cwd: &Path) -> Command {
    let mut c = Command::new(assert_cmd::cargo::cargo_bin("eaeh"));
    c.current_dir(cwd)
        .arg("--config")
        .arg(&e.cfg)
        .env_remove("EAEH_TOKEN")
        .env_remove("EAEH_PROFILE")
        .env_remove("EAEH_INSTALLATION_LABEL")
        // Periodic skill discovery every second (default 30 s) so tests need not wait.
        .env("EAEH_SKILL_DISCOVERY_INTERVAL_SECS", "1")
        .env("EAEH_HOME", &e.home);
    c
}

fn run(e: &Env, cwd: &Path, args: &[&str]) -> Out {
    let o = cmd(e, cwd).args(args).output().unwrap();
    Out {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into(),
        stderr: String::from_utf8_lossy(&o.stderr).into(),
    }
}

fn admin_login(e: &Env) {
    assert_eq!(run(e, &e.dir, &["init"]).code, 0);
    let tf = e.dir.join("admin.token");
    let t = tf.to_str().unwrap();
    let o = run(
        e,
        &e.dir,
        &[
            "admin",
            "bootstrap",
            "--tenant",
            "acme",
            "--project",
            "platform",
            "--admin-id",
            "admin",
            "--token-file",
            t,
        ],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    let o = run(
        e,
        &e.dir,
        &["auth", "login", "--profile", "admin", "--token-file", t],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    std::fs::remove_file(&tf).unwrap();
}

fn project_init(e: &Env, ws: &Path, name: &str) -> Out {
    run(
        e,
        ws,
        &[
            "--json",
            "project",
            "init",
            "--name",
            name,
            "--description",
            "Simple REST API for local development",
            "--language",
            "Java 21",
            "--framework",
            "Spring Boot 3.x",
            "--build-tool",
            "Maven",
            "--constraint",
            "Products in memory; no database",
            "--brd",
            "BRD-Simple-Spring-Boot-API.md",
            "--knowledge",
            "PROJECT-BOOTSTRAP.md",
            "--developer",
            "dev",
            "--steward",
            "steward",
            "--approver",
            "approver",
            "--mode",
            "deterministic",
            "--client",
            "claude-code",
            "--workflow",
            "requirements.analysis",
            "--workflow",
            "engineering.design",
            "--store-profiles",
        ],
    )
}

// ------------------------------------------------------------------ portal process + HTTP

struct Portal {
    child: Child,
    port: u16,
    cookie: String,
    stderr: PathBuf,
}

impl Drop for Portal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_portal(e: &Env, cwd: &Path, args: &[&str]) -> Portal {
    let stderr = e
        .dir
        .join(format!("portal-{}.err", std::process::id() ^ rand_suffix()));
    let mut child = cmd(e, cwd)
        .arg("portal")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = line.trim().to_string();
    assert!(
        url.contains("?launch="),
        "no launch URL: {url:?}\n{}",
        std::fs::read_to_string(&stderr).unwrap_or_default()
    );
    let port: u16 = url
        .trim_start_matches("http://127.0.0.1:")
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let secret = url.split("launch=").nth(1).unwrap().to_string();
    let mut p = Portal {
        child,
        port,
        cookie: String::new(),
        stderr,
    };
    let (s, h, _) = p.http("POST", "/portal/session", Some(json!({"launch": secret})));
    assert_eq!(s, 200);
    let c = h
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("set-cookie:"))
        .unwrap();
    p.cookie = c
        .split_once(':')
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap()
        .trim()
        .to_string();
    p
}

fn rand_suffix() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
}

impl Portal {
    fn http(&self, method: &str, path: &str, body: Option<Value>) -> (u16, String, Value) {
        let host = format!("127.0.0.1:{}", self.port);
        let mut s = TcpStream::connect(&host).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let b = body.map(|b| b.to_string()).unwrap_or_default();
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nOrigin: http://{host}\r\nCookie: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
            self.cookie,
            b.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut raw = vec![];
        s.read_to_end(&mut raw).unwrap();
        let raw = String::from_utf8_lossy(&raw).to_string();
        let (head, rest) = raw.split_once("\r\n\r\n").unwrap();
        let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
            dechunk(rest)
        } else {
            rest.to_string()
        };
        (
            status,
            head.to_string(),
            serde_json::from_str(&body).unwrap_or(Value::Null),
        )
    }
    fn get(&self, path: &str) -> Value {
        let (s, _, v) = self.http("GET", path, None);
        assert_eq!(s, 200, "{path}: {v}");
        v
    }
    fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

fn dechunk(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some((len, tail)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(len.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(&tail[..n]);
        rest = &tail[n + 2..];
    }
    out
}

fn row<'a>(dash: &'a Value, name: &str) -> Option<&'a Value> {
    dash["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
}

fn wait_for(p: &Portal, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let d = p.get("/api/v1/dashboard");
        if ok(&d) {
            return d;
        }
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "timed out waiting for {what}: {d}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn cli_registered_projects_are_discoverable_in_every_state_without_restarting_the_portal() {
    let e = env();
    admin_login(&e);
    // The replay fixture cites the BRD by the project name (`simple-springboot-api-brd/…`).
    let ws_a = workspace(&e, "simple-springboot-api");
    let o = project_init(&e, &ws_a, "simple-springboot-api");
    assert!(matches!(o.code, 0 | 7), "{}", o.all()); // 7 = NOT READY (skills pending)

    // Persisted in the registry the CLI uses.
    let list = run(&e, &ws_a, &["project", "list"]).json();
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "simple-springboot-api"),
        "{list}"
    );
    let cli_data_dir = list["installation"]["data_dir"].as_str().unwrap().to_string();

    let p = start_portal(
        &e,
        &ws_a,
        &[
            "--as",
            "developer",
            "--approver",
            "approver",
            "--bind",
            "127.0.0.1:0",
        ],
    );
    // Same persistence as the CLI; no mismatch warning for this folder.
    let s = p.get("/portal/session");
    assert_eq!(s["installation"]["data_dir"].as_str().unwrap(), cli_data_dir);
    assert!(!p.stderr().contains("NOT registered"), "{}", p.stderr());

    // Listed before any workflow: status Ready, workspace, stack, description, workflows.
    let projects = p.get("/api/v1/projects");
    assert!(
        projects["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == "simple-springboot-api")
    );
    let d = p.get("/api/v1/dashboard");
    let a = row(&d, "simple-springboot-api")
        .expect("listed without a workflow")
        .clone();
    assert_eq!(a["status"], "ready", "{a}");
    assert_eq!(a["current_workflow"], Value::Null);
    assert_eq!(a["workspace"]["root"].as_str().unwrap(), ws_a.to_str().unwrap());
    assert_eq!(a["stack"]["language"], "Java 21");
    assert_eq!(a["description"], "Simple REST API for local development");
    assert_eq!(
        a["registered_workflows"],
        json!(["requirements.analysis", "engineering.design"])
    );
    let pid = a["id"].as_str().unwrap().to_string();
    let ov = p.get(&format!("/api/v1/projects/{pid}/overview"));
    assert_eq!(ov["status"], "ready");
    assert_eq!(ov["workflows"], json!([]));
    assert_eq!(ov["manifest"]["brd"]["path"], "BRD-Simple-Spring-Boot-API.md");
    assert_eq!(
        ov["readiness"]["ready"], false,
        "skills not approved yet: {}",
        ov["readiness"]
    );

    // Skills approved by the human approver; the design run started from the CLI (another
    // process) appears in the running portal as waiting for approval.
    let o = run(
        &e,
        &ws_a,
        &["--as", "approver", "approval", "review", "--all", "--yes"],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    let o = run(
        &e,
        &ws_a,
        &["--json", "workflow", "run", "design", "--yes", "--no-worker"],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    let wid = o.json()["instance"].as_str().unwrap().to_string();
    let d = wait_for(&p, "waiting approval", |d| {
        row(d, "simple-springboot-api")
            .map(|r| r["status"] == "waiting_approval")
            .unwrap_or(false)
    });
    let a = row(&d, "simple-springboot-api").unwrap();
    assert_eq!(a["current_workflow"]["id"], json!(wid));
    assert_eq!(a["current_workflow"]["definition_id"], "engineering.design");
    assert_eq!(a["current_workflow"]["state"], "approve_requirements");
    assert_eq!(a["pending_approvals"], 1);
    let pending = d["pending_approvals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["project_name"] == "simple-springboot-api")
        .unwrap()
        .clone();
    assert_eq!(pending["gate"], "approve_requirements");
    assert!(
        d["active"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["workflow"]["id"] == json!(wid))
    );
    let ov = p.get(&format!("/api/v1/projects/{pid}/overview"));
    assert!(
        ov["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["id"] == json!(wid))
    );
    assert!(
        ov["approvals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == pending["id"])
    );

    // The approver opens the requirements document and approves it in the portal; the
    // dashboard follows the run to the HLD gate.
    let aid = pending["id"].as_str().unwrap();
    let doc = p.get(&format!("/api/v1/approvals/{aid}/document"));
    let hash = doc["approval"]["artifact_hash"].as_str().unwrap().to_string();
    assert!(
        doc["document"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == "requirements.md" && f["content"].as_str().unwrap().contains("REQ-"))
    );
    let (st, _, v) = p.http(
        "POST",
        &format!("/api/v1/approvals/{aid}/decision"),
        Some(json!({"decision": "approve", "comments": "requirements reviewed", "artifact_hash": hash})),
    );
    assert_eq!(st, 200, "{v}");
    let d = wait_for(&p, "HLD gate", |d| {
        row(d, "simple-springboot-api")
            .map(|r| r["current_workflow"]["state"] == "approve_hld")
            .unwrap_or(false)
    });
    assert_eq!(
        row(&d, "simple-springboot-api").unwrap()["status"],
        "waiting_approval"
    );
    let ov = p.get(&format!("/api/v1/projects/{pid}/overview"));
    let req = ov["documents"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["gate"] == "approve_requirements")
        .unwrap()
        .clone();
    assert_eq!(req["materialization"]["state"], "verified", "{req}");

    // A project registered while the portal runs appears without a restart.
    let ws_b = workspace(&e, "simple-springboot-api-b");
    let o = project_init(&e, &ws_b, "simple-springboot-api-b");
    assert!(matches!(o.code, 0 | 7), "{}", o.all());
    let d = wait_for(&p, "new project", |d| row(d, "simple-springboot-api-b").is_some());
    assert_eq!(row(&d, "simple-springboot-api-b").unwrap()["status"], "ready");
    assert_eq!(
        row(&d, "simple-springboot-api").unwrap()["current_workflow"]["state"],
        "approve_hld"
    );

    // A second portal on the same port explains the collision.
    let o = run(
        &e,
        &ws_a,
        &[
            "portal",
            "--as",
            "developer",
            "--bind",
            &format!("127.0.0.1:{}", p.port),
        ],
    );
    assert_ne!(o.code, 0);
    assert!(
        o.stderr.contains("already in use") && o.stderr.contains("demo-portal"),
        "{}",
        o.stderr
    );
}

#[test]
fn a_portal_on_another_installation_warns_that_the_folder_project_is_not_there() {
    // Installation 1 owns the project in this folder …
    let e1 = env();
    admin_login(&e1);
    let ws = workspace(&e1, "simple-springboot-api-b");
    assert!(matches!(
        project_init(&e1, &ws, "simple-springboot-api-b").code,
        0 | 7
    ));
    // … a portal started in that folder on installation 2 (like the DEMO) warns and shows
    // neither the project nor its data.
    let e2 = env();
    admin_login(&e2);
    let p = start_portal(&e2, &ws, &["--as", "admin", "--bind", "127.0.0.1:0"]);
    let err = p.stderr();
    assert!(err.contains("NOT registered in this EAEH installation"), "{err}");
    assert!(
        err.contains("simple-springboot-api-b")
            && err.contains(&e2.dir.join("data").canonicalize().unwrap().display().to_string()),
        "{err}"
    );
    let d = p.get("/api/v1/dashboard");
    assert!(row(&d, "simple-springboot-api-b").is_none());
    assert_eq!(
        p.get("/portal/session")["installation"]["data_dir"]
            .as_str()
            .unwrap(),
        e2.dir.join("data").canonicalize().unwrap().to_str().unwrap()
    );
    drop(p);
    // The launcher's label (scripts/demo-portal.sh sets DEMO) reaches the banner and the UI.
    let mut c = cmd(&e2, &e2.dir);
    c.env("EAEH_INSTALLATION_LABEL", "DEMO – test");
    let mut child = c
        .args(["portal", "--as", "admin", "--bind", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let _ = child.kill();
    let mut err = String::new();
    child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    let _ = child.wait();
    assert!(err.contains("installation: DEMO – test"), "{err}");
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}

/// Skill onboarding end to end (SKILL-ONBOARDING-PLAN), real binary: a skill directory
/// copied into the skills folder while `eaeh portal` runs is discovered without a restart,
/// shown with its nested files, approved in the portal, and a later change to a nested file
/// becomes a new pending version that is rejected with comments – the approved version stays
/// in force.
#[test]
fn skill_directories_copied_in_are_discovered_reviewed_and_versioned_through_the_portal() {
    let e = env();
    // Own skills folder: the shipped single-file skills (needed by project init) only.
    let skills = e.dir.join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    for f in std::fs::read_dir(repo().join("config/skills")).unwrap() {
        let f = f.unwrap();
        if f.file_type().unwrap().is_file() {
            std::fs::copy(f.path(), skills.join(f.file_name())).unwrap();
        }
    }
    let cfg = std::fs::read_to_string(&e.cfg).unwrap();
    let cfg = cfg
        .lines()
        .map(|l| {
            if l.starts_with("skills_dir") {
                format!("skills_dir = \"{}\"", skills.display())
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&e.cfg, cfg).unwrap();
    admin_login(&e);
    let ws = workspace(&e, "simple-springboot-api");
    assert!(matches!(
        project_init(&e, &ws, "simple-springboot-api").code,
        0 | 7
    ));

    // `cmd` runs every process with a 1 s skill discovery interval.
    let p = start_portal(
        &e,
        &ws,
        &[
            "--as",
            "developer",
            "--approver",
            "approver",
            "--bind",
            "127.0.0.1:0",
        ],
    );

    // Copy a skill directory with nested folders in; it appears pending without a restart.
    let dir = skills.join("api-error-handling");
    copy_tree(&repo().join("config/skills/api-error-handling"), &dir);
    let find = |cat: &Value, reference: &str| {
        cat["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["reference"] == reference)
            .cloned()
    };
    let t = Instant::now();
    let item = loop {
        let cat = p.get("/api/v1/skills/catalog");
        if let Some(i) = find(&cat, "api-error-handling@1.0.0") {
            break i;
        }
        assert!(t.elapsed() < Duration::from_secs(30), "not discovered: {cat}");
        std::thread::sleep(Duration::from_millis(300));
    };
    assert_eq!(item["registration"]["status"], "pending_approval");
    assert_eq!(item["registration"]["registered_by"], "system:skill-discovery");
    assert_eq!(item["package"]["file_count"], 6);
    let f = p.get("/api/v1/skills/api-error-handling/1.0.0/files");
    let nested = f["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["path"] == "references/http/status-mapping.md")
        .unwrap()
        .clone();
    assert!(nested["content"].as_str().unwrap().contains("RESOURCE_NOT_FOUND"));
    // Approve it in the portal.
    let aid = item["registration"]["approval_id"].as_str().unwrap();
    let doc = p.get(&format!("/api/v1/approvals/{aid}/document"));
    let (s, _, v) = p.http("POST", &format!("/api/v1/approvals/{aid}/decision"), Some(json!({"decision": "approve", "comments": "reviewed every file", "artifact_hash": doc["skill"]["digest"]})));
    assert_eq!(s, 200, "{v}");

    // Change a nested file ⇒ 1.0.1 pending (no restart), compared with 1.0.0 in force.
    std::fs::write(
        dir.join("references/http/status-mapping.md"),
        "# HTTP status mapping\n\n| Invalid input | 422 |\n",
    )
    .unwrap();
    let t = Instant::now();
    let item = loop {
        let cat = p.get("/api/v1/skills/catalog");
        if let Some(i) = find(&cat, "api-error-handling@1.0.1") {
            break i;
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "change not discovered: {cat}"
        );
        std::thread::sleep(Duration::from_millis(300));
    };
    assert_eq!(item["registration"]["status"], "pending_approval");
    let f = p.get("/api/v1/skills/api-error-handling/1.0.1/files");
    assert_eq!(f["in_force_version"], "1.0.0");
    assert_eq!(
        f["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["path"] == "references/http/status-mapping.md")
            .unwrap()["change"],
        "modified"
    );
    // Reject with comments.
    let aid = item["registration"]["approval_id"].as_str().unwrap();
    let (s, _, v) = p.http("POST", &format!("/api/v1/approvals/{aid}/decision"), Some(json!({"decision": "reject", "comments": "422 is not allowed by the API standard", "artifact_hash": f["digest"]})));
    assert_eq!(s, 200, "{v}");
    let cat = p.get("/api/v1/skills/catalog");
    assert_eq!(
        find(&cat, "api-error-handling@1.0.1").unwrap()["registration"]["status"],
        "rejected"
    );
    assert_eq!(find(&cat, "api-error-handling@1.0.0").unwrap()["in_force"], true);
    // The CLI sees the same: the approved version is the one agents use.
    let o = run(&e, &ws, &["--json", "skill", "pending"]);
    let layer = o.json()["layer_skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["reference"] == "api-error-handling@^1")
        .unwrap()
        .clone();
    assert_eq!(
        layer["state"],
        json!({"state": "approved", "version": "1.0.0"}),
        "{layer}"
    );
}
