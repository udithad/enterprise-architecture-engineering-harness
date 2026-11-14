//! Model management: routing, fallback, circuit breaking, budgets and usage recording (AC-21).

use async_trait::async_trait;
use eaeh_core::{
    ErrorKind, HarnessError, ModelId, ProjectId, ProviderId, Result, Scope, TenantId, TraceContext,
};
use eaeh_extensibility::HookRegistry;
use eaeh_governance::{BudgetLedger, BudgetLimits, BudgetResource};
use eaeh_intelligence::{
    Message, MessageRole, ModelCall, ModelClient, ModelDescriptor, ModelProvider, ModelRegistry,
    ModelRequest, ModelResponse, RoutingRequirements, Usage,
};
use eaeh_store::Db;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[derive(Clone, Copy)]
enum Mode {
    Ok,
    Unavailable,
    BadRequest,
}

struct Fake {
    local: bool,
    id: ProviderId,
    models: Vec<ModelDescriptor>,
    mode: Mode,
    calls: AtomicU32,
}

fn model(provider: &str, id: &str, cost: u64, quality: u8, caps: &[&str]) -> ModelDescriptor {
    ModelDescriptor {
        id: ModelId::new(id).unwrap(),
        provider: ProviderId::new(provider).unwrap(),
        version: format!("{id}-2026-01"),
        capabilities: caps.iter().map(|c| c.to_string()).collect(),
        context_window: 100_000,
        max_output_tokens: 4096,
        input_cost_per_mtok_micros: cost,
        output_cost_per_mtok_micros: cost,
        quality_tier: quality,
        residency: ["eu".to_string()].into(),
    }
}

#[async_trait]
impl ModelProvider for Fake {
    fn id(&self) -> &ProviderId {
        &self.id
    }
    fn models(&self) -> Vec<ModelDescriptor> {
        self.models.clone()
    }
    fn egress(&self) -> eaeh_intelligence::Egress {
        if self.local {
            eaeh_intelligence::Egress::Local
        } else {
            eaeh_intelligence::Egress::External
        }
    }
    async fn complete(&self, m: &ModelId, _r: &ModelRequest) -> Result<ModelResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            Mode::Ok => Ok(ModelResponse {
                content: format!("answer from {m}"),
                tool_calls: vec![],
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 50,
                },
                model_version: format!("{m}-2026-01"),
                finish_reason: "stop".into(),
            }),
            Mode::Unavailable => Err(HarnessError::unavailable("provider overloaded")),
            Mode::BadRequest => Err(HarnessError::invalid_input("bad request")),
        }
    }
}

fn fake(id: &str, models: Vec<ModelDescriptor>, mode: Mode) -> Arc<Fake> {
    Arc::new(Fake {
        local: false,
        id: ProviderId::new(id).unwrap(),
        models,
        mode,
        calls: AtomicU32::new(0),
    })
}

struct Fx {
    _d: tempfile::TempDir,
    db: Db,
    client: ModelClient,
    budgets: BudgetLedger,
    scope: Scope,
}

async fn fx(providers: Vec<Arc<Fake>>) -> Fx {
    let d = tempfile::tempdir().unwrap();
    let db = Db::open(&d.path().join("m.db")).await.unwrap();
    let reg = Arc::new(ModelRegistry::new(2, Duration::from_secs(60)));
    for p in providers {
        reg.register(p).unwrap();
    }
    let budgets = BudgetLedger::new(db.clone());
    budgets
        .init("wf", &BudgetLimits::platform_default())
        .await
        .unwrap();
    let client = ModelClient::new(
        db.clone(),
        reg,
        budgets.clone(),
        Arc::new(HookRegistry::new()),
        Arc::new(eaeh_governance::policy::RuleEngine::with_baseline().unwrap()),
        Duration::from_secs(5),
    );
    Fx {
        _d: d,
        db,
        client,
        budgets,
        scope: Scope::new(TenantId::new(), ProjectId::new()),
    }
}

fn call(f: &Fx, req: RoutingRequirements) -> ModelCall {
    ModelCall {
        scope: f.scope,
        trace: TraceContext::new_root(),
        agent_instance: None,
        budget_owners: vec!["wf".into()],
        requirements: req,
        request: ModelRequest {
            messages: vec![Message::new(MessageRole::User, "hi")],
            tools: vec![],
            max_output_tokens: 256,
            temperature: None,
        },
        prompt_refs: vec!["system.core@1.0.0".into()],
        context_snapshot: Some("abc".into()),
        max_classification: eaeh_core::Classification::Internal,
        skill_refs: vec!["skill.x@1.0.0".into()],
        knowledge_refs: vec!["src/doc@v1#0".into()],
    }
}

#[tokio::test]
async fn routes_to_cheapest_capable_model_not_most_expensive() {
    let f = fx(vec![fake(
        "p1",
        vec![
            model("p1", "big", 15_000_000, 9, &["tools", "reasoning"]),
            model("p1", "small", 250_000, 5, &["tools"]),
            model("p1", "tiny", 100_000, 2, &[]),
        ],
        Mode::Ok,
    )])
    .await;
    let reg = f.client.registry();
    let tools: RoutingRequirements = RoutingRequirements {
        capabilities: ["tools".to_string()].into(),
        ..Default::default()
    };
    assert_eq!(reg.route(&tools)[0].id.as_str(), "small");
    let reasoning = RoutingRequirements {
        capabilities: ["reasoning".to_string()].into(),
        ..Default::default()
    };
    assert_eq!(reg.route(&reasoning)[0].id.as_str(), "big");
    let preferred = RoutingRequirements {
        preferred: vec![ModelId::new("big").unwrap()],
        ..Default::default()
    };
    assert_eq!(reg.route(&preferred)[0].id.as_str(), "big");
    let us = RoutingRequirements {
        residency: Some("us".into()),
        ..Default::default()
    };
    assert!(reg.route(&us).is_empty());
    let out = f.client.complete(call(&f, tools)).await.unwrap();
    assert_eq!(out.model.id.as_str(), "small");
}

#[tokio::test]
async fn falls_back_on_retryable_failure_and_records_versions() {
    let bad = fake("aaa", vec![model("aaa", "cheap", 1, 5, &[])], Mode::Unavailable);
    let good = fake("bbb", vec![model("bbb", "pricier", 1000, 5, &[])], Mode::Ok);
    let f = fx(vec![bad.clone(), good.clone()]).await;
    let out = f
        .client
        .complete(call(&f, RoutingRequirements::default()))
        .await
        .unwrap();
    assert_eq!(out.model.id.as_str(), "pricier");
    assert_eq!(out.fallbacks.len(), 1);
    assert!(out.response.as_data().content.contains("pricier"));
    let rows: Vec<(String, String, String, i64, String)> = eaeh_store::sqlx::query_as(
        "SELECT provider_id, model_id, model_version, input_tokens, prompt_refs FROM model_usage",
    )
    .fetch_all(f.db.pool())
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "pricier-2026-01");
    assert!(rows[0].4.contains("system.core@1.0.0"));

    // Circuit breaker: after 2 failures provider 'aaa' is skipped without being called.
    f.client
        .complete(call(&f, RoutingRequirements::default()))
        .await
        .unwrap();
    let before = bad.calls.load(Ordering::SeqCst);
    f.client
        .complete(call(&f, RoutingRequirements::default()))
        .await
        .unwrap();
    assert_eq!(
        bad.calls.load(Ordering::SeqCst),
        before,
        "open circuit must skip provider"
    );
}

#[tokio::test]
async fn non_retryable_errors_do_not_fall_back() {
    let f = fx(vec![
        fake("aaa", vec![model("aaa", "m1", 1, 5, &[])], Mode::BadRequest),
        fake("bbb", vec![model("bbb", "m2", 2, 5, &[])], Mode::Ok),
    ])
    .await;
    let e = f
        .client
        .complete(call(&f, RoutingRequirements::default()))
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::InvalidInput);
}

#[tokio::test]
async fn all_failing_and_budget_limits() {
    let f = fx(vec![fake(
        "aaa",
        vec![model("aaa", "m1", 1, 5, &[])],
        Mode::Unavailable,
    )])
    .await;
    assert_eq!(
        f.client
            .complete(call(&f, RoutingRequirements::default()))
            .await
            .unwrap_err()
            .kind,
        ErrorKind::Unavailable
    );

    let f = fx(vec![fake("ok", vec![model("ok", "m", 1, 5, &[])], Mode::Ok)]).await;
    f.budgets
        .init(
            "tight",
            &BudgetLimits::default()
                .with(BudgetResource::ModelCalls, 10)
                .with(BudgetResource::Tokens, 200)
                .with(BudgetResource::CostMicros, 1_000_000),
        )
        .await
        .unwrap();
    let mut c = call(&f, RoutingRequirements::default());
    c.budget_owners = vec!["tight".into()];
    f.client.complete(c.clone()).await.unwrap(); // 150 tokens
    assert_eq!(
        f.client.complete(c.clone()).await.unwrap_err().kind,
        ErrorKind::BudgetExceeded
    );
    c.budget_owners = vec![];
    assert_eq!(
        f.client.complete(c).await.unwrap_err().kind,
        ErrorKind::BudgetExceeded
    );
}

#[tokio::test]
async fn cost_budget_is_enforced_independently_of_tokens() {
    // 150 tokens per call at 1_000_000 micros/Mtok in and out = 150 micros per call.
    let f = fx(vec![fake(
        "ok",
        vec![model("ok", "m", 1_000_000, 5, &[])],
        Mode::Ok,
    )])
    .await;
    f.budgets
        .init(
            "costly",
            &BudgetLimits::default()
                .with(BudgetResource::ModelCalls, 100)
                .with(BudgetResource::Tokens, 1_000_000)
                .with(BudgetResource::CostMicros, 200),
        )
        .await
        .unwrap();
    let mut c = call(&f, RoutingRequirements::default());
    c.budget_owners = vec!["costly".into()];
    f.client.complete(c.clone()).await.unwrap(); // 150 micros
    let e = f.client.complete(c).await.unwrap_err(); // would reach 300 > 200
    assert_eq!(e.kind, ErrorKind::BudgetExceeded);
    assert!(e.message.contains("cost_micros"), "{e}");
}

#[tokio::test]
async fn restricted_content_is_not_sent_to_external_providers() {
    let external = fake("ext", vec![model("ext", "cloud", 1, 5, &[])], Mode::Ok);
    let f = fx(vec![external.clone()]).await;
    let mut c = call(&f, RoutingRequirements::default());
    c.max_classification = eaeh_core::Classification::Restricted;
    let e = f.client.complete(c.clone()).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::PolicyDenied, "{e}");
    assert_eq!(
        external.calls.load(Ordering::SeqCst),
        0,
        "provider must not be called"
    );
    let denied: (i64,) = eaeh_store::sqlx::query_as(
        "SELECT COUNT(*) FROM audit_log WHERE action = 'model.invoke' AND outcome = 'denied' AND details LIKE '%restricted%'",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(denied.0, 1);
    // Confidential content may go to the external provider; usage records skill/knowledge refs.
    c.max_classification = eaeh_core::Classification::Confidential;
    f.client.complete(c).await.unwrap();
    let (skills, kb): (String, String) =
        eaeh_store::sqlx::query_as("SELECT skill_refs, knowledge_refs FROM model_usage")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert!(skills.contains("skill.x@1.0.0") && kb.contains("src/doc@v1#0"));

    // A local (on-host) provider may receive restricted content.
    let local = Arc::new(Fake {
        local: true,
        id: ProviderId::new("loc").unwrap(),
        models: vec![model("loc", "onhost", 1, 5, &[])],
        mode: Mode::Ok,
        calls: AtomicU32::new(0),
    });
    let f = fx(vec![]).await;
    f.client.registry().register(local.clone()).unwrap();
    let mut c = call(&f, RoutingRequirements::default());
    c.max_classification = eaeh_core::Classification::Restricted;
    f.client.complete(c).await.unwrap();
    assert_eq!(local.calls.load(Ordering::SeqCst), 1);
}
