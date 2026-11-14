//! Architecture fitness tests (AC-02, clarification 5).
//!
//! Enforced from `cargo metadata` and the source tree:
//! 1. Each crate depends (normal deps) only on its allowed internal crates (layering).
//! 2. No dependency cycles among internal crates.
//! 3. Governance never depends on intelligence, agents, extensibility, workflow or runtime;
//!    the workflow engine never depends on intelligence or agents.
//! 4. `eaeh-testkit` is never a normal (non-dev) dependency.
//! 5. Only `eaeh-store` depends on `sqlx` directly.
//! 6. `unsafe` appears only in `eaeh-isolation`.
//! 7. No credential-looking literals in configuration; agent prompts live in `prompts/`.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// crate -> (normal internal deps, dev internal deps, normal external deps)
type Graph = BTreeMap<String, (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>)>;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap()
}

/// crate -> (normal internal deps, dev internal deps, all normal external dep names)
fn graph() -> Graph {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline"])
        .current_dir(root())
        .output()
        .expect("cargo metadata");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let meta: Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut g = BTreeMap::new();
    for pkg in meta["packages"].as_array().unwrap() {
        let name = pkg["name"].as_str().unwrap().to_string();
        let (mut normal, mut dev, mut external) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        for d in pkg["dependencies"].as_array().unwrap() {
            let dn = d["name"].as_str().unwrap().to_string();
            let is_dev = d["kind"].as_str() == Some("dev");
            match (dn.starts_with("eaeh-"), is_dev) {
                (true, false) => {
                    normal.insert(dn);
                }
                (true, true) => {
                    dev.insert(dn);
                }
                (false, false) => {
                    external.insert(dn);
                }
                _ => {}
            }
        }
        g.insert(name, (normal, dev, external));
    }
    g
}

fn allowed() -> BTreeMap<&'static str, Vec<&'static str>> {
    let base = ["eaeh-core", "eaeh-store", "eaeh-events"];
    let mut m: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    m.insert("eaeh-core", vec![]);
    m.insert("eaeh-store", vec!["eaeh-core"]);
    m.insert("eaeh-events", vec!["eaeh-core", "eaeh-store"]);
    m.insert("eaeh-governance", base.to_vec());
    m.insert("eaeh-extensibility", [&base[..], &["eaeh-governance"]].concat());
    m.insert("eaeh-isolation", vec!["eaeh-core"]);
    m.insert("eaeh-validation", vec!["eaeh-core"]);
    m.insert("eaeh-workspace", [&base[..], &["eaeh-extensibility"]].concat());
    m.insert(
        "eaeh-tools",
        [
            &base[..],
            &[
                "eaeh-governance",
                "eaeh-extensibility",
                "eaeh-isolation",
                "eaeh-workspace",
                "eaeh-validation",
            ],
        ]
        .concat(),
    );
    m.insert(
        "eaeh-intelligence",
        [&base[..], &["eaeh-governance", "eaeh-extensibility"]].concat(),
    );
    m.insert(
        "eaeh-knowledge",
        [
            &base[..],
            &[
                "eaeh-governance",
                "eaeh-extensibility",
                "eaeh-validation",
                "eaeh-intelligence",
                "eaeh-tools",
            ],
        ]
        .concat(),
    );
    m.insert(
        "eaeh-skills",
        [
            &base[..],
            &[
                "eaeh-governance",
                "eaeh-validation",
                "eaeh-intelligence",
                "eaeh-tools",
            ],
        ]
        .concat(),
    );
    m.insert("eaeh-provider-anthropic", vec!["eaeh-core", "eaeh-intelligence"]);
    // Phase 3: deterministic design models (no storage, agents or models) and their
    // integration as governed tools.
    m.insert("eaeh-requirements", vec!["eaeh-core", "eaeh-validation"]);
    m.insert(
        "eaeh-architecture",
        vec!["eaeh-core", "eaeh-validation", "eaeh-requirements"],
    );
    m.insert(
        "eaeh-engineering",
        vec![
            "eaeh-core",
            "eaeh-tools",
            "eaeh-validation",
            "eaeh-knowledge",
            "eaeh-requirements",
            "eaeh-architecture",
        ],
    );
    m.insert(
        "eaeh-agents",
        [
            &base[..],
            &[
                "eaeh-governance",
                "eaeh-extensibility",
                "eaeh-tools",
                "eaeh-intelligence",
                "eaeh-validation",
                "eaeh-workspace",
                "eaeh-knowledge",
                "eaeh-skills",
            ],
        ]
        .concat(),
    );
    m.insert(
        "eaeh-workflow",
        [
            &base[..],
            &[
                "eaeh-governance",
                "eaeh-extensibility",
                "eaeh-workspace",
                "eaeh-validation",
            ],
        ]
        .concat(),
    );
    m.insert(
        "eaeh-runtime",
        [
            &base[..],
            &[
                "eaeh-governance",
                "eaeh-extensibility",
                "eaeh-isolation",
                "eaeh-workspace",
                "eaeh-tools",
                "eaeh-intelligence",
                "eaeh-validation",
                "eaeh-agents",
                "eaeh-workflow",
                "eaeh-knowledge",
                "eaeh-skills",
                "eaeh-provider-anthropic",
                "eaeh-engineering",
                "eaeh-requirements",
            ],
        ]
        .concat(),
    );
    m.insert(
        "eaeh-api",
        vec![
            "eaeh-core",
            "eaeh-store",
            "eaeh-knowledge",
            "eaeh-skills",
            "eaeh-governance",
            "eaeh-extensibility",
            "eaeh-workflow",
            "eaeh-runtime",
        ],
    );
    m.insert(
        "eaeh-cli",
        vec![
            "eaeh-core",
            "eaeh-knowledge",
            "eaeh-skills",
            "eaeh-governance",
            "eaeh-workflow",
            "eaeh-runtime",
            "eaeh-api",
        ],
    );
    m.insert("eaeh-testkit", vec!["eaeh-core", "eaeh-intelligence"]);
    m.insert("eaeh-acceptance", vec![]);
    m
}

#[test]
fn crate_dependencies_follow_the_layering() {
    let g = graph();
    let allowed = allowed();
    for (name, (normal, _, _)) in &g {
        let ok: BTreeSet<String> = allowed
            .get(name.as_str())
            .unwrap_or_else(|| panic!("crate {name} has no layering rule; add it to the fitness test"))
            .iter()
            .map(|s| s.to_string())
            .collect();
        let violations: Vec<&String> = normal.difference(&ok).collect();
        assert!(
            violations.is_empty(),
            "{name} has forbidden dependencies: {violations:?}"
        );
    }
}

#[test]
fn no_internal_dependency_cycles() {
    let g = graph();
    fn visit(n: &str, g: &Graph, stack: &mut Vec<String>, done: &mut BTreeSet<String>) {
        if done.contains(n) {
            return;
        }
        assert!(
            !stack.contains(&n.to_string()),
            "dependency cycle: {stack:?} -> {n}"
        );
        stack.push(n.to_string());
        if let Some((deps, _, _)) = g.get(n) {
            for d in deps {
                visit(d, g, stack, done);
            }
        }
        stack.pop();
        done.insert(n.to_string());
    }
    let mut done = BTreeSet::new();
    for n in g.keys() {
        visit(n, &g, &mut vec![], &mut done);
    }
}

#[test]
fn governance_is_separated_from_intelligent_orchestration() {
    let g = graph();
    let gov = &g["eaeh-governance"].0;
    for forbidden in [
        "eaeh-intelligence",
        "eaeh-agents",
        "eaeh-extensibility",
        "eaeh-workflow",
        "eaeh-runtime",
        "eaeh-tools",
    ] {
        assert!(
            !gov.contains(forbidden),
            "eaeh-governance must not depend on {forbidden}"
        );
    }
    let wf = &g["eaeh-workflow"].0;
    for forbidden in [
        "eaeh-intelligence",
        "eaeh-agents",
        "eaeh-tools",
        "eaeh-runtime",
        "eaeh-knowledge",
        "eaeh-skills",
        "eaeh-provider-anthropic",
    ] {
        assert!(
            !wf.contains(forbidden),
            "deterministic workflow engine must not depend on {forbidden}"
        );
    }
}

/// Phase 3 (ADR-0018/0019): requirement and architecture models, validators and renderers
/// are deterministic – they cannot reach storage, models, agents, tools or the network.
#[test]
fn design_models_are_deterministic_and_independent_of_intelligence() {
    let g = graph();
    for c in ["eaeh-requirements", "eaeh-architecture"] {
        let (normal, _, external) = &g[c];
        for forbidden in [
            "eaeh-store",
            "eaeh-intelligence",
            "eaeh-agents",
            "eaeh-tools",
            "eaeh-knowledge",
            "eaeh-skills",
            "eaeh-workflow",
            "eaeh-provider-anthropic",
        ] {
            assert!(!normal.contains(forbidden), "{c} must not depend on {forbidden}");
        }
        for ext in external {
            assert!(
                ["serde", "serde_json", "async-trait"].contains(&ext.as_str()),
                "{c} has unexpected external dependency {ext}"
            );
        }
    }
    // Nothing but the composition root wires the design tools in.
    for (name, (normal, _, _)) in &g {
        if normal.contains("eaeh-engineering") {
            assert_eq!(
                *name, "eaeh-runtime",
                "{name} must not depend on eaeh-engineering"
            );
        }
    }
}

#[test]
fn testkit_is_never_a_production_dependency() {
    for (name, (normal, _, _)) in graph() {
        assert!(
            !normal.contains("eaeh-testkit"),
            "{name} depends on eaeh-testkit outside dev-dependencies"
        );
    }
}

#[test]
fn only_store_uses_sqlx_directly() {
    for (name, (_, _, external)) in graph() {
        if name != "eaeh-store" {
            assert!(
                !external.contains("sqlx"),
                "{name} must use the database through eaeh-store"
            );
        }
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn unsafe_code_is_confined_to_isolation() {
    let mut files = vec![];
    rust_files(&root().join("crates"), &mut files);
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let has_unsafe = text.lines().any(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && (t.contains("unsafe {") || t.contains("unsafe fn"))
        });
        if has_unsafe {
            assert!(
                f.to_string_lossy().contains("crates/eaeh-isolation/src/"),
                "unsafe code outside eaeh-isolation: {}",
                f.display()
            );
        }
    }
}

#[test]
fn all_database_writes_use_the_serialized_writer() {
    // Production code (src/, excluding #[cfg(test)] modules) must not execute
    // INSERT/UPDATE/DELETE directly on the pool.
    let mut files = vec![];
    rust_files(&root().join("crates"), &mut files);
    for f in files
        .into_iter()
        .filter(|f| f.to_string_lossy().contains("/src/"))
    {
        let text = std::fs::read_to_string(&f).unwrap();
        let prod = text.split("#[cfg(test)]").next().unwrap_or("");
        let lines: Vec<&str> = prod.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            if l.contains(".execute(") && (l.contains(".pool()") || l.contains("pool)")) {
                let window = lines[i.saturating_sub(12)..i].join(" ").to_uppercase();
                assert!(
                    !(window.contains("INSERT ") || window.contains("UPDATE ") || window.contains("DELETE ")),
                    "direct pool write in {}:{}",
                    f.display(),
                    i + 1
                );
            }
        }
    }
}

#[test]
fn no_credentials_in_configuration_and_prompts_are_assets() {
    let cfg = std::fs::read_to_string(root().join("config/eaeh.toml")).unwrap();
    // Inspect actual key assignments (comments may legitimately name env variables).
    for line in cfg
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && l.contains('='))
    {
        let key = line.split('=').next().unwrap_or("").trim().to_lowercase();
        for bad in ["password", "secret", "api_key", "apikey", "token", "credential"] {
            // `*_tokens` keys are token counts (e.g. context_max_tokens), not credentials.
            assert!(
                !key.contains(bad) || key.ends_with("tokens"),
                "config assigns a credential-like key '{key}'"
            );
        }
    }
    assert!(!cfg.contains("sk-ant-"), "literal Anthropic key in config");
    // No literal EAEH API token (eaeh_ + 64 hex) anywhere in config or source.
    let mut files = vec![];
    rust_files(&root().join("crates"), &mut files);
    files.push(root().join("config/eaeh.toml"));
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        if !f.to_string_lossy().contains("/tests/") {
            assert!(
                !text.contains("sk-ant-api"),
                "literal Anthropic API key in {}",
                f.display()
            );
        }
    }
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for (i, _) in text.match_indices("eaeh_") {
            let tail: String = text[i + 5..].chars().take(64).collect();
            let literal_token = tail.len() == 64
                && tail.chars().all(|c| c.is_ascii_hexdigit())
                && !tail.chars().all(|c| c == '0');
            assert!(!literal_token, "hard-coded token literal in {}", f.display());
        }
    }
    // Every agent's instruction prompt is a versioned asset in prompts/.
    let prompts: BTreeSet<String> = std::fs::read_dir(root().join("prompts"))
        .unwrap()
        .flatten()
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().to_string()))
        .collect();
    for e in std::fs::read_dir(root().join("config/agents")).unwrap().flatten() {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
        let p = v["instructions_prompt"].as_str().unwrap();
        assert!(
            prompts.contains(p),
            "agent {} references prompt {p} not present in prompts/",
            e.path().display()
        );
    }
}

#[test]
fn http_client_and_provider_are_confined() {
    for (name, (normal, _, external)) in graph() {
        if name != "eaeh-provider-anthropic" {
            for c in ["reqwest", "rustls", "hyper"] {
                assert!(
                    !external.contains(c),
                    "{name} must not depend on {c} (network egress is confined to the provider adapter)"
                );
            }
        }
        if name != "eaeh-runtime" {
            assert!(
                !normal.contains("eaeh-provider-anthropic"),
                "{name} must not depend on the provider adapter"
            );
        }
    }
}

#[test]
fn governed_context_constructors_have_single_producers() {
    // enterprise_standard(..) only from eaeh-knowledge; skill_instruction(..) only from eaeh-skills
    // (definitions live in eaeh-intelligence; tests are exempt).
    let mut files = vec![];
    rust_files(&root().join("crates"), &mut files);
    for f in files
        .into_iter()
        .filter(|f| f.to_string_lossy().contains("/src/"))
    {
        let path = f.to_string_lossy().to_string();
        let text = std::fs::read_to_string(&f).unwrap();
        let prod = text.split("#[cfg(test)]").next().unwrap_or("");
        if prod.contains("ContextItem::enterprise_standard(") {
            assert!(
                path.contains("crates/eaeh-knowledge/src/"),
                "enterprise standards may only be produced by eaeh-knowledge: {path}"
            );
        }
        if prod.contains("ContextItem::skill_instruction(") {
            assert!(
                path.contains("crates/eaeh-skills/src/"),
                "skill instructions may only be produced by eaeh-skills: {path}"
            );
        }
    }
}

/// ADR-0026: the automated external execution path (coordinator, Claude Code adapter, CLI
/// worker) has no route to a model API: no HTTP client, no model endpoint, no API-key
/// handling, and never Claude Code's API-key-only `--bare` mode.
#[test]
fn automated_execution_has_no_model_api_path() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    for f in [
        "crates/eaeh-runtime/src/coordinator/mod.rs",
        "crates/eaeh-runtime/src/coordinator/claude.rs",
        "crates/eaeh-cli/src/worker.rs",
    ] {
        let src = std::fs::read_to_string(root.join(f)).unwrap();
        // Only the code above the unit tests (tests mention the forbidden strings on purpose).
        let code = src.split("#[cfg(test)]").next().unwrap();
        for forbidden in [
            "reqwest",
            "hyper::",
            "api.anthropic.com",
            "x-api-key",
            "eaeh_provider_anthropic",
            "ModelClient",
            "\"--bare\"",
            "dangerously-skip-permissions",
        ] {
            assert!(!code.contains(forbidden), "{f} must not contain {forbidden}");
        }
    }
}
