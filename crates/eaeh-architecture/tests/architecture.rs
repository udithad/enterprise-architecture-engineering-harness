//! HLD/LLD validation, standards compliance, deterministic rendering and traceability
//! (AC3-07…AC3-10, AC3-14) on the labelled demonstration fixtures in `examples/design/`.

use eaeh_architecture::*;
use eaeh_requirements::{AnalysisConfig, AnalysisContext, BrdSection, RequirementSet, analyze};
use eaeh_validation::{Finding, Severity, ValidationSubject, Validator};
use serde_json::{Value, json};

const STD: &str = "demo-api-standards/error-format@v1";

fn load(name: &str) -> Value {
    let p = format!("{}/../../examples/design/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn reqs() -> RequirementSet {
    serde_json::from_value(load("requirements")).unwrap()
}

fn ctx() -> DesignContext {
    DesignContext {
        requirements: Some(reqs()),
        standards: vec![STD.into()],
        hld: Some(serde_json::from_value(load("hld")).unwrap()),
    }
}

fn errors(f: &[Finding]) -> Vec<String> {
    f.iter()
        .filter(|f| f.severity >= Severity::Error)
        .map(|f| format!("{}: {}", f.location.clone().unwrap_or_default(), f.message))
        .collect()
}

fn hld_err(v: &Value) -> Vec<String> {
    errors(&validate_hld(&serde_json::from_value(v.clone()).unwrap(), &ctx()))
}

fn lld_err(v: &Value) -> Vec<String> {
    errors(&validate_lld(&serde_json::from_value(v.clone()).unwrap(), &ctx()))
}

fn has(errs: &[String], needle: &str) -> bool {
    errs.iter().any(|e| e.contains(needle))
}

#[test]
fn demonstration_fixtures_pass_all_deterministic_checks() {
    // Requirements fixture against the 8 sections of the demonstration BRD.
    let sections = AnalysisContext {
        brd_sections: (0..8)
            .map(|i| BrdSection {
                citation: format!("demo-brd/order-management@v1#{i}"),
                heading: String::new(),
            })
            .collect(),
    };
    let a = analyze(&reqs(), &sections, &AnalysisConfig::default());
    let blocking: Vec<_> = a.blocking().map(|f| f.message.clone()).collect();
    assert!(blocking.is_empty(), "{blocking:?}");
    assert!(a.coverage.uncovered.is_empty());
    assert!(hld_err(&load("hld")).is_empty(), "{:?}", hld_err(&load("hld")));
    assert!(lld_err(&load("lld")).is_empty(), "{:?}", lld_err(&load("lld")));
}

#[test]
fn hld_integrity_coverage_decisions_and_risks_are_enforced() {
    let mut h = load("hld");
    h["interfaces"][0]["provider"] = json!("billing-service");
    h["interfaces"][2]["consumers"] = json!(["auditor"]);
    h["data_flows"][1]["to"] = json!("data-lake");
    h["deployment"][0]["hosts"] = json!(["web-shop", "ghost"]);
    h["components"][0]["requirements"] = json!(["REQ-F-099"]);
    h["components"][1]["requirements"] = json!(["REQ-F-002"]); // drops REQ-F-001, REQ-F-006, ...
    h["interfaces"][0]["requirements"] = json!([]);
    h["interfaces"][1]["requirements"] = json!([]);
    h["decisions"][2]["alternatives"] = json!([]);
    h["risks"][0]["mitigation"] = json!(" ");
    h["tradeoffs"][0]["choice"] = json!("Something else");
    h["components"][2]["id"] = json!("Order DB");
    let e = hld_err(&h);
    for n in [
        "provider 'billing-service' is not a component",
        "consumer 'auditor' is not a component or actor",
        "'data-lake' is not a component or actor",
        "hosted 'ghost' is not a component",
        "unknown requirement REQ-F-099",
        "Must requirement REQ-F-001 is not allocated",
        "Must requirement REQ-F-004 is not allocated", // REQ-F-006 stays allocated via D-002
        "no alternatives considered",
        "description and mitigation are required",
        "choice must be one of the options",
        "id must be a lower-case slug",
    ] {
        assert!(has(&e, n), "missing '{n}' in {e:#?}");
    }
    // Strict parsing: unknown fields are rejected.
    let mut u = load("hld");
    u["components"][0]["owner"] = json!("x");
    let f = hld_findings(&ValidationSubject {
        kind: "artifact".into(),
        name: "hld".into(),
        content: u,
        metadata: Default::default(),
    });
    assert!(f[0].message.contains("unknown field"), "{f:?}");
}

#[test]
fn mandatory_standards_must_be_addressed_and_deviations_need_an_adr() {
    let mut h = load("hld");
    h["compliance"] = json!([]);
    assert!(has(
        &hld_err(&h),
        &format!("mandatory standard {STD} is not addressed")
    ));

    let mut h = load("hld");
    h["compliance"][0]["status"] = json!("deviation");
    h["compliance"][0]["justification"] = json!("");
    let e = hld_err(&h);
    assert!(
        has(&e, "deviation needs a justification") && has(&e, "must be documented by a decision"),
        "{e:?}"
    );

    h["compliance"][0]["justification"] = json!("Legacy clients need plain-text errors until 2027");
    h["compliance"][0]["decision"] = json!("D-001");
    assert!(hld_err(&h).is_empty(), "{:?}", hld_err(&h));

    // Citing a standard that is not among the (validated) citations is refused.
    let mut h = load("hld");
    h["citations"] = json!([]);
    assert!(has(&hld_err(&h), "standard is not listed in 'citations'"));
}

#[test]
fn lld_must_refine_the_approved_hld_and_stay_consistent() {
    let mut l = load("lld");
    l["components"][0]["component"] = json!("mobile-app"); // not in HLD; web-shop no longer designed
    l["apis"][1]["path"] = json!("orders/{orderId}");
    l["apis"][2]["method"] = json!("FETCH");
    l["apis"][0]["response"] = json!("Receipt");
    l["data"][0]["relationships"][0]["target"] = json!("Invoice");
    l["sequences"][0]["steps"][0]["operation"] = json!("op-place-order"); // step target is web-shop
    l["sequences"][1]["steps"][0]["from"] = json!("courier");
    l["apis"][3]["requirements"] = json!([]);
    l["sequences"][1]["requirements"] = json!([]);
    l["decisions"][0]["requirements"] = json!(["REQ-F-003"]);
    let e = lld_err(&l);
    for n in [
        "'mobile-app' does not refine any component of the approved HLD",
        "HLD component 'web-shop' (web app) has no detailed design",
        "HTTP path must start with '/'",
        "method 'FETCH' must be one of",
        "payload 'Receipt' is not a defined entity",
        "relationship target 'Invoice' is not an entity",
        "operation 'op-place-order' belongs to 'order-service', not 'web-shop'",
        "participant 'courier' is not a component or actor",
        "Must requirement REQ-F-005 is not realised",
    ] {
        assert!(has(&e, n), "missing '{n}' in {e:#?}");
    }
}

#[test]
fn rendering_is_deterministic_and_complete() {
    let h: Hld = serde_json::from_value(load("hld")).unwrap();
    let l: Lld = serde_json::from_value(load("lld")).unwrap();
    let b1 = render_hld(&h);
    assert_eq!(b1, render_hld(&h));
    let paths: Vec<&str> = b1.files.keys().map(String::as_str).collect();
    assert_eq!(
        paths,
        vec![
            "adr/d-001-single-order-service-with-a-relational-store.md",
            "adr/d-002-delegate-card-handling-to-the-payment-provider.md",
            "adr/d-003-run-two-instances-across-two-zones.md",
            "diagrams/components.mmd",
            "diagrams/context.mmd",
            "diagrams/deployment.mmd",
            "hld.md",
        ]
    );
    let md = &b1.files["hld.md"];
    for s in [
        "## 8. Standards compliance",
        STD,
        "```mermaid\nflowchart LR",
        "| REQ-F-006 | D-002, interface I-002, order-service |",
    ] {
        assert!(md.contains(s), "hld.md lacks {s}");
    }
    assert!(
        b1.files["diagrams/components.mmd"]
            .contains("c_order_service -->|\"HTTPS (provider API)\"| c_payment_provider")
    );
    assert!(
        b1.files["adr/d-002-delegate-card-handling-to-the-payment-provider.md"]
            .contains("## Alternatives considered")
    );
    let b2 = render_lld(&l, Some(&h));
    assert_eq!(b2, render_lld(&l, Some(&h)));
    for p in [
        "lld.md",
        "diagrams/data-model.mmd",
        "diagrams/sequence-s-001.mmd",
        "diagrams/sequence-s-002.mmd",
        "adr/ld-001-optimistic-locking-for-status-transitions.md",
    ] {
        assert!(b2.files.contains_key(p), "missing {p}");
    }
    for path in b1.files.keys().chain(b2.files.keys()) {
        assert!(
            path.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '/')),
            "bundle path {path} is not a valid (lower-case) artifact name"
        );
    }
    assert!(b2.files["diagrams/data-model.mmd"].contains("E_Order ||--o{ E_OrderLine : \"contains\""));
    assert!(
        b2.files["diagrams/sequence-s-001.mmd"]
            .contains("p_web_shop->>p_order_service: POST /orders [op-place-order]")
    );
}

#[test]
fn model_text_cannot_inject_into_diagrams_or_documents() {
    let mut h = load("hld");
    h["context"]["system_name"] =
        json!("Shop\"]\n%%{init: {'theme':'dark'}}%%\nclick sys_system href \"javascript:alert(1)\"");
    h["components"][1]["name"] = json!("Order <script>alert(1)</script> | x");
    h["data_flows"][0]["data"] = json!("a|\"b\nclick c_web_shop call evil()");
    let b = render_hld(&serde_json::from_value(h).unwrap());
    for (path, body) in &b.files {
        for line in body.lines() {
            let t = line.trim_start();
            assert!(
                !t.starts_with("%%{") && !t.starts_with("click "),
                "{path}: injected line {line}"
            );
        }
        assert!(!body.contains("<script>"), "{path}: raw HTML");
    }
}

#[test]
fn trace_links_connect_requirements_to_design_elements() {
    let h: Hld = serde_json::from_value(load("hld")).unwrap();
    let l: Lld = serde_json::from_value(load("lld")).unwrap();
    let ht = hld_trace(&h);
    assert!(
        ht.iter()
            .any(|t| t.from_id == "REQ-F-006" && t.relation == "allocated_to" && t.to_id == "order-service")
    );
    assert!(
        ht.iter()
            .any(|t| t.from_id == "REQ-NF-001" && t.relation == "addressed_by" && t.to_id == "D-002")
    );
    let lt = lld_trace(&l);
    assert!(
        lt.iter()
            .any(|t| t.from_id == "REQ-F-001" && t.to_kind == "api_operation" && t.to_id == "op-place-order")
    );
    assert!(
        lt.iter()
            .any(|t| t.from_kind == "hld_component" && t.from_id == "web-shop" && t.relation == "refined_by")
    );
    let rt = eaeh_requirements::trace_links(&reqs());
    assert!(
        rt.iter()
            .any(|t| t.from_id == "demo-brd/order-management@v1#5" && t.to_id == "REQ-NF-001")
    );
}

#[tokio::test]
async fn validators_report_deterministically_with_context() {
    let subject = |doc: Value, ctx: Value| ValidationSubject {
        kind: "artifact".into(),
        name: "x".into(),
        content: json!({"output": doc, "context": ctx}),
        metadata: Default::default(),
    };
    let c = serde_json::to_value(ctx()).unwrap();
    let r = HldValidator.validate(&subject(load("hld"), c.clone())).await;
    assert!(errors(&r.findings).is_empty(), "{:?}", r.findings);
    assert!(HldValidator.applies_to(&subject(load("hld"), c.clone())));
    assert!(!HldValidator.applies_to(&subject(load("lld"), c.clone())));
    let r = LldValidator.validate(&subject(load("lld"), c)).await;
    assert!(errors(&r.findings).is_empty(), "{:?}", r.findings);
    // Malformed platform context fails closed.
    let r = LldValidator
        .validate(&subject(load("lld"), json!({"hld": 7})))
        .await;
    assert_eq!(r.findings[0].severity, Severity::Critical);
}
