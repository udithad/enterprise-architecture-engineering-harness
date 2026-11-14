//! Analyzer quality baseline (AC3-05) on a fixed, labelled fixture set. The labels were
//! written by the implementer (not an independent panel) and deliberately include cases the
//! heuristics are expected to miss ("timely", "large", a semantic conflict) or to flag
//! wrongly ("fast-track"). Results are printed as `ANALYZER-EVAL` lines and recorded in
//! `docs/implementation/evidence/phase-03-analyzer-eval.log`; thresholds guard regressions.

use eaeh_requirements::*;
use serde_json::json;

fn req(id: &str, nf: Option<&str>, statement: &str) -> serde_json::Value {
    json!({"id": id, "kind": if nf.is_some() {"non_functional"} else {"functional"}, "category": nf,
           "title": id, "statement": statement, "priority": "could", "sources": ["b/x@v1#0"]})
}

fn set(reqs: Vec<serde_json::Value>) -> RequirementSet {
    serde_json::from_value(
        json!({"document": "requirements", "title": "eval", "requirements": reqs, "citations": ["b/x@v1#0"]}),
    )
    .unwrap()
}

fn cfg() -> AnalysisConfig {
    AnalysisConfig {
        nfr_categories: vec![],
        ..Default::default()
    }
}

#[test]
fn analyzer_precision_recall_baseline() {
    // (statement, NFR category, labelled ambiguous?)
    let singles: &[(&str, Option<&str>, bool)] = &[
        ("The system must be user-friendly for all customers.", None, true),
        ("Search results should load quickly.", None, true),
        ("The service must be fast.", Some("performance_efficiency"), true),
        ("The admin can export reports in PDF, CSV, etc.", None, true),
        ("Discount rules are TBD.", None, true),
        ("Passwords must be stored securely.", Some("security"), true),
        ("The UI should be intuitive.", None, true),
        ("Notifications are sent as appropriate.", None, true),
        ("The checkout must be easy to use.", Some("usability"), true),
        ("The system should support approximately 1000 users.", None, true),
        (
            "Users must be notified when the order is shipped, if possible.",
            None,
            true,
        ),
        (
            "The platform must be highly available.",
            Some("reliability"),
            true,
        ),
        ("The report must be generated in a timely manner.", None, true),
        ("The system must handle large files.", None, true),
        (
            "A customer must be able to cancel an order before it is shipped.",
            None,
            false,
        ),
        (
            "The order confirmation e-mail must contain the order number.",
            None,
            false,
        ),
        (
            "Search must return results within 300 ms at the 95th percentile.",
            Some("performance_efficiency"),
            false,
        ),
        (
            "The service must be available 99.9% of each calendar month.",
            Some("reliability"),
            false,
        ),
        ("An order must contain at least one line.", None, false),
        ("All traffic must use TLS 1.3 or later.", Some("security"), false),
        (
            "Warehouse staff must be able to mark an order as shipped.",
            None,
            false,
        ),
        (
            "The system must provide a fast-track lane for VIP customers.",
            None,
            false,
        ),
        ("Payments are captured by the external provider.", None, false),
        (
            "Each service must have at least 80% unit-test line coverage.",
            Some("maintainability"),
            false,
        ),
    ];
    let (mut tp, mut fp, mut fneg, mut tn) = (0, 0, 0, 0);
    for (i, (s, nf, label)) in singles.iter().enumerate() {
        let id = format!("{}-{:03}", if nf.is_some() { "REQ-NF" } else { "REQ-F" }, i + 1);
        let a = analyze(&set(vec![req(&id, *nf, s)]), &AnalysisContext::default(), &cfg());
        let predicted = a.issues.iter().any(|x| x.kind == IssueKind::Ambiguity);
        match (predicted, *label) {
            (true, true) => tp += 1,
            (true, false) => fp += 1,
            (false, true) => fneg += 1,
            (false, false) => tn += 1,
        }
        if predicted != *label {
            println!("ANALYZER-EVAL miss ambiguity predicted={predicted} label={label}: {s}");
        }
    }
    let p = tp as f64 / (tp + fp) as f64;
    let r = tp as f64 / (tp + fneg) as f64;
    println!(
        "ANALYZER-EVAL ambiguity n={} tp={tp} fp={fp} fn={fneg} tn={tn} precision={p:.3} recall={r:.3}",
        singles.len()
    );

    // (a, b, same NFR category, expected: "conflict" | "duplicate" | "none")
    let pairs: &[(&str, &str, Option<&str>, &str)] = &[
        (
            "A customer must be able to cancel an order before it is shipped.",
            "A customer must not be able to cancel an order before it is shipped.",
            None,
            "conflict",
        ),
        (
            "Search must return results within 300 ms at the 95th percentile.",
            "Search must return results within 1 second at the 95th percentile.",
            Some("performance_efficiency"),
            "conflict",
        ),
        (
            "The order confirmation e-mail must contain the order number.",
            "The order confirmation e-mail must contain the order number.",
            None,
            "duplicate",
        ),
        (
            "The confirmation e-mail must contain the order number.",
            "The order confirmation e-mail shall contain the order number.",
            None,
            "duplicate",
        ),
        (
            "Guests may check out without an account.",
            "Customers must register before placing an order.",
            None,
            "conflict",
        ),
        (
            "A customer must be able to cancel an order.",
            "Warehouse staff must be able to mark an order as shipped.",
            None,
            "none",
        ),
        (
            "Search must return results within 300 ms.",
            "The service must be available 99.9% of each month.",
            None,
            "none",
        ),
        (
            "The platform must not store card numbers.",
            "The platform must store order history for 7 years.",
            None,
            "none",
        ),
    ];
    let mut correct = 0;
    for (a1, b1, nf, expected) in pairs {
        let (ka, kb) = if nf.is_some() {
            ("REQ-NF-001", "REQ-NF-002")
        } else {
            ("REQ-F-001", "REQ-F-002")
        };
        let a = analyze(
            &set(vec![req(ka, *nf, a1), req(kb, *nf, b1)]),
            &AnalysisContext::default(),
            &cfg(),
        );
        let got = if a.issues.iter().any(|x| x.kind == IssueKind::Conflict) {
            "conflict"
        } else if a.issues.iter().any(|x| x.kind == IssueKind::Duplicate) {
            "duplicate"
        } else {
            "none"
        };
        if got == *expected {
            correct += 1;
        } else {
            println!("ANALYZER-EVAL miss pair expected={expected} got={got}: '{a1}' / '{b1}'");
        }
    }
    let acc = correct as f64 / pairs.len() as f64;
    println!(
        "ANALYZER-EVAL pairs n={} correct={correct} accuracy={acc:.3}",
        pairs.len()
    );

    // Regression guards (the measured baseline is recorded in the evidence log).
    assert!(p >= 0.9, "ambiguity precision regressed: {p}");
    assert!(r >= 0.8, "ambiguity recall regressed: {r}");
    assert!(acc >= 0.85, "pair accuracy regressed: {acc}");
}
