//! Design tools invoked directly (they are also exercised through the governed gateway in
//! `tests/tests/design_demo.rs`): `design.check`, `requirements.render`,
//! `architecture.render`, validator registration and default gates.

use eaeh_core::{
    AgentDefinitionId, AgentInstanceId, AutonomyLevel, InvocationId, Principal, ProjectId, Scope, TenantId,
    TraceContext,
};
use eaeh_engineering::*;
use eaeh_requirements::AnalysisConfig;
use eaeh_tools::{Tool, ToolContext};
use eaeh_validation::ValidationEngine;
use serde_json::{Value, json};
use std::sync::Arc;

fn example(name: &str) -> Value {
    let p = format!("{}/../../examples/design/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn ctx() -> ToolContext {
    let scope = Scope::new(TenantId::new(), ProjectId::new());
    ToolContext {
        invocation_id: InvocationId::new(),
        principal: Principal::agent(
            AgentInstanceId::new(),
            AgentDefinitionId::new("solution-architect").unwrap(),
            "1.0.0".into(),
            AutonomyLevel::ReadOnly,
            scope,
        ),
        scope,
        workspace: None,
        provider: None,
        trace: TraceContext::new_root(),
    }
}

fn engine() -> Arc<ValidationEngine> {
    let e = Arc::new(ValidationEngine::new());
    register_validators(&e, AnalysisConfig::default()).unwrap();
    e
}

#[tokio::test]
async fn design_check_reports_findings_for_drafts() {
    let tool = DesignCheckTool::new(engine()).unwrap();
    let reqs = example("requirements");
    let ok = tool
        .invoke(&ctx(), json!({"document": example("hld"), "context": {"requirements": reqs, "standards": ["demo-api-standards/error-format@v1"]}}))
        .await
        .unwrap();
    assert_eq!(ok["passed"], true, "{ok}");
    let mut bad = example("hld");
    bad["compliance"] = json!([]);
    let r = tool
        .invoke(&ctx(), json!({"document": bad, "context": {"requirements": example("requirements"), "standards": ["demo-api-standards/error-format@v1"]}}))
        .await
        .unwrap();
    assert_eq!(r["passed"], false);
    assert!(r["findings"].to_string().contains("is not addressed"), "{r}");
    assert!(
        tool.invoke(&ctx(), json!({"document": {"document": "memo"}}))
            .await
            .is_err()
    );
    // Gates referenced by the shipped workflows exist and block on errors.
    let ids: Vec<String> = default_gates().into_iter().map(|g| g.id).collect();
    assert_eq!(
        ids,
        vec!["requirements", "hld", "lld", "delivery-plan", "delivery", "build"]
    );
}

#[tokio::test]
async fn render_tools_produce_bundles_and_trace_links() {
    let t = RequirementsRenderTool::new(AnalysisConfig::default()).unwrap();
    let sections: Vec<Value> = (0..8)
        .map(|i| json!({"citation": format!("demo-brd/order-management@v1#{i}"), "heading": ""}))
        .collect();
    let out = t
        .invoke(
            &ctx(),
            json!({"requirements": example("requirements"), "brd": {"sections": sections}}),
        )
        .await
        .unwrap();
    let files: Vec<&String> = out["files"].as_object().unwrap().keys().collect();
    assert_eq!(
        files,
        vec!["analysis.json", "requirements.json", "requirements.md"]
    );
    assert_eq!(out["summary"]["blocking"], 0);
    assert!(
        out["trace"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["to_id"] == "REQ-NF-001")
    );
    assert!(
        t.invoke(&ctx(), json!({"requirements": {"document": "requirements"}}))
            .await
            .is_err()
    );

    let a = ArchitectureRenderTool::new().unwrap();
    let h = a.invoke(&ctx(), json!({"model": example("hld")})).await.unwrap();
    assert_eq!(h["document"], "hld");
    assert!(h["files"]["hld.json"].is_string() && h["files"]["diagrams/context.mmd"].is_string());
    let l = a
        .invoke(&ctx(), json!({"model": example("lld"), "hld": example("hld")}))
        .await
        .unwrap();
    assert!(
        l["files"]["lld.md"]
            .as_str()
            .unwrap()
            .contains("Web shop (web-shop)"),
        "HLD names resolved"
    );
    assert!(
        a.invoke(&ctx(), json!({"model": {"document": "hld", "title": 1}}))
            .await
            .is_err()
    );
    assert!(
        a.invoke(&ctx(), json!({"model": {"document": "spec"}}))
            .await
            .is_err()
    );
}
