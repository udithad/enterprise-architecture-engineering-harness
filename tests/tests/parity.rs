//! Scenario 14: every CLI operation is classified in the CLI-to-Portal parity inventory
//! (`docs/implementation/EAEH-PORTAL-CLI-PARITY.md`), exactly once, with a valid class, and
//! every exclusion states its reason.
//!
//! The operations are derived from the `eaeh` command tree in `crates/eaeh-cli/src/main.rs`
//! (clap derive enums: `Cmd` and its `#[command(subcommand)]` enums), so a new command fails
//! this test until it is classified.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// clap's default naming: `SetTeams` → `set-teams`.
fn kebab(s: &str) -> String {
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                o.push('-');
            }
            o.push(c.to_ascii_lowercase());
        } else {
            o.push(c);
        }
    }
    o
}

/// Variant names of every `enum X { … }` at brace depth 0 of its body.
fn enums(src: &str) -> BTreeMap<String, Vec<(String, Option<String>)>> {
    let mut out = BTreeMap::new();
    let mut lines = src.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(name) = line.strip_prefix("enum ").and_then(|r| r.strip_suffix(" {")) else {
            continue;
        };
        let mut depth = 0i32;
        let mut variants = vec![];
        for l in lines.by_ref() {
            let t = l.trim();
            if depth == 0 && t == "}" {
                break;
            }
            if depth == 0 && t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                let ident: String = t.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                // `Workflow(WorkflowCmd)` → a subcommand enum.
                let sub = t
                    .strip_prefix(&ident)
                    .and_then(|r| r.strip_prefix('('))
                    .and_then(|r| r.split(')').next())
                    .map(str::to_string);
                variants.push((ident, sub));
            }
            depth += l.matches('{').count() as i32 - l.matches('}').count() as i32;
        }
        out.insert(name.to_string(), variants);
    }
    out
}

fn cli_operations() -> BTreeSet<String> {
    let src = std::fs::read_to_string(root().join("crates/eaeh-cli/src/main.rs")).unwrap();
    let e = enums(&src);
    let mut ops = BTreeSet::new();
    for (v, sub) in &e["Cmd"] {
        match sub {
            Some(s) => {
                let subs = e
                    .get(s)
                    .unwrap_or_else(|| panic!("subcommand enum {s} not found"));
                assert!(!subs.is_empty(), "{s} has no variants");
                for (sv, _) in subs {
                    ops.insert(format!("eaeh {} {}", kebab(v), kebab(sv)));
                }
            }
            None => {
                ops.insert(format!("eaeh {}", kebab(v)));
            }
        }
    }
    ops
}

#[test]
fn every_cli_operation_is_classified_once_with_reasons_for_exclusions() {
    let ops = cli_operations();
    assert!(ops.len() >= 77, "the command tree was not parsed: {ops:?}");
    assert!(
        ops.contains("eaeh workflow restart")
            && ops.contains("eaeh project init")
            && ops.contains("eaeh review")
    );
    let doc = std::fs::read_to_string(root().join("docs/implementation/EAEH-PORTAL-CLI-PARITY.md")).unwrap();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for line in doc.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // | `cmd` | Class | capability | reason |
        if cells.len() < 6 {
            continue;
        }
        let Some(cmd) = cells[1].strip_prefix('`').and_then(|c| c.strip_suffix('`')) else {
            continue;
        };
        if !cmd.starts_with("eaeh") {
            continue;
        }
        let class = cells[2];
        assert!(
            ["Action", "Read", "CLI", "N/A"].contains(&class),
            "{cmd}: invalid class '{class}'"
        );
        if matches!(class, "CLI" | "N/A") {
            assert!(cells[4].len() > 10, "{cmd}: an excluded command must state why");
        } else {
            assert!(
                cells[3].len() > 5 && cells[3] != "–",
                "{cmd}: a portal capability must name where it is"
            );
        }
        assert!(
            seen.insert(cmd.to_string(), class.to_string()).is_none(),
            "{cmd} is listed twice"
        );
    }
    let listed: BTreeSet<String> = seen.keys().cloned().collect();
    let missing: Vec<_> = ops.difference(&listed).collect();
    let unknown: Vec<_> = listed.difference(&ops).collect();
    assert!(
        missing.is_empty(),
        "CLI operations missing from the parity inventory: {missing:?}"
    );
    assert!(
        unknown.is_empty(),
        "inventory rows for commands that do not exist: {unknown:?}"
    );
    let count = |c: &str| seen.values().filter(|v| *v == c).count();
    eprintln!(
        "parity: {} operations – Action {}, Read {}, CLI {}, N/A {}",
        seen.len(),
        count("Action"),
        count("Read"),
        count("CLI"),
        count("N/A")
    );
}
