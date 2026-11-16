//! Governance Portal acceptance tests (ADR-0026, `Approval-Portal.md` §11).
//!
//! Real data: the shipped `engineering.design` workflow on the shipped configuration runs
//! to its requirements approval gate (the model is the labelled test double
//! `ScriptedModelProvider`, as in `design_demo.rs`). Every portal request goes through the
//! real portal application (`eaeh_api::portal::app`): local guard, identity injection, the
//! unchanged `/v1` router, the approval manager and the workflow engine.

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::*;
use eaeh_api::portal::{Acting, COOKIE, PortalState, app};
use eaeh_core::{Principal, TraceContext, WorkflowDefinitionId, WorkflowInstanceId};
use eaeh_testkit::Script;
use eaeh_workflow::InstanceStatus;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const HOST: &str = "127.0.0.1:8090";
const ORIGIN: &str = "http://127.0.0.1:8090";

struct Portal {
    app: axum::Router,
    tokens: Vec<String>,
}

async fn token(w: &World, p: &Principal) -> String {
    w.h.identity
        .issue_token(&w.admin, &p.id, "portal-test", 1)
        .await
        .unwrap()
        .token
        .expose()
        .to_string()
}

async fn portal(
    w: &World,
    viewer: &Principal,
    approver: Option<&Principal>,
    static_dir: Option<std::path::PathBuf>,
) -> Portal {
    let vt = token(w, viewer).await;
    let mut tokens = vec![vt.clone()];
    let approver = match approver {
        Some(a) => {
            let t = token(w, a).await;
            tokens.push(t.clone());
            Some(Acting::new("approver", t, a.clone()))
        }
        None => None,
    };
    let state = PortalState::new(
        Acting::new("viewer", vt, viewer.clone()),
        approver,
        SECRET.into(),
        HOST.parse().unwrap(),
        vec![],
        static_dir,
        w.h.execution_mode.to_string(),
    );
    Portal {
        app: app(w.h.clone(), state),
        tokens,
    }
}

async fn portal_with_steward(
    w: &World,
    viewer: &Principal,
    approver: Option<&Principal>,
    steward: Option<&Principal>,
) -> Portal {
    let mut p = portal(w, viewer, approver, None).await;
    let st = match steward {
        Some(s) => {
            let t = token(w, s).await;
            p.tokens.push(t.clone());
            Some(Acting::new("steward", t, s.clone()))
        }
        None => None,
    };
    let state = PortalState::new(
        Acting::new("viewer", p.tokens[0].clone(), viewer.clone()),
        match approver {
            Some(a) => Some(Acting::new("approver", p.tokens[1].clone(), a.clone())),
            None => None,
        },
        SECRET.into(),
        HOST.parse().unwrap(),
        vec![],
        None,
        w.h.execution_mode.to_string(),
    )
    .with_steward(st);
    p.app = app(w.h.clone(), state);
    p
}

impl Portal {
    async fn send(&self, req: Request<Body>) -> (StatusCode, Value, String) {
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).to_string();
        for t in &self.tokens {
            assert!(
                !text.contains(t.as_str()),
                "a stored token leaked into a portal response"
            );
        }
        let v = serde_json::from_slice(&bytes).unwrap_or(Value::String(text));
        (status, v, cookie)
    }
    async fn get(&self, path: &str) -> (StatusCode, Value) {
        let (s, v, _) = self
            .send(
                Request::get(path)
                    .header(header::HOST, HOST)
                    .header(header::COOKIE, format!("{COOKIE}={SECRET}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        (s, v)
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        let (s, v, _) = self
            .send(
                Request::post(path)
                    .header(header::HOST, HOST)
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, format!("{COOKIE}={SECRET}"))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;
        (s, v)
    }
}

/// `engineering.design` at its requirements gate, plus HLD/LLD outputs for later stages.
async fn design_at_requirements_gate() -> (World, WorkflowInstanceId) {
    let good = example("requirements.json");
    let read_brd = || Script::tool("knowledge.read", json!({"document": BRD}));
    let script = vec![
        read_brd(),
        Script::json(good.clone()),
        read_brd(),
        Script::json(good.clone()),
        Script::json(example("hld.json")),
        Script::json(example("lld.json")),
    ];
    let w = world(script, "").await;
    let id =
        w.h.engine
            .start(
                &w.operator,
                &WorkflowDefinitionId::new("engineering.design").unwrap(),
                w.scope,
                json!({"brd": BRD}),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    assert_eq!(
        w.h.engine.drive(id).await.unwrap(),
        InstanceStatus::WaitingApproval
    );
    (w, id)
}

async fn wait_for_gate(w: &World, id: WorkflowInstanceId, gate: &str) {
    for _ in 0..400 {
        let i = w.h.engine.get(id).await.unwrap();
        if i.state == gate && i.status == InstanceStatus::WaitingApproval {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!(
        "workflow did not reach {gate}: {:?}",
        w.h.engine.get(id).await.unwrap().state
    );
}

#[tokio::test]
async fn local_guard_requires_loopback_host_same_origin_and_the_launch_secret() {
    let (w, _) = design_at_requirements_gate().await;
    let p = portal(&w, &w.operator, Some(&w.approver), None).await;
    // No session cookie.
    let (s, v, _) = p
        .send(
            Request::get("/api/v1/dashboard")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("portal_session_required"))
    );
    // Foreign Host (DNS rebinding).
    let (s, v, _) = p
        .send(
            Request::get("/api/v1/dashboard")
                .header(header::HOST, "evil.example:8090")
                .header(header::COOKIE, format!("{COOKIE}={SECRET}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("host_not_allowed"))
    );
    // Cross-origin write, even with the cookie.
    let (s, v, _) = p
        .send(
            Request::post("/api/v1/approvals/x/decision")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, "http://evil.example")
                .header(header::COOKIE, format!("{COOKIE}={SECRET}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("origin_not_allowed"))
    );
    // Wrong launch secret.
    let open = |secret: &str| {
        Request::post("/portal/session")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"launch": secret}).to_string()))
            .unwrap()
    };
    let (s, _, cookie) = p.send(open("not-the-secret")).await;
    assert_eq!((s, cookie.as_str()), (StatusCode::UNAUTHORIZED, ""));
    // Right secret ⇒ HttpOnly, SameSite=Strict session cookie; no login page involved.
    let (s, _, cookie) = p.send(open(SECRET)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        cookie.starts_with(&format!("{COOKIE}={SECRET}"))
            && cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Strict"),
        "{cookie}"
    );
    let (s, v) = p.get("/portal/session").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["viewer"]["principal"], "olivia");
    assert_eq!(v["approver"]["principal"], "arthur");
    assert_eq!(v["can_decide"], true);
    // The installation shown is the harness's own data directory.
    let dd = std::fs::canonicalize(w.h.config.data_dir()).unwrap();
    assert_eq!(v["installation"]["data_dir"], dd.display().to_string());
}

#[tokio::test]
async fn portal_exposes_only_reads_and_the_decision_endpoint_and_ignores_client_credentials() {
    let (w, _) = design_at_requirements_gate().await;
    let p = portal(&w, &w.operator, Some(&w.approver), None).await;
    for (m, path) in [
        ("GET", "/api/v1/tokens"),
        ("POST", "/api/v1/tokens"),
        ("POST", "/api/v1/principals"),
        ("POST", "/api/v1/workflows"),
        ("POST", "/api/v1/knowledge/documents"),
        ("POST", "/api/v1/skills"),
        ("GET", "/api/v1/plugins"),
    ] {
        let (s, v) = if m == "GET" {
            p.get(path).await
        } else {
            p.post(path, json!({})).await
        };
        assert_eq!(
            (s, v["error"]["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("not_available_in_portal")),
            "{m} {path}"
        );
    }
    // A client-supplied bearer token is replaced by the viewer identity.
    let admin_token = token(&w, &w.admin).await;
    let (s, _, _) = p
        .send(
            Request::get("/api/v1/me")
                .header(header::HOST, HOST)
                .header(header::COOKIE, format!("{COOKIE}={SECRET}"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, me) = p.get("/api/v1/me").await;
    assert_eq!(me["id"], "olivia");
}

#[tokio::test]
async fn dashboard_shows_real_in_progress_work_pending_approvals_and_activity() {
    let (w, id) = design_at_requirements_gate().await;
    let p = portal(&w, &w.operator, None, None).await;
    let (s, d) = p.get("/api/v1/dashboard").await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["summary"]["projects"], 1);
    assert_eq!(d["summary"]["in_progress"], 1);
    assert_eq!(d["summary"]["waiting_approval"], 1);
    assert_eq!(d["summary"]["waiting_external"], 0);
    assert_eq!(d["summary"]["pending_approvals"], 1);
    assert_eq!(d["summary"]["completed"], 0);
    let active = &d["active"][0];
    assert_eq!(active["workflow"]["id"], id.to_string());
    assert_eq!(active["workflow"]["status"], "waiting_approval");
    assert_eq!(active["workflow"]["state"], "approve_requirements");
    assert_eq!(active["project_name"], "orders");
    let pa = &d["pending_approvals"][0];
    assert_eq!(pa["document_type"], "Requirements");
    assert_eq!(pa["required_role"], "approver");
    assert_eq!(pa["artifact"]["version"], 1);
    assert_eq!(pa["workflow"]["definition_id"], "engineering.design");
    let kinds: Vec<&str> = d["activity"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();
    for k in ["workflow_started", "artifact_generated", "approval_requested"] {
        assert!(kinds.contains(&k), "{k} missing from {kinds:?}");
    }

    // Workflow detail: definition, history and task executions with agents.
    let (s, wd) = p.get(&format!("/api/v1/workflows/{id}/detail")).await;
    assert_eq!(s, StatusCode::OK, "{wd}");
    assert_eq!(wd["definition"]["id"], "engineering.design");
    assert!(wd["history"].as_array().unwrap().len() >= 4);
    let tasks = wd["tasks"].as_array().unwrap();
    let analyse = tasks.iter().find(|t| t["state"] == "analyse").unwrap();
    assert_eq!(analyse["kind"], "agent");
    assert_eq!(analyse["status"], "succeeded");
    assert!(analyse["agent"].as_str().unwrap().contains('@'), "{analyse}");
    assert_eq!(wd["approvals"][0]["status"], "pending");

    // Queues across projects; project overview with artifacts.
    let (_, q) = p.get("/api/v1/workflow-queue?status=active").await;
    assert_eq!(q["items"].as_array().unwrap().len(), 1);
    let (s, q) = p.get("/api/v1/workflow-queue?status=bogus").await;
    assert_eq!(
        (s, q["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_input"))
    );
    let (_, aq) = p.get("/api/v1/approval-queue").await;
    assert_eq!(aq["items"][0]["status"], "pending");
    let (_, ov) = p
        .get(&format!("/api/v1/projects/{}/overview", w.scope.project))
        .await;
    assert_eq!(ov["status"], "waiting_approval");
    assert_eq!(ov["current_workflow"]["state"], "approve_requirements");
    let names: Vec<&str> = ov["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"design/requirements"), "{names:?}");
    // Unknown project: not found (no existence oracle).
    let (s, _) = p
        .get("/api/v1/projects/00000000-0000-7000-8000-000000000000/overview")
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn approval_document_is_readable_with_metadata_hash_and_validation_results() {
    let (w, _) = design_at_requirements_gate().await;
    let p = portal(&w, &w.operator, None, None).await;
    let (aid, hash, _) = pending_gate(&w).await;
    let (s, d) = p.get(&format!("/api/v1/approvals/{aid}/document")).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["approval"]["document_type"], "Requirements");
    assert_eq!(d["approval"]["artifact_hash"], hash);
    assert_eq!(d["document"]["artifact"]["content_hash"], hash);
    assert_eq!(d["document"]["bundle"], true);
    let files = d["document"]["files"].as_array().unwrap();
    let md = files
        .iter()
        .find(|f| f["path"] == "requirements.md")
        .expect("requirements.md in bundle");
    assert!(md["content"].as_str().unwrap().contains("BRD sections: 8"));
    assert_eq!(
        d["workflow"]["instance"]["quality"]["refine"],
        "requirements:passed (0 blocking, 4 advisory)"
    );
    // Bulky agent outputs are not sent with the workflow view.
    assert!(d["workflow"]["instance"].get("outputs").is_none());
}

#[tokio::test]
async fn read_only_portal_refuses_decisions_without_touching_the_approval() {
    let (w, _) = design_at_requirements_gate().await;
    let p = portal(&w, &w.operator, None, None).await;
    let (aid, hash, _) = pending_gate(&w).await;
    let (s, v) = p
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "approve", "comments": "", "artifact_hash": hash}),
        )
        .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("approver_not_configured"))
    );
    assert_eq!(
        w.h.approvals.get(aid).await.unwrap().status,
        eaeh_governance::ApprovalStatus::Pending
    );
}

#[tokio::test]
async fn backend_stays_authoritative_for_role_and_stale_hash() {
    let (w, _) = design_at_requirements_gate().await;
    let (aid, hash, _) = pending_gate(&w).await;
    // An identity without the approver role configured as "approver": the API refuses.
    let p = portal(&w, &w.operator, Some(&w.operator), None).await;
    let (s, v) = p
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "approve", "comments": "", "artifact_hash": hash}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    // The real approver with a stale/foreign hash: refused, approval still pending.
    let p = portal(&w, &w.operator, Some(&w.approver), None).await;
    let (s, v) = p
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "approve", "comments": "", "artifact_hash": "0".repeat(64)}),
        )
        .await;
    assert!(s.is_client_error(), "{s} {v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("hash"),
        "{v}"
    );
    assert_eq!(
        w.h.approvals.get(aid).await.unwrap().status,
        eaeh_governance::ApprovalStatus::Pending
    );
}

#[tokio::test]
async fn decisions_go_through_the_approval_service_preserve_hash_and_audit_and_resume_the_engine() {
    let (w, id) = design_at_requirements_gate().await;
    let p = portal(&w, &w.operator, Some(&w.approver), None).await;

    // Request changes: comments recorded, workflow revises and returns to the same gate.
    let (aid, hash, _) = pending_gate(&w).await;
    let (s, v) = p
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "request_changes", "comments": "Add a rationale for the latency target", "artifact_hash": hash}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "changes_requested");
    // The scripted provider has no revision response, so the revision task fails: the
    // engine (not the portal) decides what happens next.
    for _ in 0..400 {
        let i = w.h.engine.get(id).await.unwrap();
        if i.state != "approve_requirements" || i.status != InstanceStatus::WaitingApproval {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let rec = w.h.approvals.get(aid).await.unwrap();
    let dec = rec.decision.unwrap();
    assert_eq!(
        (dec.approver_id.as_str(), dec.decision.as_str()),
        ("arthur", "request_changes")
    );
    assert_eq!(dec.artifact_hash.as_deref(), Some(hash.as_str()));
    assert_eq!(dec.comments, "Add a rationale for the latency target");
    let audit: Vec<(String, String)> = eaeh_store::sqlx::query_as(
        "SELECT action, actor_id FROM audit_log WHERE resource_id = ? ORDER BY seq",
    )
    .bind(aid.to_string())
    .fetch_all(w.h.db.pool())
    .await
    .unwrap();
    assert!(
        audit
            .iter()
            .any(|(a, who)| a.starts_with("approval.") && who == "arthur"),
        "{audit:?}"
    );
}

#[tokio::test]
async fn approve_via_portal_resumes_the_workflow_to_the_next_gate() {
    let (w, id) = design_at_requirements_gate().await;
    let p = portal(&w, &w.operator, Some(&w.approver), None).await;
    let (aid, hash, _) = pending_gate(&w).await;
    let (s, v) = p
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "approve", "comments": "requirements approved", "artifact_hash": hash}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "approved");
    assert_eq!(v["decision"]["artifact_hash"], hash);
    // The existing engine resumes (HLD agent, render, next gate) – nothing in the portal drives it.
    wait_for_gate(&w, id, "approve_hld").await;
    let (_, d) = p.get("/api/v1/dashboard").await;
    assert_eq!(d["pending_approvals"][0]["document_type"], "HLD");
    let (_, act) = p.get("/api/v1/activity").await;
    assert!(
        act["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "approval_granted")
    );
    // A second decision on the decided request is refused by the approval service.
    let (s, _) = p
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "approve", "comments": "", "artifact_hash": hash}),
        )
        .await;
    assert!(s.is_client_error());
}

#[tokio::test]
async fn empty_installation_and_errors_are_reported_consistently() {
    let w = world(vec![], "").await;
    let p = portal(&w, &w.operator, None, None).await;
    let (s, d) = p.get("/api/v1/dashboard").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(d["summary"]["in_progress"], 0);
    assert_eq!(d["active"], json!([]));
    assert_eq!(d["pending_approvals"], json!([]));
    let (s, v) = p.get("/api/v1/approvals/not-an-id/document").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"]["request_id"].is_string() || v["error"]["code"].is_string());
    let (s, v) = p
        .get("/api/v1/workflows/00000000-0000-7000-8000-000000000000/detail")
        .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found"))
    );
}

#[tokio::test]
async fn static_ui_is_served_with_spa_fallback_security_headers_and_no_traversal() {
    let w = world(vec![], "").await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><title>portal</title>",
    )
    .unwrap();
    std::fs::write(dir.path().join("assets/app.js"), "console.log(1)").unwrap();
    let p = portal(&w, &w.operator, None, Some(dir.path().to_path_buf())).await;
    let get = |path: &str| {
        Request::get(path)
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap()
    };
    let res = p.app.clone().oneshot(get("/")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let h = res.headers();
    assert!(
        h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("default-src 'self'")
    );
    assert_eq!(h["referrer-policy"], "no-referrer");
    assert_eq!(h["x-frame-options"], "DENY");
    let res = p.app.clone().oneshot(get("/approvals/some-id")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("<title>portal</title>"));
    let res = p.app.clone().oneshot(get("/assets/app.js")).await.unwrap();
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    // Path traversal never leaves the UI directory: it falls back to index.html.
    for path in ["/../Cargo.toml", "/assets/../../Cargo.toml", "/%2e%2e/Cargo.toml"] {
        let res = p.app.clone().oneshot(get(path)).await.unwrap();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(
            !text.contains("[package]") && !text.contains("[workspace]"),
            "{path}: {text}"
        );
    }
}

/// Skills are registered (steward) and decided (approver) in the portal through the same
/// services as the CLI; mandatory standards stay CLI-only (ADR-0028).
#[tokio::test]
async fn portal_registers_and_decides_skills_but_not_standards() {
    let w = world(vec![], "").await;
    // Catalog: the Markdown example skill is a file, not registered yet, for the HLD/LLD layers.
    let ro = portal(&w, &w.operator, Some(&w.approver), None).await;
    let (s, cat) = ro.get("/api/v1/skills/catalog").await;
    assert_eq!(s, StatusCode::OK, "{cat}");
    let md = cat["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "architecture-diagrams")
        .unwrap()
        .clone();
    assert_eq!(md["format"], "markdown");
    assert_eq!(md["registration"], Value::Null);
    assert!(
        md["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l == "engineering.design/hld"),
        "{md}"
    );
    assert!(
        md["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l == "engineering.design/lld"),
        "{md}"
    );
    assert!(
        md["instructions"]
            .as_str()
            .unwrap()
            .starts_with("# Architecture diagrams")
    );
    let hld = cat["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "hld-design")
        .unwrap();
    assert_eq!(hld["registration"]["status"], "approved");

    // Registration needs the explicit steward identity.
    let (s, v) = ro
        .post("/api/v1/skills/sync", json!({"project_id": w.scope.project}))
        .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("steward_not_configured")),
        "{v}"
    );
    let p = portal_with_steward(&w, &w.operator, Some(&w.approver), Some(&w.steward)).await;
    let (s, v) = p
        .post("/api/v1/skills/sync", json!({"project_id": w.scope.project}))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let r = v["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["reference"] == "architecture-diagrams@=1.0.0")
        .unwrap()
        .clone();
    assert_eq!(r["action"], "registered", "{r}");
    let aid = r["approval_id"].as_str().unwrap().to_string();
    // Idempotent.
    let (_, v) = p
        .post("/api/v1/skills/sync", json!({"project_id": w.scope.project}))
        .await;
    assert!(
        v["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["action"] == "already_registered"),
        "{v}"
    );
    let (audits,): (i64,) = eaeh_store::sqlx::query_as(
        "SELECT COUNT(*) FROM audit_log WHERE action LIKE 'skill.register%' AND actor_id = ?",
    )
    .bind(w.steward.id.to_string())
    .fetch_one(w.h.db.pool())
    .await
    .unwrap();
    assert!(audits >= 1);

    // The review page shows the whole skill and its digest.
    let (s, doc) = p.get(&format!("/api/v1/approvals/{aid}/document")).await;
    assert_eq!(s, StatusCode::OK, "{doc}");
    assert_eq!(doc["approval"]["document_type"], "Skill");
    assert_eq!(doc["skill"]["reference"], "architecture-diagrams@1.0.0");
    assert!(
        doc["skill"]["instructions"]
            .as_str()
            .unwrap()
            .contains("component id")
    );
    let digest = doc["skill"]["digest"].as_str().unwrap().to_string();
    assert_eq!(doc["approval"]["artifact_hash"].as_str(), Some(digest.as_str()));

    // Stale digest refused; the registrant (steward) can never approve.
    let (s, _) = p
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "approve", "comments": "", "artifact_hash": "0".repeat(64)}),
        )
        .await;
    assert_ne!(s, StatusCode::OK);
    let sp = portal(&w, &w.operator, Some(&w.steward), None).await;
    let (s, _) = sp
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "approve", "comments": "", "artifact_hash": digest}),
        )
        .await;
    assert_ne!(s, StatusCode::OK, "a steward/registrant cannot approve");
    assert_eq!(
        w.h.approvals.get(aid.parse().unwrap()).await.unwrap().status,
        eaeh_governance::ApprovalStatus::Pending
    );
    // The approver decides it in the portal; the skill becomes usable.
    let (s, v) = p
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "approve", "comments": "diagram rules reviewed", "artifact_hash": digest}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, cat) = p.get("/api/v1/skills/catalog").await;
    let md = cat["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "architecture-diagrams")
        .unwrap()
        .clone();
    assert_eq!(md["registration"]["status"], "approved", "{md}");

    // Mandatory standards remain CLI-only.
    let std_doc =
        w.h.knowledge
            .ingest(
                &w.steward,
                eaeh_knowledge::IngestRequest {
                    source: "demo-api-standards".into(),
                    subject: "naming".into(),
                    title: "Naming".into(),
                    format: eaeh_knowledge::Format::Markdown,
                    content: "# Naming\n\nUse kebab-case ids.\n".into(),
                    scope: eaeh_knowledge::KnowledgeScope::Enterprise,
                    classification: eaeh_core::Classification::Internal,
                    tags: vec![],
                    mandatory: true,
                    review_by: None,
                    governance_project: w.scope.project,
                },
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    let sid = std_doc.approval_id.clone().unwrap();
    let (s, v) = p
        .post(
            &format!("/api/v1/approvals/{sid}/decision"),
            json!({"decision": "approve", "comments": "", "artifact_hash": std_doc.content_hash}),
        )
        .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("not_available_in_portal")),
        "{v}"
    );
}

/// Completion guard (engine): a definition flaw that lets a rejected gate lead to the success
/// terminal must not produce a "succeeded" workflow.
#[tokio::test]
async fn completion_guard_refuses_success_without_a_valid_gate_approval() {
    let w = world(vec![], "").await;
    let mut def: eaeh_workflow::WorkflowDefinition =
        serde_json::from_str(&std::fs::read_to_string(root().join("workflows/demo.approval.json")).unwrap())
            .unwrap();
    def.id = WorkflowDefinitionId::new("test.flawed-gate").unwrap();
    // Flaw: "rejected" bypasses publish and goes straight to the success terminal.
    let review = def.states.iter_mut().find(|s| s.name == "review").unwrap();
    review
        .transitions
        .insert(eaeh_workflow::Trigger::Rejected, "done".into());
    w.h.engine.register_definition(None, &def).await.unwrap();
    let id =
        w.h.engine
            .start(&w.operator, &def.id, w.scope, json!({}), TraceContext::new_root())
            .await
            .unwrap();
    assert_eq!(
        w.h.engine.drive(id).await.unwrap(),
        InstanceStatus::WaitingApproval
    );
    let (st, _) = decide(&w, eaeh_governance::ApprovalDecision::Reject, "not acceptable").await;
    assert_eq!(st, InstanceStatus::Failed, "the guard must refuse success");
    let inst = w.h.engine.get(id).await.unwrap();
    assert!(
        inst.data
            .failure
            .as_deref()
            .unwrap_or_default()
            .contains("completion guard: gate 'review' has no granted approval"),
        "{:?}",
        inst.data.failure
    );
    let (outcome,): (String,) = eaeh_store::sqlx::query_as(
        "SELECT outcome FROM audit_log WHERE action = 'workflow.completion_check' AND resource_id = ?",
    )
    .bind(id.to_string())
    .fetch_one(w.h.db.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "denied");
}

/// The guard passes legitimate runs and records the evidence.
#[tokio::test]
async fn completion_guard_records_success_for_a_properly_approved_run() {
    let w = world(vec![], "").await;
    let id =
        w.h.engine
            .start(
                &w.operator,
                &WorkflowDefinitionId::new("demo.approval").unwrap(),
                w.scope,
                json!({}),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
    assert_eq!(
        w.h.engine.drive(id).await.unwrap(),
        InstanceStatus::WaitingApproval
    );
    let (st, _) = decide(&w, eaeh_governance::ApprovalDecision::Approve, "ok").await;
    assert_eq!(st, InstanceStatus::Succeeded);
    let (outcome,): (String,) = eaeh_store::sqlx::query_as(
        "SELECT outcome FROM audit_log WHERE action = 'workflow.completion_check' AND resource_id = ?",
    )
    .bind(id.to_string())
    .fetch_one(w.h.db.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "succeeded");
}

/// Full design run through the portal: each gate shows its verified human approval, the
/// completion check is recorded, and the generation source of each artifact is visible.
#[tokio::test]
async fn completed_design_run_is_verified_gate_by_gate_with_generation_provenance() {
    let (w, id) = design_at_requirements_gate().await;
    let p = portal(&w, &w.operator, Some(&w.approver), None).await;
    let (_, d) = p.get(&format!("/api/v1/workflows/{id}/detail")).await;
    let g = &d["governance"];
    assert_eq!(g["gates_total"], 3);
    assert_eq!(g["gates_granted"], 0);
    assert_eq!(g["completion_verified"], Value::Null, "not terminal yet");
    for next in ["approve_hld", "approve_lld", ""] {
        let (aid, hash, _) = pending_gate(&w).await;
        let (s, v) = p
            .post(
                &format!("/api/v1/approvals/{aid}/decision"),
                json!({"decision": "approve", "comments": "ok", "artifact_hash": hash}),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if next.is_empty() {
            for _ in 0..400 {
                if w.h.engine.get(id).await.unwrap().status.is_terminal() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        } else {
            wait_for_gate(&w, id, next).await;
        }
    }
    assert_eq!(
        w.h.engine.get(id).await.unwrap().status,
        InstanceStatus::Succeeded
    );
    let (_, d) = p.get(&format!("/api/v1/workflows/{id}/detail")).await;
    let g = &d["governance"];
    assert_eq!(g["gates_granted"], 3, "{g}");
    assert_eq!(g["gates_with_issues"], 0, "{g}");
    assert_eq!(g["completion_check"]["outcome"], "succeeded");
    assert_eq!(g["completion_verified"], true);
    for gate in g["gates"].as_array().unwrap() {
        assert_eq!(gate["approver_id"], "arthur", "{gate}");
        assert_eq!(gate["approved_hash"], gate["current_hash"], "{gate}");
    }
    // Generation provenance: agent stages came from a model (here the labelled scripted
    // test double, registered as a direct provider); render stages are platform steps.
    let prov = &d["artifact_provenance"];
    assert_eq!(prov["hld"]["source"], "direct_provider", "{prov}");
    assert_eq!(
        prov["hld"]["agent_definition"].as_str().map(|s| s.contains('@')),
        Some(true)
    );
    assert_eq!(prov["render_hld"]["source"], "platform", "{prov}");
    // Approved documents in the project workspace, verified by re-hashing the files.
    assert_eq!(d["workspace"]["root"], json!(w.workspace.to_string_lossy()));
    let docs = d["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 3);
    for doc in docs {
        let m = &doc["materialization"];
        assert_eq!(m["state"], "verified", "{doc}");
        assert!(
            m["files"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["state"] == "verified")
        );
        assert!(m["last_success_at"].is_string() && m["checked_at"].is_string());
        assert!(
            doc["target_dir"]
                .as_str()
                .unwrap()
                .starts_with("docs/architecture/")
        );
    }
    // Remove one file: the portal must not claim it is synchronized.
    std::fs::remove_file(w.workspace.join("docs/architecture/lld/lld.md")).unwrap();
    let (_, d) = p.get(&format!("/api/v1/workflows/{id}/detail")).await;
    let lld = d["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["gate"] == "approve_lld")
        .unwrap()
        .clone();
    assert_eq!(lld["materialization"]["state"], "missing", "{lld}");
    // The review page of the LLD approval carries the same verified state.
    let aid = w.h.engine.get(id).await.unwrap().data.approved_gates["approve_lld"];
    let (_, doc) = p.get(&format!("/api/v1/approvals/{aid}/document")).await;
    assert_eq!(
        doc["workflow"]["workspace_document"]["materialization"]["state"],
        "missing"
    );
    assert_eq!(
        doc["workflow"]["workspace_document"]["target_dir"],
        "docs/architecture/lld"
    );
}

/// Skill onboarding in the portal: "Scan now" discovers a copied skill directory, the viewer
/// returns the full nested tree with contents and hashes, a reviewer rejects (with comments)
/// or approves through the existing approval service, and a changed file shows up as a new
/// version with per-file change markers.
#[tokio::test]
async fn portal_skill_onboarding_discovers_shows_files_and_decides_versions() {
    let skills = tempfile::tempdir().unwrap();
    let sd = skills.path().to_path_buf();
    let dd = sd.clone();
    let w = world_cfg(
        eaeh_runtime::ExecutionMode::DirectProvider,
        vec![],
        "",
        move |c| c.definitions.skills_dir = dd,
    )
    .await;
    let dir = sd.join("api-error-handling");
    copy(&root().join("config/skills/api-error-handling"), &dir);
    let p = portal(&w, &w.operator, Some(&w.approver), None).await;

    // Visible on disk before registration; "Scan now" registers it as pending (viewer token).
    let (_, cat) = p.get("/api/v1/skills/catalog").await;
    let d = cat["directories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["dir"] == "api-error-handling")
        .unwrap()
        .clone();
    assert_eq!(d["registered"], Value::Null, "{d}");
    let (s, v) = p.post("/api/v1/skills/discover", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["results"][0]["action"], "new");
    let (_, cat) = p.get("/api/v1/skills/catalog").await;
    let item = cat["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["reference"] == "api-error-handling@1.0.0")
        .unwrap()
        .clone();
    assert_eq!(item["registration"]["status"], "pending_approval");
    assert_eq!(item["registration"]["registered_by"], "system:skill-discovery");
    assert_eq!(item["format"], "directory");
    assert_eq!(item["package"]["file_count"], 6);
    assert!(item["layers"].to_string().contains("engineering.design/lld"));

    // Viewer: complete tree with contents (nested), kinds and hashes; binary-safe.
    let (s, f) = p.get("/api/v1/skills/api-error-handling/1.0.0/files").await;
    assert_eq!(s, StatusCode::OK, "{f}");
    let files = f["files"].as_array().unwrap();
    let nested = files
        .iter()
        .find(|x| x["path"] == "references/http/status-mapping.md")
        .unwrap();
    assert_eq!(nested["kind"], "markdown");
    assert!(nested["content"].as_str().unwrap().contains("RESOURCE_NOT_FOUND"));
    assert_eq!(
        files
            .iter()
            .find(|x| x["path"] == "examples/config.yaml")
            .unwrap()["kind"],
        "yaml"
    );
    assert_eq!(
        files
            .iter()
            .find(|x| x["path"] == "examples/error-response.json")
            .unwrap()["kind"],
        "json"
    );
    assert_eq!(
        files
            .iter()
            .find(|x| x["path"] == "scripts/validate_errors.py")
            .unwrap()["kind"],
        "code"
    );
    assert!(files.iter().all(|x| x["sha256"].as_str().unwrap().len() == 64));
    assert_eq!(f["in_force_version"], Value::Null);

    // Approve 1.0.0 in the portal (the registrant is the system, so any human approver may).
    let aid = item["registration"]["approval_id"].as_str().unwrap().to_string();
    let (_, doc) = p.get(&format!("/api/v1/approvals/{aid}/document")).await;
    let digest = doc["skill"]["digest"].as_str().unwrap().to_string();
    assert_eq!(doc["skill"]["package"]["tree_hash"], item["package"]["tree_hash"]);
    let (s, v) = p
        .post(
            &format!("/api/v1/approvals/{aid}/decision"),
            json!({"decision": "approve", "comments": "", "artifact_hash": digest}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // Change a nested file and add one ⇒ 1.0.1 pending, with change markers against 1.0.0.
    std::fs::write(
        dir.join("references/error-catalog.md"),
        "# Error catalog\n\n| `CONFLICT` | changed |\n",
    )
    .unwrap();
    std::fs::write(dir.join("references/extra.md"), "# Extra\n").unwrap();
    let (_, v) = p.post("/api/v1/skills/discover", json!({})).await;
    assert_eq!(
        (
            v["results"][0]["action"].as_str(),
            v["results"][0]["version"].as_str()
        ),
        (Some("changed"), Some("1.0.1")),
        "{v}"
    );
    let (_, f) = p.get("/api/v1/skills/api-error-handling/1.0.1/files").await;
    assert_eq!(f["in_force_version"], "1.0.0");
    assert_eq!(f["compared_with"], "1.0.0");
    let ch = |path: &str| {
        f["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["path"] == path)
            .unwrap()["change"]
            .clone()
    };
    assert_eq!(ch("references/error-catalog.md"), "modified");
    assert_eq!(ch("references/extra.md"), "added");
    assert_eq!(ch("SKILL.md"), "unchanged");
    assert_eq!(f["package"]["revision"], 2);

    // Reject 1.0.1 with comments (required by the service for rejections).
    let aid2 = f["approval_id"].as_str().unwrap().to_string();
    let (s, _) = p
        .post(
            &format!("/api/v1/approvals/{aid2}/decision"),
            json!({"decision": "reject", "comments": "", "artifact_hash": f["digest"]}),
        )
        .await;
    assert_ne!(s, StatusCode::OK, "a rejection needs a comment");
    let (s, v) = p
        .post(
            &format!("/api/v1/approvals/{aid2}/decision"),
            json!({"decision": "reject", "comments": "CONFLICT text is wrong", "artifact_hash": f["digest"]}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, f) = p.get("/api/v1/skills/api-error-handling/1.0.1/files").await;
    assert_eq!(f["status"], "rejected");
    let h = f["history"].as_array().unwrap();
    assert_eq!(h.len(), 2, "approval history of both versions");
    assert!(
        h.iter()
            .any(|x| x["decision"]["comments"] == "CONFLICT text is wrong")
    );
    assert_eq!(
        f["in_force_version"], "1.0.0",
        "the approved version stays in force"
    );
}

fn copy(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}
