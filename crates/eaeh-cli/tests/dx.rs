//! Developer-experience CLI tests (real binary): identity profiles and token lifecycle,
//! one-command bootstrap (repeatable), Claude Code files, approver review queue, readiness
//! and prerequisite diagnostics, deterministic onboarding through the requirements gate,
//! error hints and the MCP server.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    ws: PathBuf,
}

fn env(replay: bool) -> Env {
    env_mode(replay, None)
}

/// Like [`env`], with an explicit `[execution] mode`.
fn env_mode(replay: bool, mode: Option<&str>) -> Env {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap();
    let r = repo();
    let mut cfg = format!(
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
"#,
        dir = dir.display(),
        r = r.display()
    );
    if let Some(m) = mode {
        cfg.push_str(&format!("[execution]\nmode = \"{m}\"\n"));
        // Tests never start a background worker or run the real Claude Code (that would use
        // the developer's subscription): no auto-start, and a client path that does not exist.
        cfg.push_str(&format!(
            "[execution.coordinator]\nauto_start = false\nclaude_path = \"{}/no-claude\"\n",
            dir.display()
        ));
    }
    if replay {
        cfg.push_str(&format!(
            "[providers.replay]\nenabled = true\ncassette = \"{}/examples/simple-springboot-api/replay-cassette.json\"\n",
            r.display()
        ));
    }
    std::fs::write(dir.join("eaeh.toml"), cfg).unwrap();
    let ws = dir.join("simple-springboot-api");
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::fs::write(ws.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        ws.join(".git/config"),
        "[remote \"origin\"]\n\turl = https://example.invalid/simple-springboot-api.git\n",
    )
    .unwrap();
    for f in ["BRD-Simple-Spring-Boot-API.md", "PROJECT-BOOTSTRAP.md"] {
        std::fs::copy(r.join("examples/simple-springboot-api").join(f), ws.join(f)).unwrap();
    }
    Env {
        cfg: dir.join("eaeh.toml"),
        home: dir.join("home"),
        ws,
        dir,
        _d: d,
    }
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
    fn err(&self) -> Value {
        let s = &self.stderr;
        let start = s.find("{\n  \"error\"").unwrap_or(0);
        serde_json::from_str(&s[start..]).unwrap_or(Value::Null)
    }
    fn all(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

fn run(e: &Env, cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> Out {
    let mut c = Command::new(assert_cmd::cargo::cargo_bin("eaeh"));
    c.current_dir(cwd)
        .arg("--config")
        .arg(&e.cfg)
        .args(args)
        .env_remove("EAEH_TOKEN")
        .env_remove("EAEH_PROFILE")
        .env("EAEH_HOME", &e.home);
    for (k, v) in envs {
        c.env(k, v);
    }
    let o = c.output().unwrap();
    Out {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into(),
        stderr: String::from_utf8_lossy(&o.stderr).into(),
    }
}

fn admin_login(e: &Env) -> String {
    assert_eq!(run(e, &e.dir, &["init"], &[]).code, 0);
    let tf = e.dir.join("admin.token");
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
            tf.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    let token = std::fs::read_to_string(&tf).unwrap();
    let o = run(
        e,
        &e.dir,
        &[
            "auth",
            "login",
            "--profile",
            "admin",
            "--token-file",
            tf.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    assert!(!o.all().contains(token.trim()), "token must never be displayed");
    std::fs::remove_file(&tf).unwrap();
    token
}

fn init(e: &Env, provider: &str) -> Out {
    run(
        e,
        &e.ws,
        &[
            "--json",
            "project",
            "init",
            "--name",
            "simple-springboot-api",
            "--description",
            "Simple REST API",
            "--language",
            "Java 21",
            "--framework",
            "Spring Boot 3.x",
            "--build-tool",
            "Maven",
            "--constraint",
            "No database",
            "--constraint",
            "No Spring Security",
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
            "--provider",
            provider,
            // The pre-ADR-0025 scope of these tests (delivery is covered in external_ai.rs).
            "--workflow",
            "requirements.analysis",
            "--workflow",
            "engineering.design",
            "--client",
            "claude-code",
            "--store-profiles",
        ],
        &[],
    )
}

#[test]
fn identity_profiles_token_lifecycle_and_diagnostics() {
    let e = env(false);
    let token = admin_login(&e);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let f = e.home.join("credentials.json");
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(
            std::fs::metadata(&e.home).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let st = run(&e, &e.dir, &["--json", "auth", "status"], &[]);
    let p = &st.json()["profiles"][0];
    assert_eq!(
        (
            p["profile"].as_str(),
            p["valid"].as_bool(),
            p["roles"][0].as_str()
        ),
        (Some("admin"), Some(true), Some("admin"))
    );
    assert!(!st.all().contains(token.trim()));

    // Plaintext tokens are only written to a file or the store, never printed by default.
    let o = run(&e, &e.dir, &["token", "issue", "--label", "x"], &[]);
    assert_eq!(o.code, 1);
    assert!(
        o.err()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--token-file")
    );

    // Renew: the old token stops working, the profile keeps working.
    let o = run(&e, &e.dir, &["auth", "renew", "--profile", "admin"], &[]);
    assert_eq!(o.code, 0, "{}", o.all());
    let o = run(&e, &e.dir, &["project", "list"], &[("EAEH_TOKEN", token.trim())]);
    assert_eq!(o.code, 3);
    assert!(
        o.err()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("EAEH_TOKEN is invalid"),
        "{}",
        o.all()
    );
    assert!(
        o.err()["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("eaeh auth status")
    );
    assert_eq!(run(&e, &e.dir, &["project", "list"], &[]).code, 0);

    // A stored token that expired: precise diagnosis from local metadata (no server oracle).
    let f = e.home.join("credentials.json");
    let mut store: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    let saved = store.clone();
    store["profiles"]["admin"]["expires_at"] = json!("2020-01-01T00:00:00Z");
    store["profiles"]["admin"]["token"] = json!(format!("eaeh_{}", "0".repeat(64)));
    std::fs::write(&f, serde_json::to_vec(&store).unwrap()).unwrap();
    let o = run(&e, &e.dir, &["project", "list"], &[]);
    assert_eq!(o.code, 3);
    assert!(
        o.err()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("expired at 2020-01-01"),
        "{}",
        o.all()
    );
    std::fs::write(&f, serde_json::to_vec(&saved).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        let o = run(&e, &e.dir, &["project", "list"], &[]);
        assert!(
            o.err()["error"]["message"]
                .as_str()
                .unwrap()
                .contains("chmod 600"),
            "{}",
            o.all()
        );
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    // Logout with revocation; afterwards there are no credentials (with the fix).
    assert_eq!(
        run(
            &e,
            &e.dir,
            &["auth", "logout", "--profile", "admin", "--revoke"],
            &[]
        )
        .code,
        0
    );
    let o = run(&e, &e.dir, &["project", "list"], &[]);
    assert_eq!(o.code, 3);
    assert!(
        o.err()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("eaeh auth login --profile"),
        "{}",
        o.all()
    );
}

#[test]
fn bootstrap_is_one_command_repeatable_and_generates_claude_code_files() {
    let e = env(false);
    let admin_token = admin_login(&e);
    std::fs::write(e.ws.join("CLAUDE.md"), "# Team notes\n\nKeep this line.\n").unwrap();
    let o = init(&e, "none");
    assert_eq!(
        o.code,
        7,
        "not ready yet (skills pending, no provider): {}",
        o.all()
    );
    let v = o.json();
    let actions = v["actions"].to_string();
    for s in [
        "created",
        "BRD simple-springboot-api-brd/brd-simple-spring-boot-api@v1",
        "registered",
        "manifest written",
    ] {
        assert!(actions.contains(s), "{s}: {actions}");
    }
    let project = v["project"].as_str().unwrap().to_string();
    // Manifest: stack, constraints, repository association, roles.
    let m = std::fs::read_to_string(e.ws.join(".eaeh/project.toml")).unwrap();
    for s in [
        "language = \"Java 21\"",
        "framework = \"Spring Boot 3.x\"",
        "build_tool = \"Maven\"",
        "No Spring Security",
        "git_remote = \"https://example.invalid/simple-springboot-api.git\"",
        "approvers = [\"approver\"]",
    ] {
        assert!(m.contains(s), "{s}: {m}");
    }
    // Claude Code: instructions (existing content kept) and MCP server – no secrets.
    let claude = std::fs::read_to_string(e.ws.join("CLAUDE.md")).unwrap();
    assert!(claude.starts_with("# Team notes") && claude.contains("Keep this line."));
    assert!(claude.contains("eaeh_activity_run") && claude.contains("Approvals are human-only"));
    let mcp: Value = serde_json::from_str(&std::fs::read_to_string(e.ws.join(".mcp.json")).unwrap()).unwrap();
    let srv = &mcp["mcpServers"]["eaeh"];
    assert_eq!(srv["env"]["EAEH_PROFILE"], "developer");
    assert!(
        srv["args"].to_string().contains("\"mcp\"") && srv["args"].to_string().contains("--project-root")
    );
    let store = std::fs::read_to_string(e.home.join("credentials.json")).unwrap();
    for f in ["CLAUDE.md", ".mcp.json", ".eaeh/project.toml"] {
        let body = std::fs::read_to_string(e.ws.join(f)).unwrap();
        assert!(
            !body.contains(admin_token.trim()) && !body.contains("eaeh_0") && !body.contains("\"token\""),
            "{f} leaks a credential"
        );
        for line in store.lines().filter(|l| l.contains("\"token\":")) {
            let t = line.split('"').nth(3).unwrap();
            assert!(!body.contains(t), "{f} contains a stored token");
        }
    }
    // Profiles for the separate responsibilities; developer becomes the default.
    let st = run(&e, &e.dir, &["--json", "auth", "status"], &[]).json();
    let names: Vec<(&str, &str)> = st["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["profile"].as_str().unwrap(), p["roles"][0].as_str().unwrap()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("admin", "admin"),
            ("approver", "approver"),
            ("developer", "operator"),
            ("steward", "knowledge_steward")
        ]
    );
    assert!(st["profiles"][2]["default"].as_bool().unwrap());

    // Repeat: same project, nothing duplicated, CLAUDE.md section not duplicated.
    let o = init(&e, "none");
    assert_eq!(o.code, 7);
    let v = o.json();
    assert_eq!(v["project"], project);
    let actions = v["actions"].to_string();
    assert!(
        actions.contains("already registered")
            && actions.contains("unchanged")
            && actions.contains("already_registered")
            && !actions.contains("created"),
        "{actions}"
    );
    let claude = std::fs::read_to_string(e.ws.join("CLAUDE.md")).unwrap();
    assert_eq!(claude.matches("<!-- eaeh:begin").count(), 1);
    // Bootstrap is an administrator task: the developer cannot run it.
    let o = run(
        &e,
        &e.ws,
        &[
            "--as",
            "developer",
            "project",
            "init",
            "--name",
            "x",
            "--developer",
            "dev",
            "--steward",
            "steward",
            "--approver",
            "approver",
        ],
        &[],
    );
    assert_eq!(o.code, 4, "{}", o.all());
    // SoD: developer cannot also be the approver.
    let o = run(
        &e,
        &e.ws,
        &[
            "project",
            "init",
            "--name",
            "x",
            "--developer",
            "dev",
            "--steward",
            "steward",
            "--approver",
            "dev",
        ],
        &[],
    );
    assert!(
        o.err()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("separation of duties")
    );
}

#[test]
fn approval_review_needs_an_explicit_human_approver_and_confirms_each_item() {
    let e = env(false);
    admin_login(&e);
    assert_eq!(init(&e, "none").code, 7);
    // Never auto-selected, even though an approver profile exists.
    let o = run(&e, &e.ws, &["approval", "review", "--all", "--yes"], &[]);
    assert_eq!(o.code, 3);
    assert!(
        o.err()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("(approver profiles: approver)"),
        "{}",
        o.all()
    );
    // Listing first (no decision without selection).
    let o = run(
        &e,
        &e.ws,
        &["--json", "--as", "approver", "approval", "review"],
        &[],
    );
    let pending = o.json()["pending"].as_array().unwrap().clone();
    // 3 agent skills + 1 layer skill file (architecture-diagrams.md) + 1 discovered skill
    // directory (api-error-handling/), all pending.
    assert_eq!(pending.len(), 5, "{}", o.all());
    assert!(
        pending
            .iter()
            .all(|p| p["details"].to_string().contains("definition digest"))
    );
    // Developer / steward cannot decide: every item is refused with the reason.
    for who in ["developer", "steward"] {
        let o = run(
            &e,
            &e.ws,
            &["--json", "--as", who, "approval", "review", "--all", "--yes"],
            &[],
        );
        let r = o.json()["results"].as_array().unwrap().clone();
        assert!(
            r.iter().all(|x| x["result"] == "refused"
                && x["reason"]
                    .as_str()
                    .unwrap()
                    .contains("approval.decide; roles that grant it: approver")),
            "{who}: {r:?}"
        );
    }
    // Interactive confirmation: "n" skips, "y" approves.
    let mut c = Command::new(assert_cmd::cargo::cargo_bin("eaeh"));
    c.current_dir(&e.ws)
        .args([
            "--config",
            e.cfg.to_str().unwrap(),
            "--json",
            "--as",
            "approver",
            "approval",
            "review",
            "hld-design",
            "lld-design",
        ])
        .env("EAEH_HOME", &e.home)
        .env_remove("EAEH_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"y\nn\n").unwrap();
    let out = child.wait_with_output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let res: Vec<(String, String)> = v["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["title"].as_str().unwrap()[..10].to_string(),
                r["result"].to_string(),
            )
        })
        .collect();
    assert_eq!(
        res,
        vec![
            ("hld-design".into(), "\"approved\"".into()),
            ("lld-design".into(), "\"skipped\"".into())
        ]
    );
    // Bulk with explicit --yes approves the rest; nothing left afterwards.
    let o = run(
        &e,
        &e.ws,
        &[
            "--json", "--as", "approver", "approval", "review", "--all", "--yes",
        ],
        &[],
    );
    // The rest: the skipped agent skill, the layer skill file and the skill directory.
    assert_eq!(o.json()["results"].as_array().unwrap().len(), 4);
    let o = run(&e, &e.ws, &["--json", "skill", "pending"], &[]);
    let j = o.json();
    assert!(
        j["skills"]
            .as_array()
            .unwrap()
            .iter()
            .chain(j["layer_skills"].as_array().unwrap())
            .all(|s| s["state"]["state"] == "approved"),
        "{}",
        o.all()
    );
    let layer = &j["layer_skills"][0];
    assert_eq!(layer["reference"], "architecture-diagrams@=1.0.0");
    assert!(
        layer["layers"]
            .to_string()
            .contains("engineering.design/hld (hld-design)"),
        "{layer}"
    );
}

#[test]
fn readiness_and_workflow_prerequisites_explain_what_is_missing() {
    let e = env(false);
    admin_login(&e);
    init(&e, "none");
    run(
        &e,
        &e.ws,
        &["--as", "approver", "approval", "review", "--all", "--yes"],
        &[],
    );
    let o = run(&e, &e.ws, &["--json", "project", "readiness"], &[]);
    assert_eq!(o.code, 7);
    let r = o.json();
    assert_eq!(r["blocking"], 1, "{}", o.stdout);
    let b = r["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["status"] == "blocking")
        .unwrap();
    assert_eq!(b["area"], "provider");
    assert!(b["fix"].as_str().unwrap().contains("[providers.replay]"));
    // The run is refused before anything starts, with the same report.
    let o = run(&e, &e.ws, &["workflow", "run", "requirements", "--yes"], &[]);
    assert_eq!(o.code, 7);
    assert!(
        o.stdout.contains("Not started") && o.stdout.contains("no model provider is registered"),
        "{}",
        o.all()
    );
    assert!(o.stderr.contains("acting as profile 'developer'") || o.stderr.is_empty());
    // Activities of later phases are clearly unavailable.
    let o = run(&e, &e.ws, &["workflow", "run", "release"], &[]);
    assert_eq!(o.err()["error"]["code"], "not_supported");
    // Low-level start: missing input explained with an example.
    let p = run(&e, &e.ws, &["--json", "project", "readiness"], &[]);
    let _ = p;
    let manifest = std::fs::read_to_string(e.ws.join(".eaeh/project.toml")).unwrap();
    let pid = manifest
        .lines()
        .find(|l| l.starts_with("id = "))
        .unwrap()
        .split('"')
        .nth(1)
        .unwrap()
        .to_string();
    let o = run(
        &e,
        &e.ws,
        &[
            "workflow",
            "start",
            "--definition",
            "requirements.analysis",
            "--project",
            &pid,
            "--input",
            "{}",
        ],
        &[],
    );
    assert_eq!(
        o.code,
        0,
        "the run starts and fails at its first stage: {}",
        o.all()
    );
    let st = run(&e, &e.ws, &["--json", "workflow", "status"], &[]).json();
    assert_eq!(st["status"], "failed");
    assert!(st["next"].to_string().contains("--input '{\\\"brd\\\""), "{st}");
    // Insufficient permission names the permission and the granting roles.
    let o = run(
        &e,
        &e.ws,
        &["--as", "developer", "project", "create", "--name", "z"],
        &[],
    );
    assert_eq!(o.code, 4);
    let m = o.err()["error"]["message"].as_str().unwrap().to_string();
    assert!(
        m.contains("principal.manage") && m.contains("roles that grant it: admin"),
        "{m}"
    );
    assert!(
        o.err()["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("Administrators deliberately cannot operate")
    );
}

#[test]
fn deterministic_onboarding_and_requirements_workflow() {
    let e = env(true);
    admin_login(&e);
    init(&e, "replay");
    run(
        &e,
        &e.ws,
        &["--as", "approver", "approval", "review", "--all", "--yes"],
        &[],
    );
    let o = run(&e, &e.ws, &["project", "readiness"], &[]);
    assert_eq!(o.code, 0, "{}", o.all());
    assert!(o.stdout.contains("READY") && o.stdout.contains("NOT AI-generated"));
    // Plan first (no --yes): nothing starts.
    let o = run(&e, &e.ws, &["--json", "workflow", "run", "requirements"], &[]);
    assert_eq!(o.json()["started"], false);
    assert_eq!(
        o.json()["plan"]["input"]["brd"],
        "simple-springboot-api-brd/brd-simple-spring-boot-api@v1"
    );
    let o = run(
        &e,
        &e.ws,
        &["--json", "workflow", "run", "requirements", "--yes"],
        &[],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    let st = &o.json()["status"];
    assert_eq!(
        (st["status"].as_str(), st["stage"].as_str()),
        (Some("waiting_approval"), Some("approve_requirements")),
        "{st}"
    );
    let next = st["next"].to_string();
    assert!(next.contains("eaeh artifact export") && next.contains("--as <approver-profile>"));
    // Developer cannot approve the gate.
    let a = run(
        &e,
        &e.ws,
        &["--json", "approval", "list", "--status", "pending"],
        &[],
    )
    .json();
    let gate = a["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["subject_type"] == "workflow_gate")
        .unwrap()
        .clone();
    let (aid, hash, art) = (
        gate["id"].as_str().unwrap(),
        gate["artifact_hash"].as_str().unwrap(),
        gate["artifact_id"].as_str().unwrap(),
    );
    let o = run(
        &e,
        &e.ws,
        &[
            "--as",
            "developer",
            "approval",
            "decide",
            aid,
            "--decision",
            "approve",
            "--artifact-hash",
            hash,
        ],
        &[],
    );
    assert_eq!(o.code, 4, "{}", o.all());
    // Review (export) and request changes, then approve the revision.
    let out = e.dir.join("review");
    let o = run(
        &e,
        &e.ws,
        &["artifact", "export", art, "--out", out.to_str().unwrap()],
        &[],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    let md = std::fs::read_to_string(out.join("requirements.md")).unwrap();
    assert!(
        md.contains("REQ-F-003") && md.contains("ISS-001") && md.contains("uncovered: 0"),
        "{md}"
    );
    let o = run(
        &e,
        &e.ws,
        &[
            "approval",
            "decide",
            aid,
            "--decision",
            "request_changes",
            "--comments",
            "Version from build",
            "--artifact-hash",
            hash,
            "--as",
            "approver",
        ],
        &[],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    let st = run(&e, &e.ws, &["--json", "workflow", "status"], &[]).json();
    assert_eq!(st["revisions"]["approve_requirements"], 1);
    let a = run(
        &e,
        &e.ws,
        &["--json", "approval", "list", "--status", "pending"],
        &[],
    )
    .json();
    let gate = a["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["subject_type"] == "workflow_gate")
        .unwrap()
        .clone();
    let o = run(
        &e,
        &e.ws,
        &[
            "approval",
            "decide",
            gate["id"].as_str().unwrap(),
            "--decision",
            "approve",
            "--comments",
            "ok",
            "--artifact-hash",
            gate["artifact_hash"].as_str().unwrap(),
            "--as",
            "approver",
        ],
        &[],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    let st = run(&e, &e.ws, &["--json", "workflow", "status"], &[]).json();
    assert_eq!(st["status"], "succeeded", "{st}");
    // Traceability recorded from the approved requirements; replay is visible in provenance.
    let t = run(&e, &e.ws, &["--json", "trace", "--id", "REQ-F-003"], &[]).json();
    assert!(
        t["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["relation"] == "derives" && l["to_id"] == "REQ-F-003"),
        "{t}"
    );
    let arts = st["artifacts"].as_array().unwrap();
    let refine = arts.iter().find(|a| a["state"] == "refine").unwrap();
    let o = run(
        &e,
        &e.ws,
        &[
            "artifact",
            "export",
            refine["id"].as_str().unwrap(),
            "--out",
            e.dir.join("model").to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(o.code, 0);
    let o = run(&e, &e.ws, &["audit", "list", "--limit", "500"], &[]);
    assert_eq!(
        o.code,
        0,
        "the admin profile is selected automatically for audit.read: {}",
        o.all()
    );
    assert!(o.stderr.contains("acting as profile 'admin'"));
    let audit = o.stdout;
    for a in [
        "principal.create",
        "skill.register",
        "approval.decide",
        "workflow.revision",
        "workflow.quality_gate",
        "trace.record",
        "knowledge.read",
    ] {
        assert!(audit.contains(a), "audit lacks {a}");
    }
}

#[test]
fn mcp_server_exposes_governed_tools_without_approval_powers() {
    let e = env(true);
    admin_login(&e);
    init(&e, "replay");
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("eaeh"))
        .current_dir(&e.dir)
        .args([
            "--config",
            e.cfg.to_str().unwrap(),
            "mcp",
            "--project-root",
            e.ws.to_str().unwrap(),
        ])
        .env("EAEH_HOME", &e.home)
        .env("EAEH_PROFILE", "developer")
        .env_remove("EAEH_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut call = |msg: Value| -> Value {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    };
    let init = call(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "eaeh");
    // A notification gets no response; the next request is answered normally.
    let _ = call(json!({"jsonrpc": "2.0", "id": 0, "method": "ping"}));
    let tools = call(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let names: Vec<String> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        names.contains(&"eaeh_activity_run".to_string())
            && names.contains(&"eaeh_pending_approvals".to_string())
    );
    assert!(
        !names
            .iter()
            .any(|n| n.contains("approve") || n.contains("decide")),
        "no approval tool: {names:?}"
    );
    let text = |r: &Value| r["result"]["content"][0]["text"].as_str().unwrap().to_string();
    let r = call(
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "eaeh_project_context", "arguments": {}}}),
    );
    let ctx: Value = serde_json::from_str(&text(&r)).unwrap();
    assert_eq!(ctx["you"]["principal"], "dev");
    assert_eq!(ctx["stack"]["framework"], "Spring Boot 3.x");
    // Not ready (skills pending): the run is refused, the approver command is surfaced.
    let r = call(
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "eaeh_activity_run", "arguments": {"activity": "requirements"}}}),
    );
    let v: Value = serde_json::from_str(&text(&r)).unwrap();
    assert_eq!(v["started"], false, "{v}");
    let r = call(
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "eaeh_pending_approvals", "arguments": {}}}),
    );
    let v: Value = serde_json::from_str(&text(&r)).unwrap();
    assert_eq!(
        v["skills_and_standards"].as_array().unwrap().len(),
        5,
        "3 agent skills + 1 layer skill + 1 skill directory"
    );
    assert!(
        v["note"]
            .as_str()
            .unwrap()
            .contains("cannot and must not approve")
    );
    let r = call(
        json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "eaeh_knowledge_search", "arguments": {"query": "products endpoint price"}}}),
    );
    assert!(
        text(&r).contains("simple-springboot-api-brd/brd-simple-spring-boot-api@v1#"),
        "{}",
        text(&r)
    );
    let r = call(
        json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "eaeh_approve_everything", "arguments": {}}}),
    );
    assert_eq!(r["result"]["isError"], true);
    let r = call(json!({"jsonrpc": "2.0", "id": 8, "method": "nope"}));
    assert_eq!(r["error"]["code"], -32601);
    drop(call);
    drop(stdin);
    let _ = child.wait();
}

/// Recorded example outputs for the simple-springboot-api BRD (hand-authored fixtures,
/// used here as what an external AI client would submit).
fn fixture_output(agent: &str) -> Value {
    let c: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("examples/simple-springboot-api/replay-cassette.json")).unwrap(),
    )
    .unwrap();
    c["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["match"].as_str().unwrap().contains(&format!("'{agent}'")))
        .unwrap()["responses"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|r| r.get("json").cloned())
        .unwrap()
}

#[test]
fn external_ai_bootstrap_cli_and_mcp_hand_offs_without_model_calls() {
    // external_ai installation; a replay provider is enabled but must be ignored.
    let e = env_mode(true, Some("external_ai"));
    admin_login(&e);
    let o = run(
        &e,
        &e.ws,
        &[
            "--json",
            "project",
            "init",
            "--name",
            "simple-springboot-api",
            "--language",
            "Java 21",
            "--framework",
            "Spring Boot 3.x",
            "--build-tool",
            "Maven",
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
            "--store-profiles",
        ],
        &[],
    );
    assert_eq!(o.code, 7, "skills await approval: {}", o.all());
    // Default bootstrap: external_ai, both clients, delivery enabled.
    let manifest = std::fs::read_to_string(e.ws.join(".eaeh/project.toml")).unwrap();
    for want in [
        "mode = \"external_ai\"",
        "\"claude-code\"",
        "\"github-copilot\"",
        "engineering.delivery",
        "build_verification = \"required\"",
    ] {
        assert!(manifest.contains(want), "manifest lacks {want}:\n{manifest}");
    }
    let mcp: Value = serde_json::from_str(&std::fs::read_to_string(e.ws.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(mcp["mcpServers"]["eaeh"]["env"]["EAEH_PROFILE"], "developer");
    let vs: Value =
        serde_json::from_str(&std::fs::read_to_string(e.ws.join(".vscode/mcp.json")).unwrap()).unwrap();
    assert_eq!(vs["servers"]["eaeh"]["type"], "stdio");
    assert_eq!(vs["servers"]["eaeh"]["args"][2], "mcp");
    let copilot = std::fs::read_to_string(e.ws.join(".github/copilot-instructions.md")).unwrap();
    let claude = std::fs::read_to_string(e.ws.join("CLAUDE.md")).unwrap();
    assert_eq!(copilot, claude, "one provider-neutral instruction text");
    assert!(claude.contains("eaeh_task_submit") && claude.contains("Approvals are human-only"));
    // No credential is written into any generated file.
    let creds = std::fs::read_to_string(e.home.join("credentials.json")).unwrap();
    let tokens: Vec<String> = serde_json::from_str::<Value>(&creds).unwrap()["profiles"]
        .as_object()
        .unwrap()
        .values()
        .map(|p| p["token"].as_str().unwrap().to_string())
        .collect();
    assert!(tokens.len() >= 4);
    for f in [
        ".mcp.json",
        ".vscode/mcp.json",
        "CLAUDE.md",
        ".github/copilot-instructions.md",
        ".eaeh/project.toml",
    ] {
        let t = std::fs::read_to_string(e.ws.join(f)).unwrap();
        assert!(
            tokens.iter().all(|k| !t.contains(k.as_str())),
            "{f} contains a token"
        );
    }

    let o = run(
        &e,
        &e.ws,
        &[
            "--json", "--as", "approver", "approval", "review", "--all", "--yes",
        ],
        &[],
    );
    // Agent skills + the layer skill file + the discovered skill directory.
    assert_eq!(o.json()["results"].as_array().unwrap().len(), 7, "{}", o.all());
    let o = run(
        &e,
        &e.ws,
        &["project", "readiness", "--workflow", "requirements.analysis"],
        &[],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    for want in [
        "mode external_ai",
        "under your own subscription",
        "not registered in this mode (no fallback): replay",
        "Claude Code: EAEH MCP server configured in .mcp.json",
        "GitHub Copilot (VS Code agent mode): EAEH MCP server configured in .vscode/mcp.json",
    ] {
        assert!(o.stdout.contains(want), "readiness lacks {want}:\n{}", o.stdout);
    }
    // Delivery needs an approved design first.
    let o = run(&e, &e.ws, &["workflow", "run", "deliver"], &[]);
    assert_eq!(o.code, 7, "{}", o.all());
    assert!(o.stdout.contains("no approved design"), "{}", o.stdout);

    // ---- CLI hand-off (business analyst)
    let o = run(
        &e,
        &e.ws,
        &["--json", "workflow", "run", "requirements", "--yes"],
        &[],
    );
    assert_eq!(o.json()["status"]["status"], "waiting_external", "{}", o.all());
    assert_eq!(o.json()["status"]["execution_mode"], "external_ai");
    let o = run(&e, &e.ws, &["--json", "task", "list"], &[]);
    let tasks = o.json()["tasks"].as_array().unwrap().clone();
    assert_eq!(tasks.len(), 1, "{}", o.all());
    let t1 = tasks[0]["task_id"].as_str().unwrap().to_string();
    let pkg = e.dir.join("pkg.json");
    let o = run(
        &e,
        &e.ws,
        &["task", "show", &t1, "--out", pkg.to_str().unwrap()],
        &[],
    );
    assert_eq!(o.code, 0, "{}", o.all());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&pkg).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let pv: Value = serde_json::from_str(&std::fs::read_to_string(&pkg).unwrap()).unwrap();
    assert_eq!(pv["package"]["agent"]["reference"], "business-analyst@1.0.0");
    // The approver can see the hand-off but not its package.
    let o = run(&e, &e.ws, &["--as", "approver", "task", "show", &t1], &[]);
    assert_eq!(o.code, 4, "{}", o.all());
    let brd = "simple-springboot-api-brd/brd-simple-spring-boot-api@v1";
    let o = run(
        &e,
        &e.ws,
        &[
            "task",
            "tool",
            &t1,
            "knowledge.read",
            "--input",
            &json!({"document": brd}).to_string(),
        ],
        &[],
    );
    assert_eq!(o.json()["hits"].as_array().unwrap().len(), 20, "{}", o.all());
    // An invalid result is rejected with feedback; the valid one is accepted.
    let bad = e.dir.join("bad.json");
    std::fs::write(
        &bad,
        json!({"document": "requirements", "title": "x", "requirements": [], "citations": []}).to_string(),
    )
    .unwrap();
    let o = run(
        &e,
        &e.ws,
        &[
            "--json",
            "task",
            "submit",
            &t1,
            "--output",
            bad.to_str().unwrap(),
            "--client",
            "claude-code",
        ],
        &[],
    );
    assert_eq!(o.json()["accepted"], false, "{}", o.all());
    let good = e.dir.join("ba.json");
    std::fs::write(&good, fixture_output("business-analyst").to_string()).unwrap();
    let o = run(
        &e,
        &e.ws,
        &[
            "--json",
            "task",
            "submit",
            &t1,
            "--output",
            good.to_str().unwrap(),
            "--client",
            "claude-code",
        ],
        &[],
    );
    assert_eq!(o.json()["accepted"], true, "{}", o.all());
    assert_eq!(o.json()["workflow_status"], "waiting_external");

    // ---- MCP hand-off (requirements engineer) from a client identifying as VS Code / Copilot
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("eaeh"))
        .current_dir(&e.dir)
        .args([
            "--config",
            e.cfg.to_str().unwrap(),
            "mcp",
            "--project-root",
            e.ws.to_str().unwrap(),
        ])
        .env("EAEH_HOME", &e.home)
        .env("EAEH_PROFILE", "developer")
        .env_remove("EAEH_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut call = |id: u64, method: &str, params: Value| -> Value {
        writeln!(
            stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        stdin.flush().unwrap();
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    };
    let init = call(
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "Visual Studio Code", "version": "1.100"}}),
    );
    assert!(
        init["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("execution mode external_ai")
    );
    let names: Vec<String> = call(2, "tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    for t in [
        "eaeh_task_list",
        "eaeh_task_get",
        "eaeh_task_tool",
        "eaeh_task_submit",
    ] {
        assert!(names.contains(&t.to_string()), "{names:?}");
    }
    assert!(
        !names
            .iter()
            .any(|n| n.contains("approve") || n.contains("decide"))
    );
    let mut tool = |id: u64, name: &str, args: Value| -> Value {
        let r = call(id, "tools/call", json!({"name": name, "arguments": args}));
        assert_eq!(r["result"]["isError"], false, "{r}");
        serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let ctx = tool(3, "eaeh_project_context", json!({}));
    assert_eq!(ctx["execution_mode"], "external_ai");
    let list = tool(4, "eaeh_task_list", json!({}));
    let t2 = list["tasks"][0]["task_id"].as_str().unwrap().to_string();
    assert_ne!(t1, t2);
    let pkg = tool(5, "eaeh_task_get", json!({"task_id": t2}));
    assert_eq!(
        pkg["package"]["agent"]["reference"],
        "requirements-engineer@1.0.0"
    );
    let read = tool(
        6,
        "eaeh_task_tool",
        json!({"task_id": t2, "tool": "knowledge.read", "input": {"document": brd}}),
    );
    assert_eq!(read["hits"].as_array().unwrap().len(), 20);
    let sub = tool(
        7,
        "eaeh_task_submit",
        json!({"task_id": t2, "output": fixture_output("requirements-engineer")}),
    );
    assert_eq!(sub["accepted"], true, "{sub}");
    assert_eq!(sub["status"]["status"], "waiting_approval");
    drop(stdin);
    let _ = child.wait();

    // Provenance and audit: two external clients, no model call at all.
    let o = run(&e, &e.ws, &["--json", "task", "list", "--all"], &[]);
    let clients: Vec<String> = o.json()["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["client"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(clients, vec!["claude-code", "github-copilot"]);
    let audit = run(
        &e,
        &e.dir,
        &["--as", "admin", "audit", "list", "--limit", "500"],
        &[],
    )
    .stdout;
    assert!(audit.contains("external_task.accept") && audit.contains("external_task.handoff"));
    assert!(!audit.contains("\"model.invoke\""), "no EAEH-managed model call");
}

/// ADR-0026: the task-scoped MCP server used by the automated worker exposes exactly one
/// hand-off and refuses everything else before authenticating anyone; the worker commands
/// refuse outside `external_ai` mode and report a missing client honestly.
#[test]
fn task_scoped_mcp_server_and_worker_commands() {
    let e = env_mode(false, Some("external_ai"));
    admin_login(&e);
    let task = "019a0000-0000-7000-8000-000000000001";
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("eaeh"))
        .current_dir(&e.dir)
        .args(["--config", e.cfg.to_str().unwrap(), "mcp", "--task", task])
        .env("EAEH_HOME", &e.home)
        .env_remove("EAEH_TOKEN")
        .env_remove("EAEH_PROFILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut call = |msg: Value| -> Value {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    };
    let init = call(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "claude-code", "version": "0"}}}),
    );
    assert!(init["result"]["instructions"].as_str().unwrap().contains(task));
    let tools = call(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let mut names: Vec<String> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "eaeh_task_get",
            "eaeh_task_submit",
            "eaeh_task_tool",
            "eaeh_task_validate"
        ]
    );
    let text = |r: &Value| r["result"]["content"][0]["text"].as_str().unwrap().to_string();
    for (id, name, args) in [
        (3, "eaeh_project_context", json!({})),
        (4, "eaeh_activity_run", json!({"activity": "design"})),
        (5, "eaeh_task_list", json!({})),
        (
            6,
            "eaeh_task_get",
            json!({"task_id": "019a0000-0000-7000-8000-000000000002"}),
        ),
        (
            7,
            "eaeh_task_submit",
            json!({"task_id": "019a0000-0000-7000-8000-000000000002", "output": {}}),
        ),
    ] {
        let r = call(
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}}),
        );
        assert_eq!(r["result"]["isError"], true, "{name}");
        assert!(text(&r).contains("forbidden"), "{name}: {}", text(&r));
    }
    drop(call);
    drop(stdin);
    let _ = child.wait();

    // Worker commands outside a project / without a client.
    let o = run(&e, &e.dir, &["worker", "status"], &[]);
    assert_ne!(o.code, 0, "needs the project manifest");
    let d = env_mode(false, Some("direct_provider"));
    admin_login(&d);
    std::fs::create_dir_all(d.ws.join(".eaeh")).unwrap();
    let o = run(&d, &d.dir, &["worker", "history"], &[]);
    assert_ne!(o.code, 0);
}
