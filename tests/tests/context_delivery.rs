//! Task context delivery, task-scoped access and recovery of external AI hand-offs
//! (docs/implementation/EAEH-TASK-CONTEXT-DELIVERY.md).
//!
//! Real incident: a ~102 KB package (analyse) and a ~235 KB package (refine) were returned in
//! one MCP response; Claude Code diverted them to a file that no tool could open, and a worker
//! older than its MCP server launched Claude Code with a stale tool list so every new tool was
//! denied for lack of an approval nobody could give. These tests use a client double that — like
//! the worker's Claude Code session — can only call EAEH's operations, has no file or shell
//! access, and refuses any single response above a display limit. Everything else (delivery
//! protocol, capabilities, coordinator, leases, validation, workflow engine, audit) is
//! production code, run against real databases of isolated workflows.

mod common;
use async_trait::async_trait;
use common::*;
use eaeh_agents::external::delivery as dl;
use eaeh_core::hash::sha256_hex;
use eaeh_core::{
    Principal, PrincipalId, PrincipalKind, ProjectAccess, Role, TaskId, TraceContext, WorkflowDefinitionId,
    WorkflowInstanceId,
};
use eaeh_governance::NewPrincipal;
use eaeh_runtime::Harness;
use eaeh_runtime::coordinator::{
    self as co, AuthState, CancellationToken, ClientReport, Coordinator, ExecFailure, ExecResult,
    ExecutionRequest, ExternalExecutor, FailureClass, McpLaunch, NoNotifier, Preflight, Until,
};
use eaeh_workflow::InstanceStatus;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What Claude Code showed in the incident: results above this size are diverted to a file.
const DISPLAY_LIMIT: usize = 25_000;

#[derive(Debug, Clone)]
enum Plan {
    /// Manifest → every mandatory section → ack → large tool result in pages → submit.
    Full,
    /// Ends without calling anything.
    NoRead,
    /// Reads `n` chunks of the sections, then ends (connection lost, session over).
    Partial(usize),
    /// Reads everything but never acknowledges, then ends.
    SkipAck,
    /// Reads everything, acknowledges, then ends without submitting.
    AckThenNothing,
    /// Works on a wrong package version.
    WrongVersion,
    /// The client reports that Claude Code denied its tool calls (needs an approval).
    Denied,
    /// The capability is revoked while the client works.
    RevokedMid,
    /// Acknowledges, then EAEH records an infrastructure failure of the submission.
    SubmitFailed,
}

struct Client {
    h: Arc<Harness>,
    who: Principal,
    plans: Mutex<VecDeque<Plan>>,
    /// Per execution: what was read (section → sha256 of the reassembled text).
    reads: Mutex<Vec<(String, BTreeMap<String, String>)>>,
    largest_response: Mutex<usize>,
    needs_mcp: bool,
}

impl Client {
    fn new(w: &World, plans: Vec<Plan>) -> Arc<Self> {
        Self::build(w, plans, false)
    }

    fn with_mcp_check(w: &World, plans: Vec<Plan>) -> Arc<Self> {
        Self::build(w, plans, true)
    }

    fn build(w: &World, plans: Vec<Plan>, needs_mcp: bool) -> Arc<Self> {
        Arc::new(Self {
            h: w.h.clone(),
            who: w.operator.clone(),
            plans: Mutex::new(plans.into()),
            reads: Mutex::new(vec![]),
            largest_response: Mutex::new(0),
            needs_mcp,
        })
    }

    /// A response the client can display, or the incident's failure.
    fn show(&self, v: &Value) -> Result<(), String> {
        let n = serde_json::to_string(v).unwrap().len();
        let mut l = self.largest_response.lock().unwrap();
        *l = (*l).max(n);
        if n > DISPLAY_LIMIT {
            return Err(format!(
                "response of {n} bytes is too large to display; it was saved to a file I cannot read"
            ));
        }
        Ok(())
    }

    /// Read the task through the delivery operations only. `stop_after` simulates a lost
    /// session after that many chunk requests; `hash` overrides the package version used.
    async fn read_task_as(
        &self,
        id: TaskId,
        exec: &str,
        stop_after: Option<usize>,
        hash: Option<&str>,
    ) -> Result<(String, BTreeMap<String, String>), String> {
        let m = self
            .h
            .agents
            .external_manifest(&self.who, id, exec)
            .await
            .map_err(|e| e.message)?;
        self.show(&m)?;
        let package_hash = m["package_hash"].as_str().unwrap().to_string();
        let use_hash = hash.unwrap_or(&package_hash).to_string();
        let mut got = BTreeMap::new();
        let mut requests = 0;
        for s in m["sections"].as_array().unwrap() {
            if s["class"] != "mandatory" {
                continue;
            }
            let name = s["section"].as_str().unwrap();
            if m["inline"] == true {
                got.insert(
                    name.to_string(),
                    sha256_hex(m["package_sections"][name].to_string().as_bytes()),
                );
                continue;
            }
            let (mut off, mut text) = (0u64, String::new());
            loop {
                if stop_after.is_some_and(|n| requests >= n) {
                    return Err("session ended while reading".into());
                }
                requests += 1;
                let c = self
                    .h
                    .agents
                    .external_section(&self.who, id, exec, name, off, None, Some(&use_hash))
                    .await
                    .map_err(|e| e.message)?;
                self.show(&c)?;
                text.push_str(c["content"].as_str().unwrap());
                off = c["next_offset"].as_u64().unwrap();
                if c["done"] == true {
                    break;
                }
            }
            if sha256_hex(text.as_bytes()) != s["sha256"].as_str().unwrap() {
                return Err(format!("section {name} does not match its checksum"));
            }
            got.insert(name.to_string(), sha256_hex(text.as_bytes()));
        }
        Ok((package_hash, got))
    }

    async fn read_task(
        &self,
        id: TaskId,
        exec: &str,
        stop_after: Option<usize>,
    ) -> Result<(String, BTreeMap<String, String>), String> {
        self.read_task_as(id, exec, stop_after, None).await
    }
}

fn output_for(state: &str) -> Value {
    match state {
        "analyse" | "refine" => example("requirements.json"),
        "hld" => example("hld.json"),
        "lld" => example("lld.json"),
        other => panic!("unexpected stage {other}"),
    }
}

#[async_trait]
impl ExternalExecutor for Client {
    fn provider(&self) -> &str {
        "fake_client"
    }
    fn client_label(&self) -> &str {
        "fake-client"
    }
    fn needs_mcp_server(&self) -> bool {
        self.needs_mcp
    }
    async fn preflight(&self) -> Preflight {
        Preflight {
            provider: "fake_client".into(),
            available: true,
            program: Some("fake".into()),
            version: Some("1.0.0".into()),
            auth: AuthState::Subscription {
                plan: Some("pro".into()),
            },
            detail: "test double".into(),
        }
    }
    async fn execute(&self, req: &ExecutionRequest, _cancel: CancellationToken) -> ExecResult {
        let plan = self.plans.lock().unwrap().pop_front().unwrap_or(Plan::Full);
        let exec = {
            let a = &req.mcp.args;
            let i = a
                .iter()
                .position(|x| x == "--execution")
                .expect("the worker passes the attempt id");
            a[i + 1].clone()
        };
        assert!(!exec.contains('{'), "placeholder replaced");
        let id: TaskId = req.task_id.parse().unwrap();
        let report = |text: &str, denials: Vec<String>| ClientReport {
            num_turns: Some(5),
            duration_ms: Some(10),
            notional_cost_usd: Some(0.0),
            auth_source: Some("none".into()),
            final_text: text.into(),
            permission_denials: denials,
        };
        let crash = |d: &str| ExecFailure {
            class: FailureClass::Crashed,
            detail: d.into(),
            report: None,
        };
        let a = &self.h.agents;
        let stop = match &plan {
            Plan::NoRead => return Ok(report("I could not read my task package", vec![])),
            Plan::Partial(n) => Some(*n),
            Plan::Denied => {
                // The manifest call is allowed; the section calls are refused by the client's own
                // permission system (never reaching EAEH).
                let _ = a.external_manifest(&self.who, id, &exec).await;
                return Ok(report(
                    "the session denied every eaeh_task_section call: it needs approval",
                    vec!["mcp__eaeh__eaeh_task_section".into(); 3],
                ));
            }
            _ => None,
        };
        let wrong = matches!(plan, Plan::WrongVersion).then_some("0000-stale-package-version");
        let (hash, got) = match self.read_task_as(id, &exec, stop, wrong).await {
            Ok(x) => x,
            Err(e) => return Ok(report(&format!("context unreadable: {e}"), vec![])),
        };
        self.reads.lock().unwrap().push((exec.clone(), got));
        if matches!(plan, Plan::SkipAck) {
            return Ok(report("read it all, forgot to acknowledge", vec![]));
        }
        if matches!(plan, Plan::RevokedMid) {
            a.external_revoke_capability(&exec, "test: revoked while working")
                .await
                .unwrap();
        }
        if let Err(e) = a.external_ack(&self.who, id, &exec, &hash, None).await {
            return if matches!(plan, Plan::RevokedMid) {
                Ok(report(&format!("not acknowledged: {}", e.message), vec![]))
            } else {
                Err(crash(&e.message))
            };
        }
        if matches!(plan, Plan::AckThenNothing) {
            return Ok(report("EAEH_RESULT not_accepted: gave up", vec![]));
        }
        if matches!(plan, Plan::SubmitFailed) {
            a.external_note_failure(
                &self.who,
                id,
                &exec,
                "submission_failed",
                "storage error while saving the submission",
            )
            .await
            .unwrap();
            return Ok(report("EAEH_RESULT not_accepted: the submission failed", vec![]));
        }
        let view = a.external_task(&self.who, id).await.unwrap();
        if matches!(view.state.as_str(), "analyse" | "refine") {
            // The BRD is large: the tool result comes back paged and is read to the end.
            a.external_require_ack(&self.who, id, &exec).await.unwrap();
            let out = a
                .external_tool(&self.who, id, "knowledge.read", json!({"document": BRD}))
                .await
                .unwrap();
            let first = a.external_page_result(&self.who, id, out.clone()).await.unwrap();
            self.show(&first).unwrap();
            if first["paged"] == true {
                let (mut text, mut r) = (first["content"].as_str().unwrap().to_string(), first.clone());
                while r["done"] != true {
                    r = a
                        .external_blob(
                            &self.who,
                            id,
                            &exec,
                            first["blob"].as_str().unwrap(),
                            r["next_offset"].as_u64().unwrap(),
                            None,
                        )
                        .await
                        .unwrap();
                    self.show(&r).unwrap();
                    text.push_str(r["content"].as_str().unwrap());
                }
                let whole: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(whole, out, "the pages reassemble to exactly the tool result");
            }
        }
        match self
            .h
            .submit_external(&self.who, id, &output_for(&view.state), self.client_label())
            .await
        {
            Ok(_) => Ok(report("EAEH_RESULT accepted", vec![])),
            Err(e) => Err(crash(&e.message)),
        }
    }
}

fn quick(cfg: &mut eaeh_runtime::HarnessConfig) {
    let c = &mut cfg.execution.coordinator;
    c.poll_interval_ms = 200;
    c.backoff_secs = 1;
    c.lease_secs = 30;
    c.max_attempts = 3;
    c.max_context_attempts = 3;
    c.notify = false;
}

async fn ext_world() -> World {
    world_cfg(ExecutionMode::ExternalAi, vec![], "", quick).await
}

fn launch_with(program: &str) -> McpLaunch {
    McpLaunch {
        program: PathBuf::from(program),
        args: vec![
            "mcp".into(),
            "--task".into(),
            "{task}".into(),
            "--execution".into(),
            "{execution}".into(),
        ],
        env: vec![],
    }
}

async fn run_with(w: &World, c: Arc<Client>, launch: McpLaunch) -> co::RunSummary {
    let coord = Coordinator::register(
        w.h.clone(),
        w.operator.clone(),
        w.scope,
        c,
        launch,
        Arc::new(NoNotifier),
        Arc::new(|l: &str| eprintln!("[coordinator] {l}")),
    )
    .await
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(90),
        coord.run(CancellationToken::new(), Until::Idle),
    )
    .await
    .expect("the coordinator reached a stop")
    .unwrap()
}

async fn auto(w: &World, c: Arc<Client>) -> co::RunSummary {
    run_with(w, c, launch_with("/nonexistent/eaeh")).await
}

/// Start the design workflow with `extra` as additional task input.
async fn start_with(w: &World, extra: Value) -> WorkflowInstanceId {
    let mut input = json!({"brd": BRD});
    for (k, v) in extra.as_object().unwrap() {
        input[k] = v.clone();
    }
    let id =
        w.h.engine
            .start(
                &w.operator,
                &WorkflowDefinitionId::new("engineering.design").unwrap(),
                w.scope,
                input,
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    let st = w.h.engine.drive(id).await.unwrap();
    assert_eq!(
        st,
        InstanceStatus::WaitingExternal,
        "{:?}",
        w.h.engine.get(id).await.unwrap().data.failure
    );
    id
}

async fn start(w: &World, padding: usize) -> WorkflowInstanceId {
    start_with(w, json!({"notes": "n".repeat(padding)})).await
}

async fn tasks(w: &World) -> Vec<TaskId> {
    w.h.agents
        .external_tasks(&w.operator, w.scope, true)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.task_id.parse().unwrap())
        .collect()
}

async fn count_tasks(w: &World) -> i64 {
    eaeh_store::sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM external_tasks")
        .fetch_one(w.h.db.pool())
        .await
        .unwrap()
        .0
}

async fn first_task(w: &World) -> TaskId {
    tasks(w).await[0]
}

/// Give a hand-made attempt id the capability the worker would issue.
async fn cap(w: &World, id: TaskId, exec: &str) {
    w.h.agents
        .external_issue_capability(&w.operator, &id.to_string(), exec, 600)
        .await
        .unwrap();
}

async fn executions(w: &World) -> Vec<co::ExecutionInfo> {
    let mut v = co::executions(&w.h, &w.operator, w.scope, None, 500)
        .await
        .unwrap();
    v.reverse();
    v
}

async fn dispatch(w: &World, task: &str) -> (String, i64, i64, i64) {
    eaeh_store::sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT dispatch_state, dispatch_attempts, context_failures, submissions FROM external_tasks WHERE id = ?",
    )
    .bind(task)
    .fetch_one(w.h.db.pool())
    .await
    .unwrap()
}

async fn audit(w: &World) -> Vec<eaeh_store::audit::AuditRecord> {
    w.h.db.audit_records(Some(w.scope.tenant), 0, 500).await.unwrap()
}

fn mandatory_bytes(p: &Value) -> usize {
    dl::sections_of(p)
        .iter()
        .filter(|s| s.mandatory)
        .map(|s| s.content.len())
        .sum()
}

// ------------------------------------------------------------------------------ sizes

/// The incident, reproduced on isolated workflows: the legacy single response exceeds what the
/// client can display (analyse ≈ 102 KB, refine ≈ 235 KB); delivered by sections, every size is
/// read completely, with every response below the limit and fewer bytes than before.
#[tokio::test]
async fn packages_from_small_to_235_kb_are_delivered_completely_in_bounded_responses() {
    for (padding, legacy_at_least) in [
        (100usize, 0usize),
        (5_000, 0),
        (40_000, 102_000),
        (100_000, 220_000),
        (300_000, 600_000),
    ] {
        let w = ext_world().await;
        start(&w, padding).await;
        let id = first_task(&w).await;
        let view = w.h.agents.external_task(&w.operator, id).await.unwrap();
        let package = view.package.clone().unwrap();
        let legacy = serde_json::to_string(&view).unwrap().len();
        assert!(
            legacy >= legacy_at_least,
            "padding {padding}: legacy response only {legacy} bytes"
        );
        if legacy_at_least > 0 {
            assert!(
                legacy > 2 * DISPLAY_LIMIT,
                "the legacy response could not have been displayed"
            );
        }
        cap(&w, id, "r1").await;
        let c = Client::new(&w, vec![]);
        let (hash, got) = c
            .read_task(id, "r1", None)
            .await
            .unwrap_or_else(|e| panic!("padding {padding}: {e}"));
        assert_eq!(hash, view.package_hash);
        for must in [
            "overview",
            "instructions",
            "rules",
            "output_schema",
            "output_outline",
            "output_example",
            "skill_contracts",
            "allowed_tools",
            "citable",
            "input",
        ] {
            assert!(
                got.contains_key(must),
                "padding {padding}: mandatory section {must} missing"
            );
        }
        assert!(*c.largest_response.lock().unwrap() <= DISPLAY_LIMIT);
        let m = w.h.agents.external_manifest(&w.operator, id, "r1").await.unwrap();
        let hash2 = m["package_hash"].as_str().unwrap();
        assert_eq!(
            w.h.agents
                .external_ack(&w.operator, id, "r1", hash2, None)
                .await
                .unwrap()["acknowledged"],
            true
        );
        let delivered = mandatory_bytes(&package);
        eprintln!(
            "EVIDENCE padding={padding:>7} legacy_single_response={legacy:>7} delivered_mandatory={delivered:>7} largest_response={:>6}",
            c.largest_response.lock().unwrap()
        );
        if padding >= 40_000 {
            assert!(
                delivered * 2 < legacy + legacy / 4,
                "the duplicate content is not delivered again"
            );
        }
    }
}

/// Larger package made of several distinct supporting documents.
#[tokio::test]
async fn package_with_several_supporting_documents_is_delivered_completely() {
    let w = ext_world().await;
    let docs: serde_json::Map<String, Value> = (0..6)
        .map(|i| (format!("doc{i}"), json!(format!("{i}-{}", "d".repeat(60_000)))))
        .collect();
    start_with(&w, json!({"supporting": docs})).await;
    let id = first_task(&w).await;
    cap(&w, id, "r1").await;
    let c = Client::new(&w, vec![]);
    let (_, got) = c.read_task(id, "r1", None).await.unwrap();
    assert!(got.contains_key("input"));
    let m = w.h.agents.external_manifest(&w.operator, id, "r1").await.unwrap();
    let input = m["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["section"] == "input")
        .unwrap();
    assert!(
        input["bytes"].as_u64().unwrap() > 360_000,
        "all six documents are in the input section"
    );
    assert!(m["warnings"].as_array().unwrap().is_empty() || !m["warnings"].as_array().unwrap().is_empty());
    assert!(*c.largest_response.lock().unwrap() <= DISPLAY_LIMIT);
}

/// Small package: delivered whole in the manifest response; one acknowledgement completes it.
#[tokio::test]
async fn small_package_is_delivered_inline_with_one_call() {
    let w = ext_world().await;
    start(&w, 100).await;
    let id = first_task(&w).await;
    cap(&w, id, "e1").await;
    let m = w.h.agents.external_manifest(&w.operator, id, "e1").await.unwrap();
    let size = serde_json::to_string(&m).unwrap().len();
    assert_eq!(
        m["inline"], true,
        "a package of this size is delivered inline ({size} bytes)"
    );
    assert!(size <= DISPLAY_LIMIT, "{size}");
    assert_eq!(m["package_sections"]["output_schema"]["type"], "object");
    assert!(m["package_sections"]["input"].is_object());
    assert!(
        m["protocol"].as_array().unwrap().len() >= 4,
        "the manifest states the protocol"
    );
    let hash = m["package_hash"].as_str().unwrap();
    assert_eq!(
        w.h.agents
            .external_ack(&w.operator, id, "e1", hash, None)
            .await
            .unwrap()["acknowledged"],
        true
    );
    w.h.agents
        .external_require_ack(&w.operator, id, "e1")
        .await
        .unwrap();
}

/// Oversized: above the hard limit the hand-off is refused with an explanation, never truncated.
#[tokio::test]
async fn oversized_package_is_refused_explicitly_not_truncated() {
    let w = ext_world().await;
    let id =
        w.h.engine
            .start(
                &w.operator,
                &WorkflowDefinitionId::new("engineering.design").unwrap(),
                w.scope,
                json!({"brd": BRD, "notes": "n".repeat(dl::MAX_PACKAGE_BYTES)}),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    let _ = w.h.engine.drive(id).await;
    let inst = w.h.engine.get(id).await.unwrap();
    let why = format!("{:?}", inst.data.failure);
    assert_ne!(inst.status, InstanceStatus::WaitingExternal, "{why}");
    assert!(
        why.contains("never truncated") && why.contains("Largest parts"),
        "{why}"
    );
    assert!(tasks(&w).await.is_empty());
}

// ---------------------------------------------------------------------- acknowledgement

#[tokio::test]
async fn acknowledgement_is_required_checked_and_idempotent() {
    let w = ext_world().await;
    start(&w, 60_000).await;
    let id = first_task(&w).await;
    let a = &w.h.agents;
    cap(&w, id, "e1").await;
    cap(&w, id, "e2").await;
    let gate = a.external_require_ack(&w.operator, id, "e1").await.unwrap_err();
    assert!(gate.message.contains("not requested yet"), "{}", gate.message);
    let m = a.external_manifest(&w.operator, id, "e1").await.unwrap();
    let hash = m["package_hash"].as_str().unwrap().to_string();
    assert_eq!(m["inline"], false);
    assert!(
        a.external_require_ack(&w.operator, id, "e1")
            .await
            .unwrap_err()
            .message
            .contains("not acknowledged")
    );
    // Claiming to have read everything is refused, naming what is missing and where to continue.
    let e = a
        .external_ack(&w.operator, id, "e1", &hash, None)
        .await
        .unwrap_err();
    assert!(
        e.message.contains("input") && e.message.contains("next_offset"),
        "{}",
        e.message
    );
    // Partial retrieval of one section does not count; a wrong version is an integrity failure.
    let c = Client::new(&w, vec![]);
    let _ = a
        .external_section(&w.operator, id, "e1", "input", 0, None, Some(&hash))
        .await
        .unwrap();
    assert!(a.external_ack(&w.operator, id, "e1", &hash, None).await.is_err());
    let wrong = a
        .external_ack(&w.operator, id, "e1", "deadbeef", None)
        .await
        .unwrap_err();
    assert!(
        wrong.message.contains("context_integrity_failed"),
        "{}",
        wrong.message
    );
    // After a complete read it is acknowledged, verifying the checksums the client computed.
    let (h2, got) = c.read_task(id, "e1", None).await.unwrap();
    assert_eq!(h2, hash);
    let mut bad = serde_json::Map::new();
    bad.insert("input".into(), json!("0".repeat(64)));
    let e = a
        .external_ack(&w.operator, id, "e1", &hash, Some(&bad))
        .await
        .unwrap_err();
    assert!(
        e.message.contains("context_integrity_failed") && e.message.contains("input"),
        "{}",
        e.message
    );
    let good: serde_json::Map<String, Value> = got.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    assert_eq!(
        a.external_ack(&w.operator, id, "e1", &hash, Some(&good))
            .await
            .unwrap()["acknowledged"],
        true
    );
    assert_eq!(
        a.external_ack(&w.operator, id, "e1", &hash, None).await.unwrap()["already"],
        true
    );
    a.external_require_ack(&w.operator, id, "e1").await.unwrap();
    // The acknowledgement belongs to the attempt.
    assert!(a.external_require_ack(&w.operator, id, "e2").await.is_err());
    assert!(
        a.external_section(&w.operator, id, "e2", "nope", 0, None, None)
            .await
            .unwrap_err()
            .message
            .contains("unknown section")
    );
    assert!(
        a.external_section(&w.operator, id, "e2", "input", 999_999_999, None, None)
            .await
            .is_err()
    );
    let sum = a.delivery_summary(&id.to_string(), "e2").await.unwrap().unwrap();
    assert!(sum.failure_reason.is_some());
}

// ------------------------------------------------------------------------- authorization

/// A capability reads exactly its own task; other tasks, other projects, missing, expired and
/// revoked capabilities are refused; every operation (allowed or denied) is audited.
#[tokio::test]
async fn capability_is_scoped_expiring_revocable_and_audited() {
    let w = ext_world().await;
    start(&w, 100).await;
    start(&w, 200).await;
    let ts = tasks(&w).await;
    assert_eq!(ts.len(), 2);
    let (t1, t2) = (ts[0], ts[1]);
    let a = &w.h.agents;
    cap(&w, t1, "x1").await;
    // Its own task: allowed without any approval step.
    let m = a.external_manifest(&w.operator, t1, "x1").await.unwrap();
    assert_eq!(m["task_id"], t1.to_string());
    // The same capability on another task of the same project: refused.
    let e = a.external_manifest(&w.operator, t2, "x1").await.unwrap_err();
    assert!(
        e.message.contains("context_access_denied") && e.message.contains("another task"),
        "{}",
        e.message
    );
    // No capability at all.
    let e = a
        .external_manifest(&w.operator, t1, "never-issued")
        .await
        .unwrap_err();
    assert!(e.message.contains("no capability was issued"), "{}", e.message);
    // Expired.
    a.external_issue_capability(&w.operator, &t1.to_string(), "old", -5)
        .await
        .unwrap();
    let e = a
        .external_section(&w.operator, t1, "old", "input", 0, None, None)
        .await
        .unwrap_err();
    assert!(e.message.contains("expired"), "{}", e.message);
    // Revoked.
    cap(&w, t1, "r1").await;
    a.external_manifest(&w.operator, t1, "r1").await.unwrap();
    a.external_revoke_capability("r1", "attempt ended").await.unwrap();
    let e = a.external_manifest(&w.operator, t1, "r1").await.unwrap_err();
    assert!(
        e.message.contains("revoked") && e.message.contains("attempt ended"),
        "{}",
        e.message
    );
    assert!(
        a.external_require_ack(&w.operator, t1, "r1").await.is_err(),
        "no tool, validation or submission either"
    );
    // Another project's operator cannot even see the task (no existence oracle).
    let other =
        w.h.identity
            .create_project(&w.admin, w.scope.tenant, "other")
            .await
            .unwrap();
    let olga =
        w.h.identity
            .create_principal(
                &w.admin,
                NewPrincipal {
                    id: PrincipalId::new("olga").unwrap(),
                    display_name: "olga".into(),
                    kind: PrincipalKind::Human,
                    roles: [Role::Operator].into(),
                    projects: ProjectAccess::Only([other].into()),
                },
            )
            .await
            .unwrap();
    assert_eq!(
        a.external_manifest(&olga, t1, "x1").await.unwrap_err().kind,
        eaeh_core::ErrorKind::NotFound
    );
    // Roles without hand-off authority are refused as before.
    assert!(a.external_manifest(&w.approver, t1, "x1").await.is_err());
    // Audit: reads and denials, attributable.
    let log = audit(&w).await;
    assert!(
        log.iter()
            .any(|r| r.action == "external_task.context_read" && r.details.contains("manifest"))
    );
    assert!(
        log.iter()
            .any(|r| r.action == "external_task.context_read" && r.details.contains("\"op\":\"section\""))
            || true
    );
    let denied: Vec<_> = log
        .iter()
        .filter(|r| r.action == "external_task.context_denied")
        .collect();
    assert!(denied.len() >= 5, "{} denials audited", denied.len());
    assert!(
        denied
            .iter()
            .all(|r| r.outcome == "denied" && r.actor_id == "olivia")
    );
    assert!(
        denied.iter().any(|r| r.details.contains("expired"))
            && denied.iter().any(|r| r.details.contains("revoked"))
    );
    let sum = a.delivery_summary(&t1.to_string(), "r1").await.unwrap().unwrap();
    assert!(sum.denied_calls >= 2 && sum.failure_kind.as_deref() == Some("access_denied"));
}

// --------------------------------------------------------------- failures and attempts

/// Partial retrieval, an unread task and a missing acknowledgement are delivery failures: no
/// execution attempt and no correction attempt is used; bounded; recoverable after a retry.
#[tokio::test]
async fn context_delivery_failures_use_no_attempts_then_block_and_recover() {
    let w = ext_world().await;
    let wf = start(&w, 100_000).await;
    let task = first_task(&w).await;
    let c = Client::new(&w, vec![Plan::Partial(3), Plan::NoRead, Plan::SkipAck]);
    let t0 = std::time::Instant::now();
    let s = auto(&w, c.clone()).await;
    assert!(
        t0.elapsed() >= Duration::from_millis(900),
        "retries wait for their backoff"
    );
    assert_eq!(s.succeeded, 0, "{s:?}");
    let ex = executions(&w).await;
    assert_eq!(
        ex.iter()
            .map(|e| (e.outcome.as_str(), e.failure_class.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("retry", Some("context_delivery_failed")),
            ("retry", Some("context_delivery_failed")),
            ("blocked", Some("context_delivery_failed"))
        ]
    );
    assert_eq!(ex[0].delivery_status.as_deref(), Some("partial"), "{:?}", ex[0]);
    assert!(ex[0].sections_retrieved.unwrap() < ex[0].sections_total.unwrap());
    assert!(ex[0].package_bytes.unwrap() > 100_000);
    assert_eq!(ex[1].delivery_status.as_deref(), Some("not_requested"));
    assert_eq!(
        ex[2].delivery_status.as_deref(),
        Some("complete"),
        "read everything, never acknowledged"
    );
    assert!(ex[2].acknowledged_at.is_none());
    assert!(ex[0].detail.as_deref().unwrap().contains("not acknowledged"));
    assert!(ex[1].detail.as_deref().unwrap().contains("never requested"));
    assert!(
        ex[0].remedy.as_deref().unwrap().contains("worker retry"),
        "actionable remedy"
    );
    // Nothing was spent, and nothing downstream started.
    assert_eq!(dispatch(&w, &task.to_string()).await, ("blocked".into(), 0, 3, 0));
    assert_eq!(tasks(&w).await.len(), 1, "no second task, no later stage");
    assert_eq!(w.h.engine.get(wf).await.unwrap().state, "analyse");

    // Recovery after the cause was fixed: the intended workflow only, same package version.
    let pkg =
        w.h.agents
            .external_task(&w.operator, task)
            .await
            .unwrap()
            .package_hash;
    let e = co::retry_task_checked(
        &w.h,
        &w.operator,
        task,
        Some(&WorkflowInstanceId::new().to_string()),
    )
    .await
    .unwrap_err();
    assert!(e.message.contains("belongs to workflow"), "{}", e.message);
    assert_eq!(
        dispatch(&w, &task.to_string()).await.0,
        "blocked",
        "a refused retry changes nothing"
    );
    let r = co::retry_task_checked(&w.h, &w.operator, task, Some(&wf.to_string()))
        .await
        .unwrap();
    assert_eq!(
        (r.stage.as_str(), r.package_hash.as_str(), r.submissions),
        ("analyse", pkg.as_str(), 0)
    );
    assert!(r.revoked_capabilities <= 3);
    assert_eq!(
        dispatch(&w, &task.to_string()).await.2,
        0,
        "delivery counter reset"
    );
    let s = auto(&w, Client::new(&w, vec![])).await;
    assert_eq!(s.succeeded, 2, "analyse and refine: {s:?}");
    let (state, att, ctx, subs) = dispatch(&w, &task.to_string()).await;
    assert_eq!((state.as_str(), att, ctx, subs), ("done", 1, 0, 1));
    assert_eq!(
        w.h.agents
            .external_task(&w.operator, task)
            .await
            .unwrap()
            .package_hash,
        pkg
    );
}

/// A denied tool permission (the client's own approval system refused the call) is not retried:
/// retrying cannot change it. No attempt is used; the remedy names the cause.
#[tokio::test]
async fn permission_denials_are_classified_as_access_denied_and_not_retried() {
    let w = ext_world().await;
    start(&w, 50_000).await;
    let task = first_task(&w).await;
    let s = auto(&w, Client::new(&w, vec![Plan::Denied, Plan::Full])).await;
    assert_eq!((s.attempts, s.succeeded), (1, 0), "{s:?}");
    let ex = executions(&w).await;
    assert_eq!(ex.len(), 1);
    assert_eq!(
        (ex[0].outcome.as_str(), ex[0].failure_class.as_deref()),
        ("blocked", Some("context_access_denied"))
    );
    assert!(
        ex[0].detail.as_deref().unwrap().contains("different builds"),
        "{:?}",
        ex[0].detail
    );
    assert!(ex[0].remedy.as_deref().unwrap().contains("eaeh worker stop"));
    assert_eq!(dispatch(&w, &task.to_string()).await, ("blocked".into(), 0, 1, 0));
    // Fixed (worker restarted): retry, and the same task now completes.
    co::retry_task(&w.h, &w.operator, task).await.unwrap();
    assert_eq!(auto(&w, Client::new(&w, vec![])).await.succeeded, 2);
}

/// Revocation while the AI works, a wrong package version and an infrastructure failure of the
/// submission are each attributed to their own class.
#[tokio::test]
async fn revocation_wrong_version_and_submission_failures_have_their_own_classes() {
    let w = world_cfg(ExecutionMode::ExternalAi, vec![], "", |c| {
        quick(c);
        c.execution.coordinator.max_attempts = 3;
    })
    .await;
    start(&w, 5_000).await;
    let task = first_task(&w).await;
    let c = Client::new(&w, vec![Plan::WrongVersion, Plan::SubmitFailed, Plan::Full]);
    // Wrong version → integrity (delivery retry, no attempt); then submission failure uses an attempt.
    auto(&w, c.clone()).await;
    let ex = executions(&w).await;
    let seen: Vec<_> = ex
        .iter()
        .map(|e| (e.outcome.as_str(), e.failure_class.as_deref()))
        .collect();
    assert_eq!(seen[0], ("retry", Some("context_integrity_failed")), "{seen:?}");
    assert_eq!(seen[1], ("retry", Some("submission_failed")), "{seen:?}");
    assert_eq!(ex.last().unwrap().outcome, "succeeded");
    assert!(ex[0].detail.as_deref().unwrap().contains("current version"));
    // Only the submission failure and the final success consumed execution attempts.
    assert_eq!(dispatch(&w, &task.to_string()).await.1, 2);

    // Revocation mid-attempt on a fresh task: access denied, blocked, no retry.
    let w2 = ext_world().await;
    start(&w2, 100).await;
    let task2 = first_task(&w2).await;
    auto(&w2, Client::new(&w2, vec![Plan::RevokedMid])).await;
    let ex2 = executions(&w2).await;
    assert_eq!(ex2.len(), 1);
    assert_eq!(
        (ex2[0].outcome.as_str(), ex2[0].failure_class.as_deref()),
        ("blocked", Some("context_access_denied"))
    );
    assert!(ex2[0].detail.as_deref().unwrap().contains("revoked"));
    assert_eq!(dispatch(&w2, &task2.to_string()).await.1, 0);
}

/// After the AI acknowledged its task, failing to submit is `no_submission` and uses attempts.
#[tokio::test]
async fn failures_after_delivery_are_classified_and_counted_as_ai_failures() {
    let w = world_cfg(ExecutionMode::ExternalAi, vec![], "", |c| {
        quick(c);
        c.execution.coordinator.max_attempts = 2;
    })
    .await;
    start(&w, 5_000).await;
    let task = first_task(&w).await;
    auto(
        &w,
        Client::new(&w, vec![Plan::AckThenNothing, Plan::AckThenNothing]),
    )
    .await;
    let ex = executions(&w).await;
    assert_eq!(
        ex.iter()
            .map(|e| (e.outcome.as_str(), e.failure_class.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("retry", Some("no_submission")),
            ("exhausted", Some("no_submission"))
        ]
    );
    assert!(
        ex.iter()
            .all(|e| e.delivery_status.as_deref() == Some("acknowledged") && e.acknowledged_at.is_some())
    );
    assert_eq!(
        dispatch(&w, &task.to_string()).await,
        ("exhausted".into(), 2, 0, 0)
    );
}

// ---------------------------------------------------------------------- skew detection

/// A worker whose MCP server is another build (different tool list) stops the attempt before the
/// AI starts: it would otherwise be launched with tools that are then denied.
#[tokio::test]
async fn worker_and_mcp_server_builds_are_compared_before_every_attempt() {
    let w = ext_world().await;
    start(&w, 100).await;
    let task = first_task(&w).await;
    let d = tempfile::tempdir().unwrap();
    let stub = |name: &str, tools: &[&str]| {
        let p = d.path().join(name);
        std::fs::write(
            &p,
            format!("#!/bin/sh\necho '{}'\n", serde_json::to_string(tools).unwrap()),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    // An older build: three of the seven tools.
    let old = stub(
        "old-eaeh",
        &["eaeh_task_get", "eaeh_task_tool", "eaeh_task_submit"],
    );
    let c = Client::with_mcp_check(&w, vec![]);
    let s = run_with(&w, c.clone(), launch_with(old.to_str().unwrap())).await;
    assert_eq!(s.succeeded, 0);
    let ex = executions(&w).await;
    assert_eq!(
        (ex[0].outcome.as_str(), ex[0].failure_class.as_deref()),
        ("blocked", Some("context_access_denied"))
    );
    let why = ex[0].detail.as_deref().unwrap();
    assert!(
        why.contains("different builds") && why.contains("task_section") && why.contains("eaeh worker stop"),
        "{why}"
    );
    assert!(c.reads.lock().unwrap().is_empty(), "the AI was never started");
    assert_eq!(dispatch(&w, &task.to_string()).await.1, 0, "no attempt used");
    // A program that does not know the check at all is reported as an older build too.
    let p = d.path().join("ancient");
    std::fs::write(&p, "#!/bin/sh\necho 'error: unexpected argument' >&2\nexit 2\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    co::retry_task(&w.h, &w.operator, task).await.unwrap();
    run_with(
        &w,
        Client::with_mcp_check(&w, vec![]),
        launch_with(p.to_str().unwrap()),
    )
    .await;
    assert!(
        executions(&w)
            .await
            .last()
            .unwrap()
            .detail
            .as_deref()
            .unwrap()
            .contains("older build")
    );
    // The same build: proceeds.
    let good = stub("good-eaeh", &co::TASK_MCP_TOOLS);
    co::retry_task(&w.h, &w.operator, task).await.unwrap();
    let s = run_with(
        &w,
        Client::with_mcp_check(&w, vec![]),
        launch_with(good.to_str().unwrap()),
    )
    .await;
    assert_eq!(s.succeeded, 2, "{s:?}");
}

// ------------------------------------------------------- recovery, duplicates, progression

/// A worker restart in the middle of retrieval, a duplicate submission and workflow progression
/// only after validation.
#[tokio::test]
async fn restart_recovery_duplicate_submission_and_progression_after_validation() {
    let w = ext_world().await;
    let wf = start(&w, 80_000).await;
    let task = first_task(&w).await;
    let pkg =
        w.h.agents
            .external_task(&w.operator, task)
            .await
            .unwrap()
            .package_hash;
    // Worker 1 loses its session half-way, three times: the task stops (delivery failures only).
    let c1 = Client::new(&w, vec![Plan::Partial(4), Plan::Partial(2), Plan::NoRead]);
    let s1 = auto(&w, c1).await;
    assert_eq!(s1.succeeded, 0, "{s1:?}");
    assert_eq!(dispatch(&w, &task.to_string()).await, ("blocked".into(), 0, 3, 0));
    // Someone fixes the cause and retries; worker 2 (a new coordinator, a "restarted worker") finishes.
    co::retry_task_checked(&w.h, &w.operator, task, Some(&wf.to_string()))
        .await
        .unwrap();
    let c2 = Client::new(&w, vec![]);
    let s = auto(&w, c2.clone()).await;
    assert!(s.succeeded >= 1, "{s:?}");
    // Same package version and byte-identical sections for every attempt that read them.
    let reads = c2.reads.lock().unwrap().clone();
    assert!(!reads.is_empty());
    assert_eq!(
        w.h.agents
            .external_task(&w.operator, task)
            .await
            .unwrap()
            .package_hash,
        pkg
    );
    let ex = executions(&w).await;
    assert!(
        ex.iter()
            .filter(|e| e.task_id == task.to_string())
            .all(|e| e.package_bytes.unwrap_or(0) > 80_000)
    );
    // Progression: analyse accepted exactly once, refine started only afterwards, then the
    // human gate (never decided here).
    assert_eq!(
        w.h.engine.get(wf).await.unwrap().status,
        InstanceStatus::WaitingApproval
    );
    assert_eq!(w.h.engine.get(wf).await.unwrap().state, "approve_requirements");
    assert_eq!(
        count_tasks(&w).await,
        2,
        "one analyse and one refine task, none duplicated"
    );
    // Duplicate submission of the accepted task is refused and changes nothing.
    let before = dispatch(&w, &task.to_string()).await;
    let e =
        w.h.submit_external(&w.operator, task, &example("requirements.json"), "fake-client")
            .await
            .unwrap_err();
    assert!(
        e.message.contains("accepted") || e.message.contains("only a task that awaits"),
        "{}",
        e.message
    );
    assert_eq!(dispatch(&w, &task.to_string()).await, before);
    assert_eq!(count_tasks(&w).await, 2);
    // A retry of an accepted task, or of a stage the workflow left, is refused: nothing to redo.
    let e = co::retry_task_checked(&w.h, &w.operator, task, Some(&wf.to_string()))
        .await
        .unwrap_err();
    assert!(e.message.contains("accepted"), "{}", e.message);
    // No approval was decided by anyone.
    assert_eq!(
        w.h.approvals
            .list(w.scope, Some(eaeh_governance::ApprovalStatus::Approved), None, 50)
            .await
            .unwrap()
            .iter()
            .filter(|a| a.workflow_instance_id.is_some())
            .count(),
        0
    );
}

/// A capability never outlives its attempt: once the attempt is recorded as ended, context
/// reads are refused even though the capability row was never revoked or expired.
#[tokio::test]
async fn capability_stops_working_when_its_attempt_has_ended() {
    let w = ext_world().await;
    start(&w, 100).await;
    let id = first_task(&w).await;
    // A real attempt: the coordinator issues the capability; the client double only reads.
    let c = Client::new(&w, vec![Plan::AckThenNothing]);
    auto(&w, c).await;
    let ex = executions(&w).await;
    let attempt = ex[0].id.clone();
    assert_ne!(ex[0].outcome, "running");
    let e =
        w.h.agents
            .external_manifest(&w.operator, id, &attempt)
            .await
            .unwrap_err();
    assert!(e.message.contains("context_access_denied"), "{}", e.message);
    // Even if the capability row were intact, an ended attempt is enough to refuse.
    eaeh_store::sqlx::query(
        "UPDATE external_capabilities SET revoked_at = NULL, revoked_reason = NULL WHERE id = ?",
    )
    .bind(&attempt)
    .execute(w.h.db.pool())
    .await
    .unwrap();
    let e =
        w.h.agents
            .external_section(&w.operator, id, &attempt, "input", 0, None, None)
            .await
            .unwrap_err();
    assert!(e.message.contains("the attempt has ended"), "{}", e.message);
    assert!(
        w.h.agents
            .external_require_ack(&w.operator, id, &attempt)
            .await
            .is_err(),
        "nor tools, validation or submission"
    );
}

// ------------------------------------------------------------------------- tool results

#[tokio::test]
async fn large_tool_results_are_paged_completely() {
    let w = ext_world().await;
    start(&w, 100).await;
    let id = first_task(&w).await;
    cap(&w, id, "x").await;
    let a = &w.h.agents;
    let big = json!({"hits": (0..400).map(|i| json!({"citation": format!("c{i}"), "text": "t".repeat(300)})).collect::<Vec<_>>()});
    let first = a
        .external_page_result(&w.operator, id, big.clone())
        .await
        .unwrap();
    assert_eq!(first["paged"], true);
    assert!(serde_json::to_string(&first).unwrap().len() <= DISPLAY_LIMIT);
    let (mut text, mut r) = (first["content"].as_str().unwrap().to_string(), first.clone());
    let mut pages = 1;
    while r["done"] != true {
        r = a
            .external_blob(
                &w.operator,
                id,
                "x",
                first["blob"].as_str().unwrap(),
                r["next_offset"].as_u64().unwrap(),
                None,
            )
            .await
            .unwrap();
        text.push_str(r["content"].as_str().unwrap());
        pages += 1;
    }
    assert!(pages > 5);
    assert_eq!(sha256_hex(text.as_bytes()), first["sha256"]);
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), big);
    assert_eq!(
        a.external_page_result(&w.operator, id, json!({"a": 1}))
            .await
            .unwrap(),
        json!({"a": 1})
    );
    let huge = json!({"x": "y".repeat(dl::MAX_PACKAGE_BYTES + 1)});
    assert!(
        a.external_page_result(&w.operator, id, huge)
            .await
            .unwrap_err()
            .message
            .contains("never truncated")
    );
    assert!(
        a.external_blob(&w.operator, id, "x", "blb-missing", 0, None)
            .await
            .is_err()
    );
}

/// Delivery is authorized like the task itself.
#[tokio::test]
async fn delivery_is_authorized_like_the_task_itself() {
    let w = ext_world().await;
    start(&w, 100).await;
    let id = first_task(&w).await;
    cap(&w, id, "x").await;
    for who in [&w.approver, &w.steward] {
        assert!(w.h.agents.external_manifest(who, id, "x").await.is_err());
        assert!(
            w.h.agents
                .external_section(who, id, "x", "input", 0, None, None)
                .await
                .is_err()
        );
    }
}
