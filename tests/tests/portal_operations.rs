//! Portal operations (PORTAL-OPERATIONS-PLAN W1–W4): workflow and task operations through
//! the real portal application, the explicit operator/admin identities, project isolation,
//! and the confined *Register existing project* endpoints.
//!
//! Real data: `engineering.design` on the shipped configuration; the model is the labelled
//! test double `ScriptedModelProvider`. Approval decisions are made by the fixture's human
//! approver identity through the approval service, never by the operator.

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::*;
use eaeh_api::portal::{Acting, COOKIE, HostFuture, PortalHost, PortalState, RegisterRequest, app};
use eaeh_core::{
    Principal, PrincipalId, PrincipalKind, ProjectAccess, Role, TraceContext, WorkflowDefinitionId,
    WorkflowInstanceId,
};
use eaeh_governance::{ApprovalDecision, NewPrincipal};
use eaeh_runtime::{ExecutionMode, Harness};
use eaeh_testkit::Script;
use eaeh_workflow::InstanceStatus;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const HOST: &str = "127.0.0.1:8090";
const ORIGIN: &str = "http://127.0.0.1:8090";

async fn token(w: &World, p: &Principal) -> String {
    w.h.identity
        .issue_token(&w.admin, &p.id, "portal-ops-test", 1)
        .await
        .unwrap()
        .token
        .expose()
        .to_string()
}

/// Records what the portal asked the CLI host to do (the real host is `eaeh portal`'s,
/// exercised end to end with the binary).
#[derive(Default)]
struct RecordingHost {
    registered: Mutex<Vec<(PathBuf, RegisterRequest)>>,
}

impl PortalHost for RecordingHost {
    fn register<'a>(
        &'a self,
        _h: &'a Harness,
        admin: &'a Principal,
        root: PathBuf,
        req: RegisterRequest,
    ) -> HostFuture<'a, Value> {
        Box::pin(async move {
            self.registered.lock().unwrap().push((root.clone(), req));
            Ok(json!({"registered_by": admin.id, "workspace": root.display().to_string()}))
        })
    }
    fn ensure_worker<'a>(
        &'a self,
        _h: &'a Harness,
        _o: &'a Principal,
        _p: eaeh_core::ProjectId,
    ) -> HostFuture<'a, Option<String>> {
        Box::pin(async { Ok(None) })
    }
}

struct Portal {
    app: axum::Router,
    tokens: Vec<String>,
    host: Arc<RecordingHost>,
}

struct Ids<'a> {
    viewer: &'a Principal,
    operator: Option<&'a Principal>,
    admin: Option<&'a Principal>,
    roots: Vec<PathBuf>,
}

async fn portal(w: &World, ids: Ids<'_>) -> Portal {
    let vt = token(w, ids.viewer).await;
    let mut tokens = vec![vt.clone()];
    let mut acting = |label: &str, t: String, p: &Principal| {
        tokens.push(t.clone());
        Acting::new(label, t, p.clone())
    };
    let op = match ids.operator {
        Some(o) => Some(acting("operator", token(w, o).await, o)),
        None => None,
    };
    let ad = match ids.admin {
        Some(a) => Some(acting("admin", token(w, a).await, a)),
        None => None,
    };
    let ap = acting("approver", token(w, &w.approver).await, &w.approver);
    let host = Arc::new(RecordingHost::default());
    let state = PortalState::new(
        Acting::new("viewer", vt, ids.viewer.clone()),
        Some(ap),
        SECRET.into(),
        HOST.parse().unwrap(),
        vec![],
        None,
        w.h.execution_mode.to_string(),
    )
    .with_operations(op, ad, ids.roots, Some(host.clone() as Arc<dyn PortalHost>));
    Portal {
        app: app(w.h.clone(), state),
        tokens,
        host,
    }
}

impl Portal {
    async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).to_string();
        for t in &self.tokens {
            assert!(
                !text.contains(t.as_str()),
                "a stored token leaked into a portal response"
            );
        }
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::String(text)),
        )
    }
    async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.send(
            Request::get(path)
                .header(header::HOST, HOST)
                .header(header::COOKIE, format!("{COOKIE}={SECRET}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send(
            Request::post(path)
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, format!("{COOKIE}={SECRET}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }
}

fn design_script(extra: usize) -> Vec<Script> {
    let good = example("requirements.json");
    let read_brd = || Script::tool("knowledge.read", json!({"document": BRD}));
    let mut v = vec![
        read_brd(),
        Script::json(good.clone()),
        read_brd(),
        Script::json(good),
        Script::json(example("hld.json")),
        Script::json(example("lld.json")),
    ];
    for _ in 0..extra {
        v.push(Script::json(example("hld.json")));
        v.push(Script::json(example("lld.json")));
    }
    v
}

async fn start(w: &World) -> WorkflowInstanceId {
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
    w.h.engine.drive(id).await.unwrap();
    id
}

async fn wait_status(w: &World, id: WorkflowInstanceId, want: InstanceStatus, state: &str) {
    for _ in 0..400 {
        let i = w.h.engine.get(id).await.unwrap();
        if i.status == want && i.state == state {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let i = w.h.engine.get(id).await.unwrap();
    panic!(
        "{id}: expected {want:?} at {state}, got {:?} at {}",
        i.status, i.state
    );
}

async fn count(w: &World, sql: &'static str, id: &str) -> i64 {
    let (n,): (i64,) = eaeh_store::sqlx::query_as(sql)
        .bind(id)
        .fetch_one(w.h.db.pool())
        .await
        .unwrap();
    n
}

/// Scenarios 4, 5, 6, 10, 11, 12, 15: view stages and tasks, approvals move the run on,
/// restart from HLD through the portal with preview and confirmation, downstream regenerated
/// and re-approved, approved versions kept, everything audited as the operator.
#[tokio::test]
async fn portal_restarts_from_hld_with_preview_confirmation_and_lineage() {
    let w = world(design_script(1), "").await;
    let p = portal(
        &w,
        Ids {
            viewer: &w.operator,
            operator: Some(&w.operator),
            admin: None,
            roots: vec![],
        },
    )
    .await;
    // Scenario 3: the design workflow is started through the portal (as the operator); the
    // definition list comes from the backend.
    let (_, defs) = p.get("/api/v1/workflow-definitions").await;
    assert!(
        defs["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == "engineering.design")
    );
    let (s, r) = p
        .post(
            "/api/v1/workflows",
            json!({"definition": "engineering.design", "project_id": w.scope.project, "input": {"brd": BRD}}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{r}");
    let src: WorkflowInstanceId = r["id"].as_str().unwrap().parse().unwrap();
    wait_status(&w, src, InstanceStatus::WaitingApproval, "approve_requirements").await;
    let started_by = w.h.engine.get(src).await.unwrap().started_by;
    assert_eq!(started_by, w.operator.id.as_str());

    // Stages, tasks and operations from the backend (nothing hardcoded in the UI).
    let (s, d) = p.get(&format!("/api/v1/workflows/{src}/detail")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(d["operations"]["resume"], true);
    assert_eq!(d["operations"]["may_operate"], true);
    assert!(d["definition"]["states"].as_array().unwrap().len() > 5);
    assert_eq!(d["lineage"]["derived_from"], Value::Null);
    let task_id = d["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["state"] == "render_requirements")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, t) = p.get(&format!("/api/v1/tasks/{task_id}")).await;
    assert_eq!(s, StatusCode::OK, "{t}");
    assert_eq!(t["task"]["state"], "render_requirements");
    assert!(t["depends_on"].as_array().is_some());
    assert_eq!(t["workflow"]["id"], json!(src.to_string()));
    assert!(t["artifact"]["version"].as_u64().is_some(), "{t}");

    // Approvals (human approver) move the run on: requirements → HLD → waiting at LLD.
    let (st, _) = decide(&w, ApprovalDecision::Approve, "ok").await;
    assert_eq!(st, InstanceStatus::WaitingApproval);
    let (st, _) = decide(&w, ApprovalDecision::Approve, "ok").await;
    assert_eq!(st, InstanceStatus::WaitingApproval);
    let before = w.h.engine.get(src).await.unwrap();
    assert_eq!(before.state, "approve_lld");
    let old_hld = before.data.artifacts["render_hld"].clone();

    // Preview: carried, restart point, rerun; approvals kept / repeated; fingerprint.
    let (s, plan) = p
        .post(
            &format!("/api/v1/workflows/{src}/restart/preview"),
            json!({"stage": "hld"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{plan}");
    assert_eq!(plan["carried_approvals"], json!(["approve_requirements"]));
    assert_eq!(plan["repeat_approvals"], json!(["approve_hld"]));
    assert_eq!(plan["source_will_be_cancelled"], true);
    // Injection and gate skipping are refused by the engine.
    let (s, e) = p
        .post(
            &format!("/api/v1/workflows/{src}/restart/preview"),
            json!({"stage": "deploy"}),
        )
        .await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_input"))
    );
    // Confirmation needs the previewed fingerprint and a reason.
    let (s, _) = p
        .post(
            &format!("/api/v1/workflows/{src}/restart"),
            json!({"stage": "hld", "fingerprint": "stale", "reason": "x"}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = p
        .post(
            &format!("/api/v1/workflows/{src}/restart"),
            json!({"stage": "hld", "fingerprint": plan["fingerprint"], "reason": " "}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let body = json!({"stage": "hld", "fingerprint": plan["fingerprint"], "reason": "architecture changed"});
    let (s, r) = p
        .post(&format!("/api/v1/workflows/{src}/restart"), body.clone())
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{r}");
    let new: WorkflowInstanceId = r["id"].as_str().unwrap().parse().unwrap();
    // A double click does not create a second run.
    let (_, r2) = p.post(&format!("/api/v1/workflows/{src}/restart"), body).await;
    assert_eq!(r2["id"], r["id"]);
    wait_status(&w, new, InstanceStatus::WaitingApproval, "approve_hld").await;
    // History preserved: the source is superseded, its approved HLD unchanged and readable.
    let s2 = w.h.engine.get(src).await.unwrap();
    assert_eq!(s2.status, InstanceStatus::Cancelled);
    assert_eq!(s2.data.artifacts["render_hld"], old_hld);
    assert!(w.h.artifacts.read(w.scope, old_hld.id).await.is_ok());
    let n = w.h.engine.get(new).await.unwrap();
    assert!(
        n.data.artifacts["render_hld"].version > old_hld.version,
        "regenerated as a new version"
    );
    // Lineage both ways, through the portal.
    let (_, l) = p.get(&format!("/api/v1/workflows/{new}/lineage")).await;
    assert_eq!(l["derived_from"]["source"], json!(src.to_string()));
    let (_, d) = p.get(&format!("/api/v1/workflows/{src}/detail")).await;
    assert_eq!(
        d["lineage"]["derived_runs"][0]["instance"],
        json!(new.to_string())
    );
    // The HLD needs a new human decision; the LLD follows; the run completes with the
    // carried requirements approval verified by the completion guard.
    decide(&w, ApprovalDecision::Approve, "ok").await;
    decide(&w, ApprovalDecision::Approve, "ok").await;
    wait_status(&w, new, InstanceStatus::Succeeded, "done").await;
    // Audit: the restart is recorded as the operator.
    let (actor,): (String,) = eaeh_store::sqlx::query_as(
        "SELECT actor_id FROM audit_log WHERE action = 'workflow.restart' AND resource_id = ?",
    )
    .bind(new.to_string())
    .fetch_one(w.h.db.pool())
    .await
    .unwrap();
    assert_eq!(actor, w.operator.id.as_str());
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'workflow.superseded' AND resource_id = ?",
            &src.to_string()
        )
        .await,
        1
    );

    // Task queue with filters and the workflow queue with the pending action.
    let (_, q) = p
        .get(&format!(
            "/api/v1/task-queue?project_id={}&stage=hld&limit=10",
            w.scope.project
        ))
        .await;
    let items = q["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "one HLD task per run: {q}");
    assert!(items.iter().all(|i| i["task"]["state"] == "hld"));
    let (_, q) = p
        .get(&format!(
            "/api/v1/task-queue?project_id={}&limit=1",
            w.scope.project
        ))
        .await;
    let cursor = q["next_cursor"].as_str().unwrap().to_string();
    let (_, q2) = p
        .get(&format!(
            "/api/v1/task-queue?project_id={}&limit=1&cursor={}",
            w.scope.project,
            cursor.replace('|', "%7C").replace(':', "%3A").replace('+', "%2B")
        ))
        .await;
    assert_ne!(
        q2["items"][0]["task"]["id"], q["items"][0]["task"]["id"],
        "the cursor pages forward"
    );
    let (_, wq) = p
        .get(&format!(
            "/api/v1/workflow-queue?project_id={}&run_status=cancelled",
            w.scope.project
        ))
        .await;
    assert_eq!(wq["items"].as_array().unwrap().len(), 1);
    assert_eq!(wq["items"][0]["pending_action"]["kind"], "none");
}

/// Scenario 15: operations need the explicit operator identity in the portal, and the backend
/// still authorizes the identity used.
#[tokio::test]
async fn operations_need_the_operator_identity_and_backend_authorization() {
    let w = world(design_script(0), "").await;
    let id = start(&w).await;
    // No operator configured: refused with a hint, nothing changes.
    let p = portal(
        &w,
        Ids {
            viewer: &w.operator,
            operator: None,
            admin: None,
            roots: vec![],
        },
    )
    .await;
    for (path, body) in [
        (format!("/api/v1/workflows/{id}/resume"), json!({})),
        (format!("/api/v1/workflows/{id}/cancel"), json!({})),
        (format!("/api/v1/workflows/{id}/restart/preview"), json!({})),
        ("/api/v1/workflows".to_string(), json!({})),
    ] {
        let (s, e) = p.post(&path, body).await;
        assert_eq!(
            (s, e["error"]["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("operator_not_configured")),
            "{path}"
        );
        assert!(e["error"]["message"].as_str().unwrap().contains("--operator"));
    }
    assert_eq!(
        w.h.engine.get(id).await.unwrap().status,
        InstanceStatus::WaitingApproval
    );
    // An "operator" without workflow.start (the approver) is refused by the backend.
    let p = portal(
        &w,
        Ids {
            viewer: &w.operator,
            operator: Some(&w.approver),
            admin: None,
            roots: vec![],
        },
    )
    .await;
    let (s, _) = p
        .post(&format!("/api/v1/workflows/{id}/restart/preview"), json!({}))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = p.post(&format!("/api/v1/workflows/{id}/resume"), json!({})).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // Still not available through the portal at all: tokens, principals, knowledge ingestion.
    for path in [
        "/api/v1/tokens",
        "/api/v1/principals",
        "/api/v1/knowledge/documents",
    ] {
        let (s, e) = p.post(path, json!({})).await;
        assert_eq!(
            (s, e["error"]["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("not_available_in_portal")),
            "{path}"
        );
    }
    // Resume as the operator is audited; cancel works through the engine.
    let p = portal(
        &w,
        Ids {
            viewer: &w.operator,
            operator: Some(&w.operator),
            admin: None,
            roots: vec![],
        },
    )
    .await;
    let (s, _) = p.post(&format!("/api/v1/workflows/{id}/resume"), json!({})).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'workflow.resume' AND resource_id = ?",
            &id.to_string()
        )
        .await,
        1
    );
    let (s, _) = p.post(&format!("/api/v1/workflows/{id}/cancel"), json!({})).await;
    assert_eq!(s, StatusCode::OK);
    // A finished run is restarted, not resumed.
    let (s, e) = p.post(&format!("/api/v1/workflows/{id}/resume"), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{e}");
}

/// Scenario 15: another project's people see neither runs, tasks nor lineage.
#[tokio::test]
async fn project_isolation_hides_runs_tasks_and_lineage() {
    let w = world(design_script(0), "").await;
    let id = start(&w).await;
    let other =
        w.h.identity
            .create_project(&w.admin, w.scope.tenant, "other-project")
            .await
            .unwrap();
    let outsider =
        w.h.identity
            .create_principal(
                &w.admin,
                NewPrincipal {
                    id: PrincipalId::new("oscar").unwrap(),
                    display_name: "Oscar".into(),
                    kind: PrincipalKind::Human,
                    roles: [Role::Operator].into(),
                    projects: ProjectAccess::Only([other].into()),
                },
            )
            .await
            .unwrap();
    let task = w.h.engine.tasks_for(&w.operator, id).await.unwrap()[0].id.clone();
    let p = portal(
        &w,
        Ids {
            viewer: &outsider,
            operator: Some(&outsider),
            admin: None,
            roots: vec![],
        },
    )
    .await;
    for path in [
        format!("/api/v1/workflows/{id}/detail"),
        format!("/api/v1/workflows/{id}/lineage"),
        format!("/api/v1/tasks/{task}"),
    ] {
        let (s, _) = p.get(&path).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    let (s, _) = p
        .post(&format!("/api/v1/workflows/{id}/restart/preview"), json!({}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = p
        .get(&format!("/api/v1/task-queue?project_id={}", w.scope.project))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, q) = p.get("/api/v1/task-queue").await;
    assert!(q["items"].as_array().unwrap().is_empty());
}

/// Scenarios 7 and 8: a run waiting for external AI is resumed without a duplicate hand-off;
/// a hand-off stopped by a worker failure is retried from the task page.
#[tokio::test]
async fn external_ai_wait_is_resumed_and_a_stopped_handoff_is_retried() {
    let w = world_in(ExecutionMode::ExternalAi, vec![], "").await;
    let id = start(&w).await;
    assert_eq!(
        w.h.engine.get(id).await.unwrap().status,
        InstanceStatus::WaitingExternal
    );
    let p = portal(
        &w,
        Ids {
            viewer: &w.operator,
            operator: Some(&w.operator),
            admin: None,
            roots: vec![],
        },
    )
    .await;
    let ids = id.to_string();
    let handoffs = || {
        count(
            &w,
            "SELECT COUNT(*) FROM external_tasks WHERE workflow_instance_id = ?",
            &ids,
        )
    };
    let n = handoffs().await;
    assert_eq!(n, 1);
    let (s, _) = p.post(&format!("/api/v1/workflows/{id}/resume"), json!({})).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        w.h.engine.get(id).await.unwrap().status,
        InstanceStatus::WaitingExternal
    );
    assert_eq!(handoffs().await, n, "resuming does not hand the task off twice");

    // A worker stopped on the hand-off (e.g. sign-in problem): the task page offers Retry.
    let (tid,): (String,) =
        eaeh_store::sqlx::query_as("SELECT id FROM external_tasks WHERE workflow_instance_id = ?")
            .bind(id.to_string())
            .fetch_one(w.h.db.pool())
            .await
            .unwrap();
    eaeh_store::sqlx::query(
        "UPDATE external_tasks SET dispatch_state = 'blocked', last_failure = 'auth_required' WHERE id = ?",
    )
    .bind(&tid)
    .execute(w.h.db.pool())
    .await
    .unwrap();
    let (s, t) = p.get(&format!("/api/v1/tasks/{tid}")).await;
    assert_eq!(s, StatusCode::OK, "{t}");
    assert_eq!(t["handoff"]["dispatch_state"], "blocked");
    assert_eq!(t["operations"]["retry_handoff"], true);
    assert!(
        t["handoff"].get("package").is_none(),
        "the package is not part of the read model"
    );
    let (s, r) = p.post(&format!("/api/v1/tasks/{tid}/retry"), json!({})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (state,): (String,) =
        eaeh_store::sqlx::query_as("SELECT dispatch_state FROM external_tasks WHERE id = ?")
            .bind(&tid)
            .fetch_one(w.h.db.pool())
            .await
            .unwrap();
    assert_eq!(state, "pending");
    assert_eq!(
        count(
            &w,
            "SELECT COUNT(*) FROM audit_log WHERE action = 'external_task.retry' AND resource_id = ?",
            &tid
        )
        .await,
        1
    );
    // Execution and integration read models.
    let (s, e) = p
        .get(&format!("/api/v1/executions?project_id={}", w.scope.project))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(e["handoffs"].as_array().unwrap().len(), 1);
    let (s, i) = p
        .get(&format!("/api/v1/integrations?project_id={}", w.scope.project))
        .await;
    assert_eq!(s, StatusCode::OK, "{i}");
    assert_eq!(i["installation_execution_mode"], "external_ai");
    assert_eq!(i["direct_model_calls"], false);
}

/// Scenarios 1, 2 and 15 (portal side): browsing and detection are confined to the workspace
/// roots; registration needs the admin identity, refuses duplicates and documents outside
/// the folder, and passes the confirmed values to the shared registration service.
#[tokio::test]
async fn register_existing_project_is_confined_validated_and_needs_the_admin() {
    let w = world(vec![], "").await;
    let base = tempfile::tempdir().unwrap();
    let ws = base.path().join("workspaces");
    let app_dir = ws.join("orders-api");
    std::fs::create_dir_all(app_dir.join("src/main/java")).unwrap();
    std::fs::create_dir_all(app_dir.join("docs")).unwrap();
    std::fs::write(
        app_dir.join("pom.xml"),
        "<project><artifactId>spring-boot-starter-parent</artifactId></project>",
    )
    .unwrap();
    std::fs::write(app_dir.join("docs/BRD.md"), "# Orders BRD\n").unwrap();
    std::fs::write(base.path().join("secret.md"), "outside").unwrap();
    std::fs::create_dir_all(ws.join("not-a-project")).unwrap();
    let roots = vec![ws.clone()];

    // Without --admin: browsing and detection work, registration is refused.
    let p = portal(
        &w,
        Ids {
            viewer: &w.operator,
            operator: None,
            admin: None,
            roots: roots.clone(),
        },
    )
    .await;
    let (s, l) = p.get("/portal/fs").await;
    assert_eq!(s, StatusCode::OK);
    let root_c = std::fs::canonicalize(&ws).unwrap();
    assert_eq!(l["entries"][0]["path"], json!(root_c.display().to_string()));
    let (s, l) = p.get(&format!("/portal/fs?path={}", root_c.display())).await;
    assert_eq!(s, StatusCode::OK, "{l}");
    let names: Vec<_> = l["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["not-a-project", "orders-api"]);
    let (s, _) = p.get(&format!("/portal/fs?path={}", base.path().display())).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "outside the roots");
    let (s, _) = p.get("/portal/fs?path=../").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, d) = p
        .post(
            "/portal/projects/inspect",
            json!({"path": app_dir.display().to_string()}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["detection"]["framework"], "Spring Boot");
    assert_eq!(
        d["detection"]["documents"]["brd_candidates"],
        json!(["docs/BRD.md"])
    );
    assert_eq!(d["registration"]["state"], "new");
    let (s, _) = p.post("/portal/projects/inspect", json!({"path": "/etc"})).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = p
        .post("/portal/projects/inspect", json!({"path": "missing"}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let req = json!({"path": app_dir.display().to_string(), "name": "orders-api", "brd": "docs/BRD.md", "developer": "dev", "steward": "stew", "approvers": ["appr"]});
    let (s, e) = p.post("/portal/projects/register", req.clone()).await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("admin_not_configured"))
    );
    // The session secret is required for portal-process endpoints too.
    let res = p
        .app
        .clone()
        .oneshot(
            Request::get("/portal/fs")
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // With --admin.
    let p = portal(
        &w,
        Ids {
            viewer: &w.operator,
            operator: None,
            admin: Some(&w.admin),
            roots,
        },
    )
    .await;
    let bad = |patch: Value| {
        let mut r = req.clone();
        for (k, v) in patch.as_object().unwrap() {
            r[k] = v.clone();
        }
        r
    };
    let (s, _) = p
        .post(
            "/portal/projects/register",
            bad(json!({"brd": "../../secret.md"})),
        )
        .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "documents stay inside the project folder"
    );
    let (s, _) = p
        .post(
            "/portal/projects/register",
            bad(json!({"knowledge": ["/etc/hosts"]})),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = p
        .post(
            "/portal/projects/register",
            bad(json!({"path": ws.join("not-a-project").display().to_string()})),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "not a project folder");
    let existing = w.h.identity.projects(w.scope.tenant).await.unwrap();
    let taken = existing
        .iter()
        .find(|(id, _)| *id == w.scope.project)
        .unwrap()
        .1
        .clone();
    let (s, e) = p
        .post("/portal/projects/register", bad(json!({"name": taken})))
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "duplicate name: {e}");
    assert!(
        p.host.registered.lock().unwrap().is_empty(),
        "nothing reached the registration service yet"
    );
    let (s, r) = p.post("/portal/projects/register", req).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    assert_eq!(r["registered_by"], "admin");
    let calls = p.host.registered.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, std::fs::canonicalize(&app_dir).unwrap());
    assert_eq!(calls[0].1.brd.as_deref(), Some("docs/BRD.md"));
}
