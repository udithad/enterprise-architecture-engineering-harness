//! API contract & security tests (AC-11 via API, AC-24, AC-27, AC-28).

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use eaeh_api::{AppState, router};
use eaeh_core::PrincipalId;
use eaeh_runtime::{Harness, HarnessConfig};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

struct Fx {
    _d: tempfile::TempDir,
    app: Router,
    admin: String,
    project: String,
}

async fn fx(rpm: u32) -> Fx {
    let d = tempfile::tempdir().unwrap();
    let mut cfg =
        HarnessConfig::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/eaeh.toml"))
            .unwrap();
    cfg.storage.data_dir = d.path().to_path_buf();
    cfg.isolation.container_enabled = false;
    cfg.api.requests_per_minute = rpm;
    cfg.api.max_body_bytes = 64 * 1024;
    let h = Arc::new(Harness::builder(cfg).build().await.unwrap());
    let (_admin, project, tok) = h
        .identity
        .bootstrap(
            "acme",
            "platform",
            PrincipalId::new("admin").unwrap(),
            "Admin",
            30,
        )
        .await
        .unwrap();
    let app = router(AppState::new(h));
    Fx {
        _d: d,
        app,
        admin: tok.token.expose().to_string(),
        project: project.to_string(),
    }
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    assert!(res.headers().contains_key("x-request-id"));
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes).unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()));
    (status, v)
}

async fn user(f: &Fx, id: &str, roles: Value, projects: Option<Value>) -> String {
    let mut body = json!({"id": id, "display_name": id, "roles": roles});
    if let Some(p) = projects {
        body["projects"] = p;
    }
    let (s, v) = call(&f.app, "POST", "/v1/principals", Some(&f.admin), Some(body)).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let (s, v) = call(
        &f.app,
        "POST",
        "/v1/tokens",
        Some(&f.admin),
        Some(json!({"principal_id": id, "label": "test"})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    v["token"].as_str().unwrap().to_string()
}

async fn wait_status(f: &Fx, token: &str, id: &str, want: &str) -> String {
    for _ in 0..100 {
        let (_, v) = call(&f.app, "GET", &format!("/v1/workflows/{id}"), Some(token), None).await;
        let s = v["instance"]["status"].as_str().unwrap_or("").to_string();
        if s == want || ["succeeded", "failed", "cancelled", "escalated"].contains(&s.as_str()) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    "timeout".into()
}

#[tokio::test]
async fn authentication_and_error_contract() {
    let f = fx(600).await;
    let (s, v) = call(&f.app, "GET", "/v1/health", None, None).await;
    assert_eq!((s, v["status"].as_str()), (StatusCode::OK, Some("ok")));
    let (s, v) = call(&f.app, "GET", "/v1/me", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(v["error"]["code"], "unauthenticated");
    let (s, _) = call(
        &f.app,
        "GET",
        "/v1/me",
        Some("eaeh_0000000000000000000000000000000000000000000000000000000000000000"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v) = call(&f.app, "GET", "/v1/me", Some(&f.admin), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["id"], "admin");
    let (s, v) = call(&f.app, "GET", "/v1/nope", Some(&f.admin), None).await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found"))
    );
    // Malformed JSON and oversized bodies.
    let req = Request::builder()
        .method("POST")
        .uri("/v1/projects")
        .header("authorization", format!("Bearer {}", f.admin))
        .header("content-type", "application/json")
        .body(Body::from("{not json"))
        .unwrap();
    assert_eq!(
        f.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let (s, _) = call(
        &f.app,
        "POST",
        "/v1/projects",
        Some(&f.admin),
        Some(json!({"name": "x".repeat(100_000)})),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn approval_gate_cannot_be_bypassed_through_the_api() {
    let f = fx(600).await;
    let p = json!([f.project]);
    let op = user(&f, "olivia", json!(["operator"]), Some(p.clone())).await;
    let ap = user(&f, "arthur", json!(["approver"]), Some(p.clone())).await;
    let (s, v) = call(
        &f.app,
        "POST",
        "/v1/workflows",
        Some(&op),
        Some(json!({"definition": "demo.approval", "project_id": f.project})),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let wf = v["id"].as_str().unwrap().to_string();
    assert_eq!(
        wait_status(&f, &op, &wf, "waiting_approval").await,
        "waiting_approval"
    );
    let (_, v) = call(
        &f.app,
        "GET",
        &format!("/v1/approvals?project_id={}&status=pending", f.project),
        Some(&ap),
        None,
    )
    .await;
    let a = v["items"][0].clone();
    let aid = a["id"].as_str().unwrap();
    let hash = a["artifact_hash"].as_str().unwrap();
    let decide = |tok: &str, h: &str| {
        let app = f.app.clone();
        let tok = tok.to_string();
        let body = json!({"decision": "approve", "comments": "ok", "artifact_hash": h});
        let uri = format!("/v1/approvals/{aid}/decision");
        async move { call(&app, "POST", &uri, Some(&tok), Some(body)).await }
    };
    // Operator (initiator) and admin cannot approve; wrong hash is a conflict.
    assert_eq!(decide(&op, hash).await.0, StatusCode::FORBIDDEN);
    assert_eq!(decide(&f.admin, hash).await.0, StatusCode::FORBIDDEN);
    assert_eq!(decide(&ap, "deadbeef").await.0, StatusCode::CONFLICT);
    // Publish has not executed.
    let (_, v) = call(&f.app, "GET", &format!("/v1/workflows/{wf}"), Some(&op), None).await;
    assert!(v["instance"]["data"]["outputs"].get("publish").is_none());
    let (s, v) = decide(&ap, hash).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "approved");
    assert_eq!(v["decision"]["approver_id"], "arthur");
    assert_eq!(wait_status(&f, &op, &wf, "succeeded").await, "succeeded");
    // Deciding again is a conflict.
    assert_eq!(decide(&ap, hash).await.0, StatusCode::CONFLICT);
    // Artifact retrieval is scoped.
    let (_, v) = call(&f.app, "GET", &format!("/v1/workflows/{wf}"), Some(&op), None).await;
    let art = v["instance"]["data"]["artifacts"]["draft"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, v) = call(
        &f.app,
        "GET",
        &format!("/v1/artifacts/{art}?project_id={}", f.project),
        Some(&ap),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["artifact"]["content_hash"], hash);
}

#[tokio::test]
async fn cross_project_isolation_and_role_checks() {
    let f = fx(600).await;
    let (s, v) = call(
        &f.app,
        "POST",
        "/v1/projects",
        Some(&f.admin),
        Some(json!({"name": "other"})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let other = v["id"].as_str().unwrap().to_string();
    let op = user(&f, "olivia", json!(["operator"]), Some(json!([f.project]))).await;
    let outsider = user(&f, "otto", json!(["operator", "viewer"]), Some(json!([other]))).await;
    let (_, v) = call(
        &f.app,
        "POST",
        "/v1/workflows",
        Some(&op),
        Some(json!({"definition": "demo.approval", "project_id": f.project})),
    )
    .await;
    let wf = v["id"].as_str().unwrap().to_string();
    let (s, _) = call(
        &f.app,
        "GET",
        &format!("/v1/workflows/{wf}"),
        Some(&outsider),
        None,
    )
    .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "other-project principal cannot see the workflow"
    );
    let (s, _) = call(
        &f.app,
        "POST",
        &format!("/v1/workflows/{wf}/cancel"),
        Some(&outsider),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &f.app,
        "GET",
        &format!("/v1/approvals?project_id={}", f.project),
        Some(&outsider),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = call(
        &f.app,
        "POST",
        "/v1/workflows",
        Some(&outsider),
        Some(json!({"definition": "demo.approval", "project_id": f.project})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // Role checks.
    let (s, _) = call(&f.app, "GET", "/v1/audit", Some(&op), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = call(
        &f.app,
        "POST",
        "/v1/principals",
        Some(&op),
        Some(json!({"id": "evil", "display_name": "e", "roles": ["admin"]})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = call(&f.app, "GET", "/v1/audit/verify", Some(&f.admin), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["valid"], true);
    let (_, v) = call(&f.app, "GET", "/v1/audit?limit=500", Some(&f.admin), None).await;
    let denied = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["outcome"] == "denied")
        .count();
    assert!(denied >= 3, "API authorization denials are audited");
    let (s, _) = call(&f.app, "POST", "/v1/plugins/acme.x/1.0.0/enable", Some(&op), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn pagination_tokens_and_rate_limits() {
    let f = fx(600).await;
    let op = user(&f, "olivia", json!(["operator"]), Some(json!([f.project]))).await;
    for _ in 0..3 {
        let (s, _) = call(
            &f.app,
            "POST",
            "/v1/workflows",
            Some(&op),
            Some(json!({"definition": "demo.approval", "project_id": f.project})),
        )
        .await;
        assert_eq!(s, StatusCode::ACCEPTED);
    }
    let (_, p1) = call(
        &f.app,
        "GET",
        &format!("/v1/workflows?project_id={}&limit=2", f.project),
        Some(&op),
        None,
    )
    .await;
    assert_eq!(p1["items"].as_array().unwrap().len(), 2);
    let cursor = p1["next_cursor"].as_str().unwrap();
    let (_, p2) = call(
        &f.app,
        "GET",
        &format!("/v1/workflows?project_id={}&limit=2&cursor={cursor}", f.project),
        Some(&op),
        None,
    )
    .await;
    assert_eq!(p2["items"].as_array().unwrap().len(), 1);
    assert!(p2["next_cursor"].is_null());

    // Own-token rotation and revocation.
    let (_, list) = call(&f.app, "GET", "/v1/tokens", Some(&op), None).await;
    let tid = list["items"][0]["id"].as_str().unwrap().to_string();
    let (s, v) = call(
        &f.app,
        "POST",
        &format!("/v1/tokens/{tid}/rotate?ttl_days=5"),
        Some(&op),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let new_tok = v["token"].as_str().unwrap().to_string();
    assert_eq!(
        call(&f.app, "GET", "/v1/me", Some(&op), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&f.app, "GET", "/v1/me", Some(&new_tok), None).await.0,
        StatusCode::OK
    );
    let new_id = v["id"].as_str().unwrap().to_string();
    assert_eq!(
        call(
            &f.app,
            "POST",
            &format!("/v1/tokens/{new_id}/revoke"),
            Some(&new_tok),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&f.app, "GET", "/v1/me", Some(&new_tok), None).await.0,
        StatusCode::UNAUTHORIZED
    );

    let f = fx(3).await;
    for _ in 0..3 {
        assert_eq!(
            call(&f.app, "GET", "/v1/me", Some(&f.admin), None).await.0,
            StatusCode::OK
        );
    }
    let (s, v) = call(&f.app, "GET", "/v1/me", Some(&f.admin), None).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(v["error"]["code"], "rate_limited");
}

#[tokio::test]
async fn metrics_and_health_details_require_permission() {
    let f = fx(600).await;
    let viewer = user(&f, "vera", json!(["viewer"]), None).await;
    assert_eq!(
        call(&f.app, "GET", "/v1/metrics", Some(&viewer), None).await.0,
        StatusCode::FORBIDDEN
    );
    let (s, v) = call(&f.app, "GET", "/v1/metrics", Some(&f.admin), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v.as_str().unwrap().contains("eaeh_outbox_pending"));
    let (s, v) = call(&f.app, "GET", "/v1/health/details", Some(&f.admin), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["migrations"], json!([1, 2, 3, 4, 5, 6, 7, 8, 9]));
}

#[tokio::test]
async fn knowledge_and_skill_routes_are_governed() {
    let f = fx(600).await;
    let steward = user(&f, "sam", json!(["knowledge_steward"]), None).await;
    let approver = user(&f, "arthur", json!(["approver"]), None).await;
    let viewer = user(&f, "vera", json!(["viewer"]), Some(json!([f.project]))).await;
    let (s, _) = call(
        &f.app,
        "POST",
        "/v1/knowledge/sources",
        Some(&viewer),
        Some(json!({"name": "std", "kind": "standard", "description": "d"})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = call(
        &f.app,
        "POST",
        "/v1/knowledge/sources",
        Some(&steward),
        Some(json!({"name": "std", "kind": "standard", "description": "Standards"})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let doc = json!({
        "source": "std", "subject": "error-format", "title": "Errors", "format": "markdown",
        "content": "# Errors\nUse problem+json for every error response.\n",
        "scope": {"level": "enterprise"}, "mandatory": true, "governance_project": f.project
    });
    let (s, _) = call(
        &f.app,
        "POST",
        "/v1/knowledge/documents",
        Some(&viewer),
        Some(doc.clone()),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, d) = call(
        &f.app,
        "POST",
        "/v1/knowledge/documents",
        Some(&steward),
        Some(doc),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{d}");
    assert_eq!(d["status"], "pending_approval");
    // Pending standards are not searchable.
    let search = json!({"project_id": f.project, "text": "problem json errors", "mode": "hybrid", "k": 3});
    let (s, v) = call(
        &f.app,
        "POST",
        "/v1/knowledge/search",
        Some(&viewer),
        Some(search.clone()),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
    // Steward (author) cannot approve; approver approves with the content hash via /v1/approvals.
    let aid = d["approval_id"].as_str().unwrap();
    let decide = json!({"decision": "approve", "artifact_hash": d["content_hash"]});
    assert_eq!(
        call(
            &f.app,
            "POST",
            &format!("/v1/approvals/{aid}/decision"),
            Some(&steward),
            Some(decide.clone())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &f.app,
            "POST",
            &format!("/v1/approvals/{aid}/decision"),
            Some(&approver),
            Some(decide)
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, v) = call(
        &f.app,
        "POST",
        "/v1/knowledge/search",
        Some(&viewer),
        Some(search),
    )
    .await;
    assert_eq!(v["items"][0]["citation"], "std/error-format@v1#0");
    // Viewer cannot search a project it has no access to; nor manage documents.
    let (s, v) = call(
        &f.app,
        "POST",
        "/v1/projects",
        Some(&f.admin),
        Some(json!({"name": "other"})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let other = v["id"].as_str().unwrap();
    assert_eq!(
        call(
            &f.app,
            "POST",
            "/v1/knowledge/search",
            Some(&viewer),
            Some(json!({"project_id": other, "text": "errors"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&f.app, "GET", "/v1/knowledge/documents", Some(&viewer), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (s, v) = call(
        &f.app,
        "GET",
        "/v1/knowledge/documents?status=active",
        Some(&steward),
        None,
    )
    .await;
    assert_eq!((s, v["items"].as_array().unwrap().len()), (StatusCode::OK, 1));

    // Skills: register (pending), approve through /v1/approvals, list, deprecate.
    let skill = json!({"project_id": f.project, "definition": {
        "id": "api-review", "version": "1.0.0", "description": "Review APIs", "capabilities": ["api-review"],
        "instructions": "Cite sources.", "required_tools": ["knowledge.search"], "max_risk": "read_only"}});
    assert_eq!(
        call(&f.app, "POST", "/v1/skills", Some(&viewer), Some(skill.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (s, r) = call(&f.app, "POST", "/v1/skills", Some(&steward), Some(skill)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    assert_eq!(r["status"], "pending_approval");
    let said = r["approval_id"].as_str().unwrap();
    let (s, _) = call(
        &f.app,
        "POST",
        &format!("/v1/approvals/{said}/decision"),
        Some(&approver),
        Some(json!({"decision": "approve", "artifact_hash": r["digest"]})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, v) = call(&f.app, "GET", "/v1/skills", Some(&viewer), None).await;
    assert_eq!(v["items"][0]["status"], "approved");
    let (s, v) = call(
        &f.app,
        "POST",
        "/v1/skills/api-review/1.0.0/deprecate",
        Some(&steward),
        None,
    )
    .await;
    assert_eq!((s, v["status"].as_str()), (StatusCode::OK, Some("deprecated")));
    // ABAC attributes are admin-managed.
    assert_eq!(
        call(
            &f.app,
            "POST",
            &format!("/v1/projects/{}/domain", f.project),
            Some(&steward),
            Some(json!({"domain": "commerce"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &f.app,
            "POST",
            &format!("/v1/projects/{}/domain", f.project),
            Some(&f.admin),
            Some(json!({"domain": "commerce"}))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &f.app,
            "POST",
            "/v1/principals/vera/teams",
            Some(&f.admin),
            Some(json!({"teams": ["platform"]}))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn request_changes_and_traceability_routes_are_governed() {
    let f = fx(600).await;
    let p = json!([f.project]);
    let op = user(&f, "olivia", json!(["operator"]), Some(p.clone())).await;
    let ap = user(&f, "arthur", json!(["approver"]), Some(p.clone())).await;
    let outsider = user(&f, "otto", json!(["viewer"]), Some(json!([]))).await;
    let (_, v) = call(
        &f.app,
        "POST",
        "/v1/workflows",
        Some(&op),
        Some(json!({"definition": "demo.approval", "project_id": f.project})),
    )
    .await;
    let wf = v["id"].as_str().unwrap().to_string();
    wait_status(&f, &op, &wf, "waiting_approval").await;
    let (_, v) = call(
        &f.app,
        "GET",
        &format!("/v1/approvals?project_id={}&status=pending", f.project),
        Some(&ap),
        None,
    )
    .await;
    let aid = v["items"][0]["id"].as_str().unwrap().to_string();
    let hash = v["items"][0]["artifact_hash"].as_str().unwrap().to_string();
    let uri = format!("/v1/approvals/{aid}/decision");
    // Unknown decisions are rejected; request_changes needs comments; an operator cannot use it.
    let (s, _) = call(
        &f.app,
        "POST",
        &uri,
        Some(&ap),
        Some(json!({"decision": "maybe", "comments": "", "artifact_hash": hash})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &f.app,
        "POST",
        &uri,
        Some(&ap),
        Some(json!({"decision": "request_changes", "comments": "", "artifact_hash": hash})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &f.app,
        "POST",
        &uri,
        Some(&op),
        Some(json!({"decision": "request_changes", "comments": "x", "artifact_hash": hash})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = call(
        &f.app,
        "POST",
        &uri,
        Some(&ap),
        Some(json!({"decision": "request_changes", "comments": "please redo", "artifact_hash": hash})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "changes_requested");
    // demo.approval has no changes_requested transition: it revises through `rejected` → draft.
    wait_status(&f, &op, &wf, "waiting_approval").await;
    let (_, v) = call(&f.app, "GET", &format!("/v1/workflows/{wf}"), Some(&op), None).await;
    assert_eq!(v["instance"]["data"]["revisions"]["review"], 1);
    assert_eq!(v["instance"]["data"]["feedback"]["review"], "please redo");

    // Traceability: project_id required, project access enforced.
    let (s, v) = call(
        &f.app,
        "GET",
        &format!("/v1/traceability?project_id={}", f.project),
        Some(&op),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["items"], json!([]));
    let (s, _) = call(&f.app, "GET", "/v1/traceability", Some(&op), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &f.app,
        "GET",
        &format!("/v1/traceability?project_id={}&id=REQ-F-001", f.project),
        Some(&outsider),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = call(
        &f.app,
        "GET",
        &format!("/v1/traceability?project_id={}", f.project),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

/// Governance Portal read models (ADR-0026): authenticated, project-scoped, read-only.
#[tokio::test]
async fn portal_read_models_are_authenticated_and_project_scoped() {
    let f = fx(600).await;
    let (_, v) = call(
        &f.app,
        "POST",
        "/v1/projects",
        Some(&f.admin),
        Some(json!({"name": "other"})),
    )
    .await;
    let other = v["id"].as_str().unwrap().to_string();
    let op = user(&f, "olivia", json!(["operator"]), Some(json!([f.project]))).await;
    let outsider = user(&f, "otto", json!(["operator", "viewer"]), Some(json!([other]))).await;
    let ap = user(&f, "arthur", json!(["approver"]), Some(json!([f.project]))).await;
    let (_, v) = call(
        &f.app,
        "POST",
        "/v1/workflows",
        Some(&op),
        Some(json!({"definition": "demo.approval", "project_id": f.project})),
    )
    .await;
    let wf = v["id"].as_str().unwrap().to_string();
    assert_eq!(
        wait_status(&f, &op, &wf, "waiting_approval").await,
        "waiting_approval"
    );

    for path in [
        "/v1/dashboard".to_string(),
        "/v1/activity".into(),
        "/v1/approval-queue".into(),
        "/v1/workflow-queue".into(),
        format!("/v1/workflows/{wf}/detail"),
        format!("/v1/projects/{}/overview", f.project),
    ] {
        let (s, _) = call(&f.app, "GET", &path, None, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{path}");
    }

    let (s, d) = call(&f.app, "GET", "/v1/dashboard", Some(&op), None).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["summary"]["projects"], 1);
    assert_eq!(d["summary"]["waiting_approval"], 1);
    assert_eq!(d["summary"]["pending_approvals"], 1);
    assert_eq!(d["active"][0]["workflow"]["id"], wf);
    // Project rows carry the derived lifecycle status and the current workflow.
    assert_eq!(d["projects"][0]["status"], "waiting_approval");
    assert_eq!(d["projects"][0]["current_workflow"]["id"], wf);
    assert_eq!(d["projects"][0]["current_workflow"]["state"], "review");

    // The outsider sees only their own (empty) project, never the other's work.
    let (_, d) = call(&f.app, "GET", "/v1/dashboard", Some(&outsider), None).await;
    assert_eq!(d["summary"]["projects"], 1);
    assert_eq!(d["projects"][0]["id"], other);
    assert_eq!(
        d["projects"][0]["status"], "ready",
        "a project without workflows is listed as ready"
    );
    assert_eq!(d["projects"][0]["current_workflow"], Value::Null);
    assert!(
        !d.to_string().contains(&wf),
        "no trace of the other project's workflow"
    );
    assert_eq!(d["active"], json!([]));
    assert_eq!(d["pending_approvals"], json!([]));
    let (_, l) = call(&f.app, "GET", "/v1/projects", Some(&outsider), None).await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1);
    assert_eq!(l["items"][0]["id"], other);
    let (s, _) = call(
        &f.app,
        "GET",
        &format!("/v1/workflows/{wf}/detail"),
        Some(&outsider),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &f.app,
        "GET",
        &format!("/v1/projects/{}/overview", f.project),
        Some(&outsider),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, q) = call(
        &f.app,
        "GET",
        &format!("/v1/approval-queue?project_id={}", other),
        Some(&outsider),
        None,
    )
    .await;
    assert_eq!(q["items"], json!([]));
    let (s, _) = call(
        &f.app,
        "GET",
        &format!("/v1/approval-queue?project_id={}", f.project),
        Some(&outsider),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Approval document: approval.read + artifact.read; an outsider gets not-found.
    let (_, q) = call(&f.app, "GET", "/v1/approval-queue", Some(&ap), None).await;
    let aid = q["items"][0]["id"].as_str().unwrap().to_string();
    let (s, doc) = call(
        &f.app,
        "GET",
        &format!("/v1/approvals/{aid}/document"),
        Some(&ap),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{doc}");
    assert_eq!(doc["approval"]["status"], "pending");
    let (s, _) = call(
        &f.app,
        "GET",
        &format!("/v1/approvals/{aid}/document"),
        Some(&outsider),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = call(&f.app, "GET", "/v1/approval-queue?status=bogus", Some(&op), None).await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_input"))
    );

    // Latest-first audit keeps audit.read.
    let (s, _) = call(&f.app, "GET", "/v1/audit?order=desc", Some(&op), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = call(
        &f.app,
        "GET",
        "/v1/audit?order=desc&limit=5",
        Some(&f.admin),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let seqs: Vec<i64> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["seq"].as_i64().unwrap())
        .collect();
    assert!(seqs.windows(2).all(|w| w[0] > w[1]), "{seqs:?}");
    let before = seqs[seqs.len() - 1];
    let (_, v2) = call(
        &f.app,
        "GET",
        &format!("/v1/audit?order=desc&limit=5&before_seq={before}"),
        Some(&f.admin),
        None,
    )
    .await;
    assert!(
        v2["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["seq"].as_i64().unwrap() < before)
    );
    let (s, _) = call(&f.app, "GET", "/v1/audit?order=sideways", Some(&f.admin), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
