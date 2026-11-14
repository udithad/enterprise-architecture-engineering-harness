//! Governed software delivery (`engineering.delivery`, ADR-0025): deterministic validators
//! for the implementation plan, the implementation and the build/test result, and the
//! `delivery.snapshot` tool, which turns the task workspace into an approvable,
//! exportable source bundle with requirement → file/test trace links.
//!
//! The reasoning (planning, coding, writing tests) is done by an agent: an external AI
//! client in `external_ai` mode, or a model in the other modes. Everything here is
//! deterministic and model-free.

use async_trait::async_trait;
use eaeh_core::{HarnessError, Result, RiskClass, ToolId};
use eaeh_tools::{Tool, ToolContext, ToolDescriptor};
use eaeh_validation::{Finding, Severity, ValidationReport, ValidationSubject, Validator, ValidatorKind};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const PLAN_VALIDATOR: &str = "delivery.plan";
pub const IMPLEMENTATION_VALIDATOR: &str = "delivery.implementation";
pub const BUILD_VALIDATOR: &str = "delivery.build";

/// Snapshot limits: files, bytes per file, total bytes.
const MAX_FILES: usize = 500;
const MAX_FILE: usize = 1024 * 1024;
const MAX_TOTAL: usize = 8 * 1024 * 1024;

fn finding(v: &str, sev: Severity, msg: impl Into<String>, loc: Option<String>) -> Finding {
    Finding {
        validator: v.into(),
        severity: sev,
        message: msg.into(),
        location: loc,
        evidence: Value::Null,
    }
}

fn parts(s: &ValidationSubject) -> (&Value, &Value) {
    (
        s.content.get("output").unwrap_or(&Value::Null),
        s.content.get("context").unwrap_or(&Value::Null),
    )
}

/// `(id, priority, kind)` of every requirement in the context's approved requirement set.
fn requirements(ctx: &Value) -> Vec<(String, String, String)> {
    ctx.pointer("/requirements/requirements")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    Some((
                        r.get("id")?.as_str()?.to_string(),
                        r.get("priority")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_ascii_lowercase(),
                        r.get("kind").and_then(Value::as_str).unwrap_or("").to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default()
}

/// Repository-relative path without traversal, absolute roots, backslashes or hidden
/// EAEH internals.
pub fn safe_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 512
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains('\0')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
        && !p.starts_with(".eaeh")
}

pub fn plan_findings(s: &ValidationSubject) -> Vec<Finding> {
    let v = PLAN_VALIDATOR;
    let (plan, ctx) = parts(s);
    let mut f = vec![];
    let reqs = requirements(ctx);
    if reqs.is_empty() {
        return vec![finding(
            v,
            Severity::Error,
            "no approved requirements in the validation context",
            None,
        )];
    }
    let known: BTreeSet<&str> = reqs.iter().map(|r| r.0.as_str()).collect();
    let tasks = plan
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if tasks.is_empty() {
        f.push(finding(v, Severity::Error, "the plan has no tasks", None));
    }
    let mut ids = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for (i, t) in tasks.iter().enumerate() {
        let id = t
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let loc = Some(format!("tasks[{i}]"));
        if id.is_empty() || !ids.insert(id.clone()) {
            f.push(finding(
                v,
                Severity::Error,
                format!("task id '{id}' is missing or duplicated"),
                loc.clone(),
            ));
        }
        let rs = strs(t.get("requirements"));
        if rs.is_empty() {
            f.push(finding(
                v,
                Severity::Error,
                format!("task {id} implements no requirement"),
                loc.clone(),
            ));
        }
        for r in rs {
            if known.contains(r.as_str()) {
                covered.insert(r);
            } else {
                f.push(finding(
                    v,
                    Severity::Error,
                    format!("task {id} references unknown requirement {r}"),
                    loc.clone(),
                ));
            }
        }
        for p in strs(t.get("files")).iter().chain(strs(t.get("tests")).iter()) {
            if !safe_path(p) {
                f.push(finding(
                    v,
                    Severity::Error,
                    format!("task {id}: unsafe path '{p}'"),
                    loc.clone(),
                ));
            }
        }
    }
    for (id, prio, _) in &reqs {
        if covered.contains(id) {
            continue;
        }
        match prio.as_str() {
            "must" => f.push(finding(
                v,
                Severity::Error,
                format!("Must requirement {id} is not covered by any task"),
                None,
            )),
            "wont" => {}
            _ => f.push(finding(
                v,
                Severity::Warning,
                format!("{prio} requirement {id} is not covered by any task"),
                None,
            )),
        }
    }
    f
}

pub fn implementation_findings(s: &ValidationSubject) -> Vec<Finding> {
    let v = IMPLEMENTATION_VALIDATOR;
    let (imp, ctx) = parts(s);
    let mut f = vec![];
    let reqs = requirements(ctx);
    let known: BTreeSet<&str> = reqs.iter().map(|r| r.0.as_str()).collect();
    let mut impl_by: BTreeMap<String, usize> = BTreeMap::new();
    let mut verified_by: BTreeMap<String, usize> = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for (key, counter) in [("files", &mut impl_by), ("tests", &mut verified_by)] {
        let items = imp
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if items.is_empty() {
            f.push(finding(
                v,
                Severity::Error,
                format!("the implementation lists no {key}"),
                None,
            ));
        }
        for (i, it) in items.iter().enumerate() {
            let loc = Some(format!("{key}[{i}]"));
            let p = it.get("path").and_then(Value::as_str).unwrap_or_default();
            if !safe_path(p) {
                f.push(finding(
                    v,
                    Severity::Error,
                    format!("unsafe or missing path '{p}'"),
                    loc.clone(),
                ));
            } else if !paths.insert(p.to_string()) {
                f.push(finding(
                    v,
                    Severity::Error,
                    format!("path '{p}' is listed twice"),
                    loc.clone(),
                ));
            }
            for r in strs(it.get("requirements")) {
                if known.contains(r.as_str()) {
                    *counter.entry(r).or_default() += 1;
                } else {
                    f.push(finding(
                        v,
                        Severity::Error,
                        format!("{p} references unknown requirement {r}"),
                        loc.clone(),
                    ));
                }
            }
        }
    }
    for (id, prio, kind) in &reqs {
        if prio != "must" {
            continue;
        }
        let functional = kind == "functional";
        if functional && !impl_by.contains_key(id) {
            f.push(finding(
                v,
                Severity::Error,
                format!("Must requirement {id} is not implemented by any listed file"),
                None,
            ));
        }
        if !verified_by.contains_key(id) {
            f.push(finding(
                v,
                if functional {
                    Severity::Error
                } else {
                    Severity::Warning
                },
                format!("Must requirement {id} is not verified by any listed test"),
                None,
            ));
        }
    }
    f
}

pub fn build_findings(s: &ValidationSubject) -> Vec<Finding> {
    let v = BUILD_VALIDATOR;
    let (out, _) = parts(s);
    let code = out.get("exit_code").and_then(Value::as_i64);
    let timed_out = out.get("timed_out").and_then(Value::as_bool).unwrap_or(false);
    if code == Some(0) && !timed_out {
        return vec![];
    }
    let tail: String = {
        let e = out.get("stderr").and_then(Value::as_str).unwrap_or_default();
        let o = out.get("stdout").and_then(Value::as_str).unwrap_or_default();
        let t = if e.trim().is_empty() { o } else { e };
        let n = t.chars().count();
        t.chars().skip(n.saturating_sub(1500)).collect()
    };
    vec![finding(
        v,
        Severity::Error,
        format!(
            "build/test failed (exit code {}, timed out: {timed_out}): {tail}",
            code.map(|c| c.to_string()).unwrap_or_else(|| "none".into())
        ),
        None,
    )]
}

macro_rules! validator {
    ($name:ident, $id:expr, $kind:literal, $f:path) => {
        pub struct $name;
        #[async_trait]
        impl Validator for $name {
            fn id(&self) -> &str {
                $id
            }
            fn version(&self) -> &str {
                "1.0.0"
            }
            fn kind(&self) -> ValidatorKind {
                ValidatorKind::Deterministic
            }
            fn applies_to(&self, s: &ValidationSubject) -> bool {
                $kind.is_empty() || parts(s).0.get("document").and_then(Value::as_str) == Some($kind)
            }
            async fn validate(&self, s: &ValidationSubject) -> ValidationReport {
                ValidationReport {
                    validator: $id.into(),
                    version: "1.0.0".into(),
                    kind: ValidatorKind::Deterministic,
                    findings: $f(s),
                }
            }
        }
    };
}

validator!(
    DeliveryPlanValidator,
    PLAN_VALIDATOR,
    "implementation_plan",
    plan_findings
);
validator!(
    ImplementationValidator,
    IMPLEMENTATION_VALIDATOR,
    "implementation",
    implementation_findings
);
validator!(BuildResultValidator, BUILD_VALIDATOR, "", build_findings);

/// `delivery.snapshot`: the task workspace as a source bundle (`files`) with trace links
/// (`requirement -implemented_by-> source_file`, `requirement -verified_by-> test_file`)
/// and a delivery summary. Fails if a file the implementation lists does not exist.
pub struct DeliverySnapshotTool {
    d: ToolDescriptor,
}

impl DeliverySnapshotTool {
    pub fn new() -> Result<Self> {
        Ok(Self {
            d: ToolDescriptor {
                id: ToolId::new("delivery.snapshot")?,
                version: semver::Version::new(1, 0, 0),
                description: "Snapshot the task workspace into a source bundle with requirement trace links."
                    .into(),
                risk: RiskClass::ReadOnly,
                input_schema: json!({"type":"object","additionalProperties":false,"required":["implementation"],
                    "properties":{"implementation":{"type":"object"},"verification":{"type":"string","maxLength":200}}}),
                output_schema: json!({"type":"object","required":["files","trace"]}),
                isolation: Default::default(),
                needs_execution: false,
                needs_workspace: true,
                timeout_ms: 30_000,
                rate_limit_per_minute: Some(60),
                idempotent: true,
                max_retries: 0,
            },
        })
    }
}

#[async_trait]
impl Tool for DeliverySnapshotTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.d
    }
    async fn invoke(&self, ctx: &ToolContext, input: Value) -> Result<Value> {
        let ws = ctx.workspace()?;
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        let mut total = 0usize;
        let mut dirs = vec![None::<String>];
        while let Some(d) = dirs.pop() {
            for e in ws.list(d.as_deref())? {
                if e.is_dir {
                    dirs.push(Some(e.path));
                    continue;
                }
                if !safe_path(&e.path) {
                    continue;
                }
                if files.len() >= MAX_FILES {
                    return Err(HarnessError::invalid_input(format!(
                        "workspace has more than {MAX_FILES} files"
                    )));
                }
                let bytes = ws.read_file(&e.path, MAX_FILE)?;
                total += bytes.len();
                if total > MAX_TOTAL {
                    return Err(HarnessError::invalid_input(format!(
                        "workspace exceeds {MAX_TOTAL} bytes"
                    )));
                }
                let text = String::from_utf8(bytes)
                    .map_err(|_| HarnessError::invalid_input(format!("{} is not UTF-8 text", e.path)))?;
                files.insert(e.path, text);
            }
        }
        let imp = &input["implementation"];
        let mut trace = vec![];
        let mut rows = vec![];
        for (key, relation, kind) in [
            ("files", "implemented_by", "source_file"),
            ("tests", "verified_by", "test_file"),
        ] {
            for it in imp
                .get(key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let p = it
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if !files.contains_key(&p) {
                    return Err(HarnessError::invalid_input(format!(
                        "the implementation lists '{p}' but the workspace has no such file"
                    )));
                }
                let reqs = strs(it.get("requirements"));
                for r in &reqs {
                    trace.push(json!({"from_kind": "requirement", "from_id": r, "relation": relation, "to_kind": kind, "to_id": p}));
                }
                rows.push(format!("| `{p}` | {kind} | {} |", reqs.join(", ")));
            }
        }
        let verification = input
            .get("verification")
            .and_then(Value::as_str)
            .unwrap_or("build and tests are executed by EAEH in the next workflow step");
        let summary = format!(
            "# Delivery\n\n{}\n\n**Verification:** {verification}\n\n| Path | Kind | Requirements |\n|---|---|---|\n{}\n\nFiles in bundle: {}\n",
            imp.get("summary").and_then(Value::as_str).unwrap_or(""),
            rows.join("\n"),
            files.len()
        );
        let n = files.len();
        files.insert("EAEH-DELIVERY.md".into(), summary);
        Ok(json!({
            "document": "delivery",
            "files": files,
            "trace": trace,
            "summary": {"files": n, "trace_links": trace.len(), "verification": verification},
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(output: Value) -> ValidationSubject {
        ValidationSubject {
            kind: "workflow_output".into(),
            name: "t".into(),
            content: json!({"output": output, "context": {"requirements": {"requirements": [
                {"id": "REQ-F-001", "priority": "must", "kind": "functional"},
                {"id": "REQ-NF-001", "priority": "must", "kind": "non_functional"},
                {"id": "REQ-F-002", "priority": "should", "kind": "functional"},
            ]}}}),
            metadata: Default::default(),
        }
    }

    fn errors(f: &[Finding]) -> Vec<String> {
        f.iter()
            .filter(|x| x.severity >= Severity::Error)
            .map(|x| x.message.clone())
            .collect()
    }

    #[test]
    fn paths_must_stay_inside_the_repository() {
        for ok in ["pom.xml", "src/main/java/App.java"] {
            assert!(safe_path(ok), "{ok}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            "a\\b",
            ".eaeh/project.toml",
            "./a",
        ] {
            assert!(!safe_path(bad), "{bad}");
        }
    }

    #[test]
    fn plan_must_cover_every_must_requirement_with_known_ids() {
        let f = plan_findings(&subject(json!({"document": "implementation_plan", "tasks": [
            {"id": "T-001", "requirements": ["REQ-F-001", "REQ-X-9"], "files": ["../x"], "tests": []},
        ]})));
        let e = errors(&f).join("\n");
        assert!(e.contains("REQ-NF-001 is not covered"), "{e}");
        assert!(e.contains("unknown requirement REQ-X-9"), "{e}");
        assert!(e.contains("unsafe path '../x'"), "{e}");
        assert!(
            f.iter()
                .any(|x| x.severity == Severity::Warning && x.message.contains("REQ-F-002"))
        );
        let ok = plan_findings(&subject(json!({"document": "implementation_plan", "tasks": [
            {"id": "T-001", "requirements": ["REQ-F-001", "REQ-NF-001"], "files": ["a.java"], "tests": ["t.java"]},
        ]})));
        assert!(errors(&ok).is_empty(), "{ok:?}");
    }

    #[test]
    fn implementation_must_implement_and_test_must_functional_requirements() {
        let f = implementation_findings(&subject(json!({"document": "implementation",
            "files": [{"path": "a.java", "requirements": ["REQ-NF-001"]}],
            "tests": [{"path": "a.java", "requirements": []}]})));
        let e = errors(&f).join("\n");
        assert!(e.contains("REQ-F-001 is not implemented"), "{e}");
        assert!(e.contains("REQ-F-001 is not verified"), "{e}");
        assert!(e.contains("listed twice"), "{e}");
        let ok = implementation_findings(&subject(json!({"document": "implementation",
            "files": [{"path": "a.java", "requirements": ["REQ-F-001", "REQ-NF-001"]}],
            "tests": [{"path": "t.java", "requirements": ["REQ-F-001", "REQ-NF-001"]}]})));
        assert!(errors(&ok).is_empty(), "{ok:?}");
    }

    #[test]
    fn build_result_blocks_unless_exit_zero() {
        let s = |v: Value| ValidationSubject {
            kind: "workflow_output".into(),
            name: "b".into(),
            content: json!({"output": v, "context": {}}),
            metadata: Default::default(),
        };
        assert!(build_findings(&s(json!({"exit_code": 0, "timed_out": false}))).is_empty());
        let f = build_findings(&s(
            json!({"exit_code": 1, "timed_out": false, "stderr": "Tests run: 3, Failures: 1"}),
        ));
        assert!(f[0].message.contains("Failures: 1"));
        assert_eq!(
            build_findings(&s(json!({"exit_code": 0, "timed_out": true}))).len(),
            1
        );
    }
}
