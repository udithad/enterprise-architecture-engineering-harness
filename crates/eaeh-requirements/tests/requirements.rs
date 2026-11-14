//! Requirements model, structural validation and deterministic analysis (AC3-04…AC3-06).

use eaeh_requirements::*;
use eaeh_validation::{QualityGate, Severity, ValidationEngine, ValidationSubject};
use serde_json::{Value, json};
use std::sync::Arc;

const B: &str = "brd-orders/order-management@v1";

fn c(i: u32) -> String {
    format!("{B}#{i}")
}

/// A well-formed set covering BRD sections #0…#3.
fn good() -> Value {
    json!({
        "document": "requirements",
        "title": "Order management requirements",
        "summary": "Derived from the order management BRD.",
        "requirements": [
            {"id": "REQ-F-001", "kind": "functional", "title": "Place order", "statement": "A customer must be able to place an order for one or more products.",
             "priority": "must", "acceptance_criteria": ["Given a cart with items, when the customer checks out, then an order with status PLACED is created"],
             "sources": [c(1)], "business_rules": ["BR-001"], "concepts": ["Order"]},
            {"id": "REQ-F-002", "kind": "functional", "title": "Cancel order", "statement": "A customer must be able to cancel an order before it is shipped.",
             "priority": "should", "acceptance_criteria": ["An order in status SHIPPED cannot be cancelled"], "sources": [c(2)]},
            {"id": "REQ-NF-001", "kind": "non_functional", "category": "performance_efficiency", "title": "Order placement latency",
             "statement": "Order placement must complete within 2 seconds at the 95th percentile.", "priority": "must",
             "acceptance_criteria": ["p95 latency <= 2000 ms under 100 orders per second"], "sources": [c(3)]}
        ],
        "business_rules": [{"id": "BR-001", "statement": "An order must contain at least one line.", "sources": [c(1)]}],
        "domain_concepts": [{"name": "Order", "definition": "A customer's request to buy products.", "attributes": ["id", "status"], "sources": [c(1)]}],
        "issues": [],
        "out_of_scope": [{"source": c(0), "reason": "Document title and purpose only"}],
        "citations": [c(0), c(1), c(2), c(3)]
    })
}

fn sections(n: u32) -> AnalysisContext {
    AnalysisContext {
        brd_sections: (0..n)
            .map(|i| BrdSection {
                citation: c(i),
                heading: format!("Section {i}"),
            })
            .collect(),
    }
}

fn run(v: &Value, n: u32) -> Analysis {
    analyze(&parse(v).unwrap(), &sections(n), &AnalysisConfig::default())
}

fn errors(a: &Analysis) -> Vec<String> {
    a.findings
        .iter()
        .filter(|f| f.severity >= Severity::Error)
        .map(|f| f.message.clone())
        .collect()
}

#[test]
fn well_formed_set_has_no_blocking_findings_and_full_traceability() {
    let a = run(&good(), 4);
    assert!(errors(&a).is_empty(), "{:?}", errors(&a));
    assert_eq!(a.coverage.covered, vec![c(1), c(2), c(3)]);
    assert_eq!(a.coverage.excluded, vec![c(0)]);
    assert!(a.coverage.uncovered.is_empty());
    assert_eq!(a.trace[&c(1)], vec!["REQ-F-001", "BR-001"]);
    assert_eq!(a.metrics.functional, 2);
    assert_eq!(a.metrics.non_functional, 1);
    // Only one NFR category is present: the others are reported as missing (warnings).
    let missing: Vec<&Issue> = a.issues.iter().filter(|i| i.kind == IssueKind::Missing).collect();
    assert_eq!(missing.len(), 6);
    assert!(missing.iter().all(|i| i.origin == Origin::Analyzer));
    // Deterministic: identical input ⇒ identical analysis and rendering.
    let b = run(&good(), 4);
    assert_eq!(a, b);
    let set = parse(&good()).unwrap();
    assert_eq!(render_markdown(&set, &a), render_markdown(&set, &b));
}

#[test]
fn structural_defects_are_blocking() {
    let mut v = good();
    v["requirements"][0]["id"] = json!("REQ-NF-009"); // wrong prefix for functional
    v["requirements"][1]["id"] = json!("REQ-F-002");
    let dup = v["requirements"][1].clone();
    v["requirements"].as_array_mut().unwrap().push(dup); // duplicate id
    v["requirements"][2]["acceptance_criteria"] = json!([]); // Must without AC
    v["requirements"][2]["category"] = Value::Null; // NFR without category
    v["requirements"][0]["business_rules"] = json!(["BR-404"]);
    v["requirements"][0]["concepts"] = json!(["Invoice"]);
    v["requirements"][1]["sources"] = json!(["brd-orders/order-management@v1"]); // not a chunk
    v["requirements"][0]["sources"] = json!([format!("{B}#9")]); // not in citations
    let e = errors(&run(&v, 4)).join("\n");
    for needle in [
        "id must look like REQ-F-nnn",
        "duplicate id REQ-F-002",
        "Must requirement has no acceptance criteria",
        "needs a quality category",
        "unknown business rule BR-404",
        "unknown domain concept 'Invoice'",
        "is not a chunk citation",
        "is not listed in 'citations'",
        "is not traced to any section of the BRD",
    ] {
        assert!(e.contains(needle), "missing '{needle}' in:\n{e}");
    }
    // Unknown fields are rejected (strict parsing of model output).
    let mut u = good();
    u["requirements"][0]["owner"] = json!("someone");
    assert!(parse(&u).unwrap_err().message.contains("unknown field"));
}

#[test]
fn ambiguity_is_detected_and_acknowledged_items_are_downgraded() {
    let mut v = good();
    v["requirements"][1]["statement"] = json!("Cancellation should be fast and user-friendly, rules TBD.");
    v["requirements"][2]["statement"] = json!("Order placement must be efficient.");
    v["requirements"][2]["acceptance_criteria"] = json!(["Customers are satisfied with speed"]);
    let a = run(&v, 4);
    let amb: Vec<&Issue> = a
        .issues
        .iter()
        .filter(|i| i.kind == IssueKind::Ambiguity)
        .collect();
    let text = amb
        .iter()
        .map(|i| i.description.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("REQ-F-002 contains unresolved marker 'tbd'"),
        "{text}"
    );
    assert!(
        text.contains("'fast'") && text.contains("'user friendly'"),
        "{text}"
    );
    assert!(text.contains("REQ-NF-001 is not measurable"), "{text}");
    assert!(text.contains("'efficient'"), "{text}");
    assert!(errors(&a).iter().any(|e| e.contains("unresolved marker")));

    // The analyst raised the open point as an issue: the TBD becomes informational.
    v["issues"] = json!([{"id": "ISS-001", "kind": "ambiguity", "severity": "high",
        "description": "Cancellation rules are not defined in the BRD", "refs": ["REQ-F-002"],
        "question": "Until which fulfilment state may an order be cancelled?"}]);
    let a = run(&v, 4);
    assert!(
        !errors(&a).iter().any(|e| e.contains("unresolved marker")),
        "{:?}",
        errors(&a)
    );
    let tbd = a
        .findings
        .iter()
        .find(|f| f.message.contains("unresolved marker"))
        .unwrap();
    assert_eq!(tbd.severity, Severity::Info);
    assert!(tbd.message.contains("acknowledged in ISS-001"));
}

#[test]
fn conflicts_duplicates_and_numeric_conflicts_are_detected() {
    let mut v = good();
    let reqs = v["requirements"].as_array_mut().unwrap();
    reqs.push(
        json!({"id": "REQ-F-003", "kind": "functional", "title": "No cancel",
        "statement": "A customer must not be able to cancel an order before it is shipped.",
        "priority": "should", "acceptance_criteria": ["x"], "sources": [c(2)]}),
    );
    reqs.push(
        json!({"id": "REQ-F-004", "kind": "functional", "title": "Place order again",
        "statement": "A customer must be able to place an order for one or more products.",
        "priority": "could", "sources": [c(1)]}),
    );
    reqs.push(json!({"id": "REQ-NF-002", "kind": "non_functional", "category": "performance_efficiency",
        "title": "Placement latency", "statement": "Order placement must complete within 500 ms at the 95th percentile.",
        "priority": "should", "acceptance_criteria": ["p95 <= 500 ms"], "sources": [c(3)]}));
    let a = run(&v, 4);
    let find = |k: IssueKind, a1: &str, b1: &str| {
        a.issues
            .iter()
            .any(|i| i.kind == k && i.refs == vec![a1.to_string(), b1.to_string()])
    };
    assert!(
        find(IssueKind::Conflict, "REQ-F-002", "REQ-F-003"),
        "{:#?}",
        a.issues
    );
    assert!(
        find(IssueKind::Duplicate, "REQ-F-001", "REQ-F-004"),
        "{:#?}",
        a.issues
    );
    assert!(
        find(IssueKind::Conflict, "REQ-NF-001", "REQ-NF-002"),
        "{:#?}",
        a.issues
    );
    // Contradictions block; duplicates and numeric conflicts are warnings for review.
    assert!(errors(&a).iter().any(|e| e.contains("contradictory obligations")));
    assert!(
        !errors(&a)
            .iter()
            .any(|e| e.contains("duplicates") || e.contains("different targets"))
    );
}

#[test]
fn uncovered_brd_sections_are_missing_requirements() {
    let a = run(&good(), 6);
    assert_eq!(a.coverage.uncovered, vec![c(4), c(5)]);
    let e = errors(&a);
    assert_eq!(
        e.iter()
            .filter(|m| m.contains("has no derived requirement"))
            .count(),
        2,
        "{e:?}"
    );
}

#[tokio::test]
async fn validator_runs_in_a_fail_closed_quality_gate() {
    let engine = ValidationEngine::new();
    engine
        .register(Arc::new(RequirementsValidator::default()))
        .unwrap();
    let gate = QualityGate {
        id: "requirements".into(),
        validators: vec!["requirements.set".into()],
        block_on: Severity::Error,
        semantic_can_block: false,
    };
    let subj = |content: Value| ValidationSubject {
        kind: "artifact".into(),
        name: "requirements".into(),
        content,
        metadata: Default::default(),
    };
    let ctx = serde_json::to_value(sections(4)).unwrap();
    let ok = engine
        .run_gate(&gate, &subj(json!({"output": good(), "context": ctx})))
        .await;
    assert!(ok.passed, "{:?}", ok.blocking);
    let bad = engine
        .run_gate(
            &gate,
            &subj(json!({"output": good(), "context": serde_json::to_value(sections(5)).unwrap()})),
        )
        .await;
    assert!(!bad.passed);
    // Something that is not a requirements document: the validator does not apply, so no
    // deterministic validator ran and the gate fails closed.
    let other = engine
        .run_gate(&gate, &subj(json!({"output": {"document": "hld"}})))
        .await;
    assert!(!other.passed);
}
