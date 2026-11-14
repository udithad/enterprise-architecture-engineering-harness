//! CLI end-to-end: init, one-time bootstrap with protected token file, governed
//! commands authenticated via EAEH_TOKEN, approval flow, audit verification.

use assert_cmd::Command;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn config(dir: &Path) -> PathBuf {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let cfg = format!(
        r#"
[storage]
data_dir = "{data}"
[definitions]
prompts_dir = "{repo}/prompts"
agents_dir = "{repo}/config/agents"
workflows_dir = "{repo}/workflows"
policies_dir = "{repo}/config/policies"
[isolation]
container_enabled = false
[log]
format = "pretty"
filter = "warn"
"#,
        data = dir.join("data").display(),
        repo = repo.display()
    );
    let p = dir.join("eaeh.toml");
    std::fs::write(&p, cfg).unwrap();
    p
}

fn eaeh(cfg: &Path, token: Option<&str>, args: &[&str]) -> (i32, Value, String) {
    let mut c = Command::cargo_bin("eaeh").unwrap();
    c.arg("--config")
        .arg(cfg)
        .args(args)
        .env_remove("EAEH_TOKEN")
        .env_remove("EAEH_PROFILE")
        // Never touch the real user's credential store.
        .env("EAEH_HOME", cfg.parent().unwrap().join("eaeh-home"));
    if let Some(t) = token {
        c.env("EAEH_TOKEN", t);
    }
    let out = c.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let v = serde_json::from_str(&stdout)
        .or_else(|_| serde_json::from_str(&stderr))
        .unwrap_or(Value::Null);
    (out.status.code().unwrap_or(-1), v, stderr)
}

#[test]
fn cli_end_to_end() {
    let d = tempfile::tempdir().unwrap();
    let cfg = config(d.path());
    let (code, v, err) = eaeh(&cfg, None, &["init"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["health"]["status"], "ok");

    let token_file = d.path().join("admin.token");
    let (code, v, err) = eaeh(
        &cfg,
        None,
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
            token_file.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        v.get("token").is_none(),
        "token must not be printed when written to a file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&token_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let admin = std::fs::read_to_string(&token_file).unwrap();
    let project = v["project_id"].as_str().unwrap().to_string();

    // Bootstrap is one-time; no default credentials exist.
    let (code, v, _) = eaeh(
        &cfg,
        None,
        &[
            "admin",
            "bootstrap",
            "--tenant",
            "x",
            "--project",
            "y",
            "--admin-id",
            "root",
        ],
    );
    assert_ne!(code, 0);
    assert_eq!(v["error"]["code"], "conflict");
    let (code, _, _) = eaeh(&cfg, None, &["project", "list"]);
    assert_eq!(code, 3, "governed commands require EAEH_TOKEN");

    let (code, _, err) = eaeh(
        &cfg,
        Some(&admin),
        &[
            "principal",
            "create",
            "--id",
            "arthur",
            "--display-name",
            "Arthur",
            "--roles",
            "approver",
            "--projects",
            &project,
        ],
    );
    assert_eq!(code, 0, "{err}");
    let (code, _, _) = eaeh(
        &cfg,
        Some(&admin),
        &[
            "principal",
            "create",
            "--id",
            "olivia",
            "--display-name",
            "Olivia",
            "--roles",
            "operator",
            "--projects",
            &project,
        ],
    );
    assert_eq!(code, 0);
    // Tokens are handed over through 0600 files; the CLI never prints them by default.
    let tf = d.path().join("arthur.token");
    let (code, _, err) = eaeh(
        &cfg,
        Some(&admin),
        &[
            "token",
            "issue",
            "--principal",
            "arthur",
            "--label",
            "cli",
            "--token-file",
            tf.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    let approver = std::fs::read_to_string(&tf).unwrap();
    // Tokens are handed over through 0600 files; the CLI never prints them by default.
    let tf = d.path().join("olivia.token");
    let (code, _, err) = eaeh(
        &cfg,
        Some(&admin),
        &[
            "token",
            "issue",
            "--principal",
            "olivia",
            "--label",
            "cli",
            "--token-file",
            tf.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    let operator = std::fs::read_to_string(&tf).unwrap();

    let (code, v, err) = eaeh(
        &cfg,
        Some(&operator),
        &[
            "workflow",
            "start",
            "--definition",
            "demo.approval",
            "--project",
            &project,
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["status"], "waiting_approval");
    let wf = v["id"].as_str().unwrap().to_string();

    let (_, v, _) = eaeh(
        &cfg,
        Some(&approver),
        &["approval", "list", "--project", &project, "--status", "pending"],
    );
    let approval = v["items"][0]["id"].as_str().unwrap().to_string();
    let hash = v["items"][0]["artifact_hash"].as_str().unwrap().to_string();

    let (code, _, _) = eaeh(
        &cfg,
        Some(&operator),
        &[
            "approval",
            "decide",
            &approval,
            "--decision",
            "approve",
            "--artifact-hash",
            &hash,
        ],
    );
    assert_eq!(code, 4, "operator may not approve");
    let (code, v, err) = eaeh(
        &cfg,
        Some(&approver),
        &[
            "approval",
            "decide",
            &approval,
            "--decision",
            "approve",
            "--comments",
            "lgtm",
            "--artifact-hash",
            &hash,
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["workflow_status"], "succeeded");
    let (_, v, _) = eaeh(&cfg, Some(&operator), &["workflow", "show", &wf]);
    assert_eq!(v["instance"]["status"], "succeeded");
    assert_eq!(v["history"].as_array().unwrap().len(), 3);

    let (code, v, _) = eaeh(&cfg, None, &["audit", "verify"]);
    assert_eq!(code, 0);
    assert_eq!(v["valid"], true);
    let (code, _, _) = eaeh(&cfg, Some(&operator), &["audit", "list"]);
    assert_eq!(code, 4);
}

/// Phase 3 CLI: BRD ingestion, request-changes decisions, traceability and artifact export.
#[test]
fn phase3_brd_request_changes_trace_and_export() {
    let d = tempfile::tempdir().unwrap();
    let cfg = config(d.path());
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    assert_eq!(eaeh(&cfg, None, &["init"]).0, 0);
    let token_file = d.path().join("admin.token");
    let (code, v, err) = eaeh(
        &cfg,
        None,
        &[
            "admin",
            "bootstrap",
            "--tenant",
            "acme",
            "--project",
            "orders",
            "--admin-id",
            "admin",
            "--token-file",
            token_file.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    let project = v["project_id"].as_str().unwrap().to_string();
    let admin = std::fs::read_to_string(&token_file).unwrap();
    let mut tokens = std::collections::HashMap::new();
    for (id, role) in [
        ("sam", "knowledge_steward"),
        ("arthur", "approver"),
        ("olivia", "operator"),
    ] {
        let (code, _, err) = eaeh(
            &cfg,
            Some(&admin),
            &[
                "principal",
                "create",
                "--id",
                id,
                "--display-name",
                id,
                "--roles",
                role,
                "--projects",
                &project,
            ],
        );
        assert_eq!(code, 0, "{err}");
        let tf = d.path().join(format!("{id}.token"));
        let (code, _, err) = eaeh(
            &cfg,
            Some(&admin),
            &[
                "token",
                "issue",
                "--principal",
                id,
                "--label",
                "cli",
                "--token-file",
                tf.to_str().unwrap(),
            ],
        );
        assert_eq!(code, 0, "{err}");
        tokens.insert(id, std::fs::read_to_string(&tf).unwrap());
    }
    let brd = repo.join("examples/brd/order-management.md");
    let (code, v, err) = eaeh(
        &cfg,
        Some(&tokens["sam"]),
        &[
            "brd",
            "ingest",
            "--file",
            brd.to_str().unwrap(),
            "--project",
            &project,
            "--source",
            "demo-brd",
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["citation"], "demo-brd/order-management@v1");
    assert_eq!(v["document"]["classification"], "confidential");
    assert_eq!(v["document"]["tags"], serde_json::json!(["brd"]));
    assert_eq!(v["document"]["chunks"], 8);
    // Only Markdown/text BRDs; an operator cannot ingest.
    let spec = repo.join("examples/knowledge/specs/orders-api.json");
    assert_ne!(
        eaeh(
            &cfg,
            Some(&tokens["sam"]),
            &[
                "brd",
                "ingest",
                "--file",
                spec.to_str().unwrap(),
                "--project",
                &project
            ]
        )
        .0,
        0
    );
    assert_eq!(
        eaeh(
            &cfg,
            Some(&tokens["olivia"]),
            &[
                "brd",
                "ingest",
                "--file",
                brd.to_str().unwrap(),
                "--project",
                &project
            ]
        )
        .0,
        4
    );

    let (code, v, err) = eaeh(
        &cfg,
        Some(&tokens["olivia"]),
        &[
            "workflow",
            "start",
            "--definition",
            "demo.approval",
            "--project",
            &project,
        ],
    );
    assert_eq!(code, 0, "{err}");
    let wf = v["id"].as_str().unwrap().to_string();
    let pending = |c: &Path| {
        let (_, v, _) = eaeh(
            c,
            Some(&tokens["arthur"]),
            &["approval", "list", "--project", &project, "--status", "pending"],
        );
        (
            v["items"][0]["id"].as_str().unwrap().to_string(),
            v["items"][0]["artifact_hash"].as_str().unwrap().to_string(),
        )
    };
    let (aid, hash) = pending(&cfg);
    let (code, _, _) = eaeh(
        &cfg,
        Some(&tokens["arthur"]),
        &[
            "approval",
            "decide",
            &aid,
            "--decision",
            "request_changes",
            "--artifact-hash",
            &hash,
        ],
    );
    assert_ne!(code, 0, "request_changes needs comments");
    let (code, v, err) = eaeh(
        &cfg,
        Some(&tokens["arthur"]),
        &[
            "approval",
            "decide",
            &aid,
            "--decision",
            "request_changes",
            "--comments",
            "shorter please",
            "--artifact-hash",
            &hash,
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["approval"]["status"], "changes_requested", "{v}");
    let (_, v, _) = eaeh(&cfg, Some(&tokens["olivia"]), &["workflow", "show", &wf]);
    assert_eq!(v["instance"]["status"], "waiting_approval");
    assert_eq!(v["instance"]["data"]["revisions"]["review"], 1);
    let (aid, hash) = pending(&cfg);
    let (code, _, err) = eaeh(
        &cfg,
        Some(&tokens["arthur"]),
        &[
            "approval",
            "decide",
            &aid,
            "--decision",
            "approve",
            "--artifact-hash",
            &hash,
        ],
    );
    assert_eq!(code, 0, "{err}");

    let (code, v, err) = eaeh(&cfg, Some(&tokens["olivia"]), &["trace", "--project", &project]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        v["items"],
        serde_json::json!([]),
        "demo.approval declares no trace links"
    );
    let (_, v, _) = eaeh(&cfg, Some(&tokens["olivia"]), &["workflow", "show", &wf]);
    let art = v["instance"]["data"]["artifacts"]["draft"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let out = d.path().join("export");
    let (code, v, err) = eaeh(
        &cfg,
        Some(&tokens["olivia"]),
        &[
            "artifact",
            "export",
            "--project",
            &project,
            &art,
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["written"].as_array().unwrap().len(), 1);
    assert!(std::fs::read_dir(&out).unwrap().count() == 1);
}
