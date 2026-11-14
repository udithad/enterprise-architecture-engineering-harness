//! Deterministic Markdown rendering of a requirements set and its analysis.

use crate::analysis::Analysis;
use crate::model::*;
use std::fmt::Write;

/// Escape text for a Markdown table cell (no raw HTML, no row breaks).
pub fn cell(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace(['\r', '\n'], " ")
}

/// Escape text for a Markdown paragraph (no raw HTML).
pub fn text(s: &str) -> String {
    s.replace('<', "&lt;").replace('>', "&gt;")
}

pub fn render_markdown(set: &RequirementSet, analysis: &Analysis) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "# {}\n", cell(&set.title));
    if !set.summary.trim().is_empty() {
        let _ = writeln!(o, "{}\n", text(&set.summary));
    }
    let m = &analysis.metrics;
    let _ = writeln!(
        o,
        "| Functional | Non-functional | Business rules | Domain concepts | Must | With acceptance criteria | Open issues |\n|---|---|---|---|---|---|---|\n| {} | {} | {} | {} | {} | {} | {} |\n",
        m.functional,
        m.non_functional,
        m.business_rules,
        m.domain_concepts,
        m.must,
        m.with_acceptance_criteria,
        set.issues.len() + analysis.issues.len()
    );
    for (title, kind) in [
        ("Functional requirements", RequirementKind::Functional),
        ("Non-functional requirements", RequirementKind::NonFunctional),
    ] {
        let rs: Vec<&Requirement> = set.requirements.iter().filter(|r| r.kind == kind).collect();
        if rs.is_empty() {
            continue;
        }
        let _ = writeln!(o, "## {title}\n");
        for r in rs {
            let _ = writeln!(o, "### {} – {}\n", r.id, cell(&r.title));
            let _ = writeln!(
                o,
                "* **Priority:** {}{}",
                r.priority.label(),
                r.category
                    .as_deref()
                    .map(|c| format!(" · **Category:** {}", cell(c)))
                    .unwrap_or_default()
            );
            let _ = writeln!(o, "* **Statement:** {}", text(&r.statement));
            if !r.rationale.trim().is_empty() {
                let _ = writeln!(o, "* **Rationale:** {}", text(&r.rationale));
            }
            if !r.business_rules.is_empty() {
                let _ = writeln!(o, "* **Business rules:** {}", r.business_rules.join(", "));
            }
            if !r.concepts.is_empty() {
                let _ = writeln!(o, "* **Concepts:** {}", cell(&r.concepts.join(", ")));
            }
            let _ = writeln!(o, "* **Sources:** {}", r.sources.join(", "));
            if !r.acceptance_criteria.is_empty() {
                let _ = writeln!(o, "* **Acceptance criteria:**");
                for ac in &r.acceptance_criteria {
                    let _ = writeln!(o, "  * {}", text(ac));
                }
            }
            o.push('\n');
        }
    }
    if !set.business_rules.is_empty() {
        let _ = writeln!(o, "## Business rules\n\n| Id | Rule | Sources |\n|---|---|---|");
        for b in &set.business_rules {
            let _ = writeln!(
                o,
                "| {} | {} | {} |",
                b.id,
                cell(&b.statement),
                b.sources.join(", ")
            );
        }
        o.push('\n');
    }
    if !set.domain_concepts.is_empty() {
        let _ = writeln!(
            o,
            "## Domain concepts\n\n| Concept | Definition | Attributes | Relationships |\n|---|---|---|---|"
        );
        for c in &set.domain_concepts {
            let _ = writeln!(
                o,
                "| {} | {} | {} | {} |",
                cell(&c.name),
                cell(&c.definition),
                cell(&c.attributes.join(", ")),
                cell(&c.relationships.join("; "))
            );
        }
        o.push('\n');
    }
    let _ = writeln!(
        o,
        "## Issues\n\n| Id | Origin | Kind | Severity | Description | Refs | Question |\n|---|---|---|---|---|---|---|"
    );
    let all = set.issues.iter().chain(analysis.issues.iter());
    for i in all {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {:?} | {} | {} | {} |",
            i.id,
            match i.origin {
                Origin::Agent => "analyst",
                Origin::Analyzer => "analyzer",
            },
            i.kind.label(),
            i.severity,
            cell(&i.description),
            cell(&i.refs.join(", ")),
            cell(&i.question)
        );
    }
    o.push('\n');
    if !set.out_of_scope.is_empty() {
        let _ = writeln!(o, "## Out of scope\n\n| BRD section | Reason |\n|---|---|");
        for x in &set.out_of_scope {
            let _ = writeln!(o, "| {} | {} |", x.source, cell(&x.reason));
        }
        o.push('\n');
    }
    let _ = writeln!(o, "## Traceability (BRD → requirements)\n");
    let c = &analysis.coverage;
    if c.brd_sections > 0 {
        let _ = writeln!(
            o,
            "BRD sections: {} · covered: {} · excluded: {} · uncovered: {}\n",
            c.brd_sections,
            c.covered.len(),
            c.excluded.len(),
            c.uncovered.len()
        );
    }
    let _ = writeln!(o, "| BRD section | Derived items |\n|---|---|");
    for (src, ids) in &analysis.trace {
        let _ = writeln!(o, "| {} | {} |", src, ids.join(", "));
    }
    o
}
