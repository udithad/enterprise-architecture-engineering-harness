//! Adapter verification against a local mock of the Messages API (AC2-16, AC2-17).
//! No live API call is made by these tests.

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use eaeh_core::{ErrorKind, ModelId, SecretString};
use eaeh_intelligence::{Message, MessageRole, ModelProvider, ModelRequest, ToolSpec};
use eaeh_provider_anthropic::{AnthropicConfig, AnthropicProvider};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// (status, headers, JSON body) of one scripted mock response.
type Scripted = (u16, Vec<(String, String)>, Value);

const KEY: &str = "sk-ant-test-key-should-never-leak-123";

#[derive(Default)]
struct Mock {
    responses: Mutex<VecDeque<Scripted>>,
    seen: Mutex<Vec<(HeaderMap, Value)>>,
}

async fn handler(
    State(m): State<Arc<Mock>>,
    headers: HeaderMap,
    body: axum::Json<Value>,
) -> impl IntoResponse {
    m.seen.lock().unwrap().push((headers, body.0));
    let (status, hdrs, body) = m.responses.lock().unwrap().pop_front().unwrap_or((
        500,
        vec![],
        json!({"error": {"message": "no script"}}),
    ));
    let mut res = (StatusCode::from_u16(status).unwrap(), axum::Json(body)).into_response();
    for (k, v) in hdrs {
        res.headers_mut().insert(
            axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
            v.parse().unwrap(),
        );
    }
    res
}

async fn server(responses: Vec<Scripted>) -> (Arc<Mock>, String) {
    let mock = Arc::new(Mock {
        responses: Mutex::new(responses.into()),
        seen: Mutex::new(vec![]),
    });
    let app = Router::new()
        .route("/v1/messages", post(handler))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (mock, format!("http://127.0.0.1:{}", addr.port()))
}

fn provider(base: &str) -> AnthropicProvider {
    let cfg = AnthropicConfig {
        enabled: true,
        base_url: base.into(),
        max_retries: 2,
        ..Default::default()
    };
    AnthropicProvider::new(cfg, SecretString::new(KEY)).unwrap()
}

fn request() -> ModelRequest {
    ModelRequest {
        messages: vec![
            Message::new(MessageRole::System, "You are an EAEH agent."),
            Message::new(MessageRole::User, "Review the spec."),
        ],
        tools: vec![ToolSpec {
            name: "knowledge.search".into(),
            description: "search".into(),
            input_schema: json!({"type": "object"}),
        }],
        max_output_tokens: 1024,
        temperature: Some(0.7),
    }
}

fn ok_tool_use() -> Value {
    json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
        "content": [
            {"type": "thinking", "thinking": ""},
            {"type": "text", "text": "Searching."},
            {"type": "tool_use", "id": "tu_1", "name": "knowledge_search", "input": {"query": "error format"}}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 120, "output_tokens": 30}
    })
}

#[tokio::test]
async fn wire_format_headers_and_tool_round_trip() {
    let (mock, base) = server(vec![(200, vec![], ok_tool_use())]).await;
    let p = provider(&base);
    let r = p
        .complete(&ModelId::new("claude-opus-5-5").unwrap(), &request())
        .await
        .unwrap();
    assert_eq!(r.content, "Searching.");
    assert_eq!(
        r.tool_calls[0].tool, "knowledge.search",
        "tool name mapped back to the platform id"
    );
    assert_eq!(r.tool_calls[0].input["query"], "error format");
    assert_eq!((r.usage.input_tokens, r.usage.output_tokens), (120, 30));
    assert_eq!(r.model_version, "claude-opus-5-5");
    let seen = mock.seen.lock().unwrap();
    let (h, body) = &seen[0];
    assert_eq!(h["x-api-key"], KEY);
    assert_eq!(h["anthropic-version"], "2026-10-21");
    assert_eq!(h["anthropic-beta"], "server-side-fallback-2026-07-01");
    assert_eq!(body["model"], "claude-opus-5-5");
    assert_eq!(body["system"], "You are an EAEH agent.");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["tools"][0]["name"], "knowledge_search");
    assert_eq!(body["fallbacks"], "default");
    for forbidden in ["temperature", "top_p", "top_k", "tool_choice"] {
        assert!(body.get(forbidden).is_none(), "{forbidden} must not be sent");
    }
    assert!(!body.to_string().contains(KEY), "credentials never in the body");
}

#[tokio::test]
async fn retries_429_with_retry_after_then_succeeds() {
    let (mock, base) = server(vec![
        (
            429,
            vec![("retry-after".into(), "0".into())],
            json!({"error": {"type": "rate_limit_error", "message": "slow down"}}),
        ),
        (
            529,
            vec![],
            json!({"error": {"type": "overloaded_error", "message": "overloaded"}}),
        ),
        (200, vec![], ok_tool_use()),
    ])
    .await;
    let p = provider(&base);
    p.complete(&ModelId::new("claude-opus-5-5").unwrap(), &request())
        .await
        .unwrap();
    assert_eq!(mock.seen.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn error_mapping_and_no_credential_leakage() {
    let model = ModelId::new("claude-opus-5-5").unwrap();
    let cases = [
        (400, ErrorKind::InvalidInput),
        (401, ErrorKind::Forbidden),
        (403, ErrorKind::Forbidden),
        (404, ErrorKind::InvalidInput),
    ];
    for (status, kind) in cases {
        let (_m, base) = server(vec![(
            status,
            vec![],
            json!({"error": {"message": format!("status {status}")}}),
        )])
        .await;
        let e = provider(&base).complete(&model, &request()).await.unwrap_err();
        assert_eq!(e.kind, kind, "{status}");
        assert!(!e.message.contains(KEY));
    }
    // Persistent 5xx: retried, then `unavailable` (retryable ⇒ ModelClient fallback).
    let (m, base) = server(vec![]).await;
    let e = provider(&base).complete(&model, &request()).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unavailable);
    assert!(e.is_retryable());
    assert_eq!(m.seen.lock().unwrap().len(), 3, "1 attempt + 2 retries");
    // Refusal is a non-retryable policy decision.
    let (_m, base) = server(vec![(200, vec![], json!({"model": "claude-opus-5-5", "content": [], "stop_reason": "refusal", "stop_details": {"type": "refusal", "category": "cyber"}, "usage": {"input_tokens": 5, "output_tokens": 0}}))]).await;
    let e = provider(&base).complete(&model, &request()).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::PolicyDenied);
    assert!(e.message.contains("cyber"));
    // Debug output redacts the key.
    assert!(!format!("{:?}", provider(&base)).contains(KEY));
}

#[test]
fn configuration_is_validated() {
    let key = SecretString::new(KEY);
    assert!(
        AnthropicProvider::new(
            AnthropicConfig {
                base_url: "http://evil.example".into(),
                ..Default::default()
            },
            key.clone()
        )
        .is_err(),
        "https required"
    );
    assert!(AnthropicProvider::new(AnthropicConfig::default(), SecretString::new("  ")).is_err());
    assert!(
        AnthropicProvider::new(
            AnthropicConfig {
                effort: Some("extreme".into()),
                ..Default::default()
            },
            key.clone()
        )
        .is_err()
    );
    let p = AnthropicProvider::new(AnthropicConfig::default(), key).unwrap();
    let models = p.models();
    assert_eq!(models[0].id.as_str(), "claude-opus-5-5");
    assert_eq!(models[0].input_cost_per_mtok_micros, 4_000_000);
    assert_eq!(p.egress(), eaeh_intelligence::Egress::External);
}
