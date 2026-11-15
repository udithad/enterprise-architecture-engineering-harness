//! Governance Portal server core (ADR-0026), used by `eaeh portal`.
//!
//! * Serves the built web UI and mounts the unchanged `/v1` router under `/api`, so every
//!   portal request passes the same authentication, authorization, rate limiting, approval
//!   manager and audit as any API client.
//! * No login: requests carry the bearer token of an identity the developer stored locally
//!   (resolved by the CLI). Tokens stay in the server process and never reach the browser.
//! * Reads as the viewer; approval decisions only as the explicitly configured approver
//!   (read-only without one).
//! * Only an allowlist of read endpoints plus `POST /v1/approvals/{id}/decision`.
//! * Local-only guard: `Host` check (DNS rebinding), `Origin` check on writes (CSRF) and a
//!   per-launch secret exchanged for an `HttpOnly; SameSite=Strict` cookie, so another
//!   local process cannot use the stored approver identity through the portal.
//!
//! A trusted local-development tool, not a multi-user or production server.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Extension, Json, Router};
use eaeh_core::Principal;
use eaeh_runtime::Harness;
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const COOKIE: &str = "eaeh_portal";

/// An identity the portal acts as. The token never leaves this process.
pub struct Acting {
    pub label: String,
    token: String,
    pub principal: Principal,
}

impl std::fmt::Debug for Acting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Acting")
            .field("label", &self.label)
            .field("principal", &self.principal.id)
            .finish_non_exhaustive()
    }
}

pub struct PortalState {
    pub viewer: Acting,
    pub approver: Option<Acting>,
    secret: String,
    allowed_hosts: Vec<String>,
    allowed_origins: Vec<String>,
    static_dir: Option<PathBuf>,
    execution_mode: String,
}

impl PortalState {
    pub fn new(
        viewer: Acting,
        approver: Option<Acting>,
        secret: String,
        addr: SocketAddr,
        dev_origins: Vec<String>,
        static_dir: Option<PathBuf>,
        execution_mode: String,
    ) -> Arc<Self> {
        let port = addr.port();
        let hosts = vec![
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("[::1]:{port}"),
        ];
        let mut origins: Vec<String> = hosts.iter().map(|h| format!("http://{h}")).collect();
        origins.extend(
            dev_origins
                .into_iter()
                .map(|o| o.trim_end_matches('/').to_string()),
        );
        Arc::new(Self {
            viewer,
            approver,
            secret,
            allowed_hosts: hosts,
            allowed_origins: origins,
            static_dir,
            execution_mode,
        })
    }
}

impl Acting {
    pub fn new(label: impl Into<String>, token: impl Into<String>, principal: Principal) -> Self {
        Self {
            label: label.into(),
            token: token.into(),
            principal,
        }
    }
}

/// The portal application (also used by tests).
/// Which EAEH installation the portal shows (several can exist on one computer, e.g. a
/// demo under `target/` and the real one), so the UI can say so explicitly.
#[derive(Clone)]
struct Installation(Value);

fn installation(h: &Harness) -> Installation {
    let canon = |p: &Path| {
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .display()
            .to_string()
    };
    // Optional label set by the launcher (e.g. `DEMO` from scripts/demo-portal.sh).
    let label = std::env::var("EAEH_INSTALLATION_LABEL")
        .ok()
        .map(|l| l.trim().chars().take(40).collect::<String>())
        .filter(|l| !l.is_empty());
    Installation(json!({
        "data_dir": canon(&h.config.data_dir()),
        "config_dir": canon(&h.config.base_dir),
        "label": label,
    }))
}

pub fn app(h: Arc<Harness>, state: Arc<PortalState>) -> Router {
    let inst = installation(&h);
    let api = crate::router(crate::AppState::new(h.clone())).layer(middleware::from_fn_with_state(
        (state.clone(), h),
        inject_identity,
    ));
    Router::new()
        .route("/portal/session", post(open_session).get(session_info))
        .with_state(state.clone())
        .nest("/api", api)
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(state.clone(), local_guard))
        .layer(Extension(state))
        .layer(Extension(inst))
}

fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"error": {"code": code, "message": message}}))).into_response()
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

fn has_session(st: &PortalState, headers: &HeaderMap) -> bool {
    session_cookie(headers).is_some_and(|c| ct_eq(&c, &st.secret))
}

/// Host (DNS rebinding), Origin (cross-site writes) and session checks, plus security headers.
async fn local_guard(State(st): State<Arc<PortalState>>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !st.allowed_hosts.iter().any(|h| h == host) {
        return err(
            StatusCode::FORBIDDEN,
            "host_not_allowed",
            "the portal only answers on its loopback address",
        );
    }
    let unsafe_method = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if unsafe_method {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if !st.allowed_origins.iter().any(|o| o == origin) {
            return err(
                StatusCode::FORBIDDEN,
                "origin_not_allowed",
                "cross-origin requests are refused",
            );
        }
    }
    let path = req.uri().path().to_string();
    let needs_session = path.starts_with("/api/") || (path == "/portal/session" && !unsafe_method);
    if needs_session && !has_session(&st, req.headers()) {
        return err(
            StatusCode::UNAUTHORIZED,
            "portal_session_required",
            "open the portal with the link printed by `eaeh portal` (it carries the launch secret)",
        );
    }
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    );
    if path.starts_with("/api/") || path.starts_with("/portal/") {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

#[derive(Deserialize)]
struct OpenSession {
    launch: String,
}

async fn open_session(State(st): State<Arc<PortalState>>, Json(b): Json<OpenSession>) -> Response {
    if !ct_eq(b.launch.trim(), &st.secret) {
        return err(
            StatusCode::UNAUTHORIZED,
            "invalid_launch_secret",
            "the launch secret is not valid for this portal process",
        );
    }
    let mut res = Json(json!({"ok": true})).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!(
        "{COOKIE}={}; HttpOnly; SameSite=Strict; Path=/",
        st.secret
    )) {
        res.headers_mut().insert(header::SET_COOKIE, v);
    }
    res
}

fn acting_json(a: &Acting) -> Value {
    json!({"profile": a.label, "principal": a.principal.id, "kind": a.principal.kind_str(), "roles": a.principal.roles(), "projects": a.principal.projects})
}

async fn session_info(
    State(st): State<Arc<PortalState>>,
    Extension(inst): Extension<Installation>,
) -> Json<Value> {
    Json(json!({
        "installation": inst.0,
        "viewer": acting_json(&st.viewer),
        "approver": st.approver.as_ref().map(acting_json),
        "can_decide": st.approver.is_some(),
        "execution_mode": st.execution_mode,
        "api_version": "v1",
        "local_only": true,
        "authentication": "none (trusted local development only)",
    }))
}

/// What the portal may call. Everything else (token, principal, plugin, knowledge and skill
/// management, workflow start/cancel) stays with the CLI and the authenticated API.
fn allowed_api(method: &Method, path: &str) -> bool {
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.clone(), segs.as_slice()) {
        (Method::POST, ["v1", "approvals", _, "decision"]) => true,
        (Method::GET | Method::HEAD, s) => matches!(
            s,
            ["v1", "health"]
                | ["v1", "health", "details"]
                | ["v1", "me"]
                | ["v1", "dashboard"]
                | ["v1", "activity"]
                | ["v1", "approval-queue"]
                | ["v1", "workflow-queue"]
                | ["v1", "workflows"]
                | ["v1", "workflows", _]
                | ["v1", "workflows", _, "detail"]
                | ["v1", "workflow-definitions"]
                | ["v1", "approvals"]
                | ["v1", "approvals", _]
                | ["v1", "approvals", _, "document"]
                | ["v1", "artifacts", _]
                | ["v1", "projects"]
                | ["v1", "projects", _, "overview"]
                | ["v1", "audit"]
                | ["v1", "audit", "verify"]
                | ["v1", "traceability"]
                | ["v1", "agents"]
        ),
        _ => false,
    }
}

async fn inject_identity(
    State((st, h)): State<(Arc<PortalState>, Arc<Harness>)>,
    mut req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let path = path.strip_prefix("/api").unwrap_or(&path).to_string();
    if !allowed_api(req.method(), &path) {
        return err(
            StatusCode::FORBIDDEN,
            "not_available_in_portal",
            "this operation is not available through the portal; use the eaeh CLI",
        );
    }
    let decision = req.method() == Method::POST;
    let acting = if decision {
        match &st.approver {
            Some(a) => a,
            None => {
                return err(
                    StatusCode::FORBIDDEN,
                    "approver_not_configured",
                    "this portal is read-only: restart it with `--approver <profile>` to record decisions",
                );
            }
        }
    } else {
        &st.viewer
    };
    // Initial portal scope: workflow document gates only. Skills and standards are reviewed
    // with `eaeh approval review`, which shows their full content (the API itself is
    // unchanged and still accepts them from the CLI).
    if decision {
        let id = path
            .split('/')
            .nth(3)
            .and_then(|s| s.parse::<eaeh_core::ApprovalId>().ok());
        if let Some(id) = id
            && let Ok(rec) = h.approvals.get(id).await
            && rec.workflow_instance_id.is_none()
        {
            return err(
                StatusCode::FORBIDDEN,
                "not_available_in_portal",
                "the portal decides workflow document gates only; review skills and standards with `eaeh approval review`",
            );
        }
    }
    // Never forward a client-supplied credential.
    req.headers_mut().remove(header::AUTHORIZATION);
    match HeaderValue::from_str(&format!("Bearer {}", acting.token)) {
        Ok(v) => {
            req.headers_mut().insert(header::AUTHORIZATION, v);
        }
        Err(_) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "invalid stored credential",
            );
        }
    }
    if decision {
        tracing::info!(path = %path, approver = %acting.principal.id, "portal approval decision submitted");
    }
    next.run(req).await
}

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Built UI files; unknown paths fall back to `index.html` (client-side routing).
async fn static_file(Extension(st): Extension<Arc<PortalState>>, req: Request) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return err(StatusCode::NOT_FOUND, "not_found", "no such route");
    }
    let Some(dir) = &st.static_dir else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "The portal web UI is not built. Run `npm --prefix portal ci && npm --prefix portal run build`, then restart `eaeh portal`.",
        )
            .into_response();
    };
    let rel = req.uri().path().trim_start_matches('/');
    let safe = Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    let mut file = dir.join(if rel.is_empty() || !safe {
        "index.html"
    } else {
        rel
    });
    if !file.is_file() {
        file = dir.join("index.html");
    }
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let cache = if rel.starts_with("assets/") && file.ends_with(rel) {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                [
                    (header::CONTENT_TYPE, content_type(&file)),
                    (header::CACHE_CONTROL, cache),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => err(StatusCode::NOT_FOUND, "not_found", "file not found"),
    }
}
