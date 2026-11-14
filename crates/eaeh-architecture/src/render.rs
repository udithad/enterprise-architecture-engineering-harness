//! Deterministic document bundles: HLD/LLD Markdown with embedded Mermaid diagrams,
//! standalone `.mmd` diagram files and ADRs (one per decision). Identical models produce
//! byte-identical bundles.

use crate::mermaid;
use crate::model::*;
use eaeh_requirements::TraceLink;
use eaeh_requirements::render::{cell, text};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// Rendered files keyed by relative path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub files: BTreeMap<String, String>,
}

pub fn slug(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            o.push(c.to_ascii_lowercase());
        } else if !o.ends_with('-') && !o.is_empty() {
            o.push('-');
        }
        if o.len() >= 48 {
            break;
        }
    }
    o.trim_end_matches('-').to_string()
}

fn mermaid_block(o: &mut String, title: &str, src: &str) {
    let _ = writeln!(o, "### {title}\n\n```mermaid\n{src}```\n");
}

pub fn adr(d: &Decision, scope: &str) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "# {}: {}\n", d.id, cell(&d.title));
    let _ = writeln!(
        o,
        "* **Status:** {}\n* **Scope:** {scope}",
        match d.status {
            DecisionStatus::Proposed => "Proposed",
            DecisionStatus::Accepted => "Accepted",
        }
    );
    if !d.requirements.is_empty() {
        let _ = writeln!(o, "* **Requirements:** {}", d.requirements.join(", "));
    }
    if !d.standards.is_empty() {
        let _ = writeln!(o, "* **Standards:** {}", d.standards.join(", "));
    }
    let _ = writeln!(o, "\n## Context\n\n{}\n", text(&d.context));
    let _ = writeln!(o, "## Decision\n\n{}\n", text(&d.decision));
    let _ = writeln!(o, "## Alternatives considered\n");
    for a in &d.alternatives {
        let _ = writeln!(o, "### {}\n", cell(&a.option));
        for p in &a.pros {
            let _ = writeln!(o, "* ➕ {}", text(p));
        }
        for c in &a.cons {
            let _ = writeln!(o, "* ➖ {}", text(c));
        }
        o.push('\n');
    }
    if !d.consequences.is_empty() {
        let _ = writeln!(o, "## Consequences\n");
        for c in &d.consequences {
            let _ = writeln!(o, "* {}", text(c));
        }
    }
    o
}

/// Bundle paths are lower-case (artifact names are lower-case by platform rule).
fn adr_path(d: &Decision) -> String {
    format!("adr/{}-{}.md", slug(&d.id), slug(&d.title))
}

fn risks_table(o: &mut String, risks: &[Risk]) {
    let _ = writeln!(
        o,
        "| Id | Risk | Likelihood | Impact | Mitigation | Components |\n|---|---|---|---|---|---|"
    );
    for r in risks {
        let _ = writeln!(
            o,
            "| {} | {} | {:?} | {:?} | {} | {} |",
            r.id,
            cell(&r.description),
            r.likelihood,
            r.impact,
            cell(&r.mitigation),
            cell(&r.components.join(", "))
        );
    }
    o.push('\n');
}

fn compliance_table(o: &mut String, entries: &[ComplianceEntry]) {
    let _ = writeln!(
        o,
        "| Standard | Status | Evidence / justification | Decision | Components |\n|---|---|---|---|---|"
    );
    for e in entries {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} |",
            e.standard,
            match e.status {
                ComplianceStatus::Compliant => "compliant",
                ComplianceStatus::Deviation => "**deviation**",
                ComplianceStatus::NotApplicable => "not applicable",
            },
            cell(&e.justification),
            e.decision.as_deref().unwrap_or("–"),
            cell(&e.components.join(", "))
        );
    }
    o.push('\n');
}

fn decisions_index(o: &mut String, ds: &[Decision]) {
    let _ = writeln!(o, "| Id | Decision | Status | ADR |\n|---|---|---|---|");
    for d in ds {
        let _ = writeln!(
            o,
            "| {} | {} | {:?} | [{}]({}) |",
            d.id,
            cell(&d.title),
            d.status,
            d.id,
            adr_path(d)
        );
    }
    o.push('\n');
}

pub fn render_hld(h: &Hld) -> Bundle {
    let mut b = Bundle::default();
    let ctx = mermaid::context(h);
    let comps = mermaid::components(h);
    let name_of = |id: &str| {
        h.components
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let dep = mermaid::deployment(&h.deployment, &name_of);
    let mut o = String::new();
    let _ = writeln!(o, "# {}\n", cell(&h.title));
    if !h.summary.trim().is_empty() {
        let _ = writeln!(o, "{}\n", text(&h.summary));
    }
    let _ = writeln!(
        o,
        "## 1. System context\n\n**{}** – {}\n",
        cell(&h.context.system_name),
        text(&h.context.description)
    );
    let _ = writeln!(o, "| Actor | Kind | Description |\n|---|---|---|");
    for a in &h.context.actors {
        let _ = writeln!(
            o,
            "| {} | {:?} | {} |",
            cell(&a.name),
            a.kind,
            cell(&a.description)
        );
    }
    o.push('\n');
    mermaid_block(&mut o, "Context diagram", &ctx);
    let _ = writeln!(
        o,
        "## 2. Components\n\n| Id | Component | Kind | Technology | Responsibilities | Requirements |\n|---|---|---|---|---|---|"
    );
    for c in &h.components {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} | {} |",
            c.id,
            cell(&c.name),
            c.kind.label(),
            cell(&c.technology),
            cell(&c.responsibilities.join("; ")),
            c.requirements.join(", ")
        );
    }
    o.push('\n');
    mermaid_block(&mut o, "Component diagram", &comps);
    let _ = writeln!(
        o,
        "## 3. Interfaces and data flows\n\n| Id | Provider | Consumers | Protocol | Description |\n|---|---|---|---|---|"
    );
    for i in &h.interfaces {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} |",
            cell(&i.id),
            i.provider,
            i.consumers.join(", "),
            cell(&i.protocol),
            cell(&i.description)
        );
    }
    let _ = writeln!(
        o,
        "\n| Id | From | To | Data | Description |\n|---|---|---|---|---|"
    );
    for f in &h.data_flows {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} |",
            cell(&f.id),
            f.from,
            f.to,
            cell(&f.data),
            cell(&f.description)
        );
    }
    o.push('\n');
    let _ = writeln!(o, "## 4. Deployment\n");
    mermaid_block(&mut o, "Deployment diagram", &dep);
    let _ = writeln!(o, "## 5. Architecture decisions\n");
    decisions_index(&mut o, &h.decisions);
    let _ = writeln!(
        o,
        "## 6. Trade-offs\n\n| Id | Concern | Options | Choice | Rationale |\n|---|---|---|---|---|"
    );
    for t in &h.tradeoffs {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} |",
            t.id,
            cell(&t.concern),
            cell(&t.options.join(" / ")),
            cell(&t.choice),
            cell(&t.rationale)
        );
    }
    let _ = writeln!(o, "\n## 7. Risks\n");
    risks_table(&mut o, &h.risks);
    let _ = writeln!(o, "## 8. Standards compliance\n");
    compliance_table(&mut o, &h.compliance);
    let _ = writeln!(
        o,
        "## 9. Requirement allocation\n\n| Requirement | Allocated to |\n|---|---|"
    );
    let mut alloc: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for c in &h.components {
        for r in &c.requirements {
            alloc.entry(r).or_default().insert(c.id.clone());
        }
    }
    for i in &h.interfaces {
        for r in &i.requirements {
            alloc.entry(r).or_default().insert(format!("interface {}", i.id));
        }
    }
    for d in &h.decisions {
        for r in &d.requirements {
            alloc.entry(r).or_default().insert(d.id.clone());
        }
    }
    for (r, to) in &alloc {
        let _ = writeln!(
            o,
            "| {r} | {} |",
            to.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    if !h.citations.is_empty() {
        let _ = writeln!(o, "\n## Sources\n");
        for c in &h.citations {
            let _ = writeln!(o, "* {c}");
        }
    }
    b.files.insert("hld.md".into(), o);
    b.files.insert("diagrams/context.mmd".into(), ctx);
    b.files.insert("diagrams/components.mmd".into(), comps);
    b.files.insert("diagrams/deployment.mmd".into(), dep);
    for d in &h.decisions {
        b.files.insert(adr_path(d), adr(d, "HLD"));
    }
    b
}

pub fn render_lld(l: &Lld, hld: Option<&Hld>) -> Bundle {
    let mut b = Bundle::default();
    let name_of = |id: &str| {
        hld.and_then(|h| {
            h.components
                .iter()
                .map(|c| (c.id.as_str(), c.name.as_str()))
                .chain(h.context.actors.iter().map(|a| (a.id.as_str(), a.name.as_str())))
                .find(|(i, _)| *i == id)
                .map(|(_, n)| n.to_string())
        })
        .unwrap_or_else(|| id.to_string())
    };
    let mut o = String::new();
    let _ = writeln!(o, "# {}\n", cell(&l.title));
    if !l.summary.trim().is_empty() {
        let _ = writeln!(o, "{}\n", text(&l.summary));
    }
    let mods = mermaid::modules(l, &name_of);
    let _ = writeln!(o, "## 1. Component design\n");
    for c in &l.components {
        let _ = writeln!(o, "### {} ({})\n", cell(&name_of(&c.component)), c.component);
        if !c.description.trim().is_empty() {
            let _ = writeln!(o, "{}\n", text(&c.description));
        }
        let _ = writeln!(o, "| Module | Responsibility |\n|---|---|");
        for m in &c.modules {
            let _ = writeln!(o, "| {} | {} |", cell(&m.name), cell(&m.responsibility));
        }
        if !c.requirements.is_empty() {
            let _ = writeln!(o, "\nRequirements: {}", c.requirements.join(", "));
        }
        o.push('\n');
    }
    mermaid_block(&mut o, "Component internals", &mods);
    let _ = writeln!(
        o,
        "## 2. API design\n\n| Id | Component | Method | Path / topic | Summary | Request | Response | Errors | Requirements |\n|---|---|---|---|---|---|---|---|---|"
    );
    for a in &l.apis {
        let _ = writeln!(
            o,
            "| {} | {} | {} | `{}` | {} | {} | {} | {} | {} |",
            cell(&a.id),
            a.component,
            a.method,
            cell(&a.path).replace('`', "'"),
            cell(&a.summary),
            a.request.as_deref().map(cell).unwrap_or_else(|| "–".into()),
            a.response.as_deref().map(cell).unwrap_or_else(|| "–".into()),
            cell(&a.errors.join(", ")),
            a.requirements.join(", ")
        );
    }
    o.push('\n');
    let er = mermaid::er(&l.data);
    let _ = writeln!(o, "## 3. Data design\n");
    for e in &l.data {
        let _ = writeln!(o, "### {} (owner: {})\n", cell(&e.name), e.owner);
        if !e.description.trim().is_empty() {
            let _ = writeln!(o, "{}\n", text(&e.description));
        }
        let _ = writeln!(
            o,
            "| Attribute | Type | Required | Description |\n|---|---|---|---|"
        );
        for at in &e.attributes {
            let _ = writeln!(
                o,
                "| {} | {} | {} | {} |",
                cell(&at.name),
                cell(&at.kind),
                if at.required { "yes" } else { "no" },
                cell(&at.description)
            );
        }
        o.push('\n');
    }
    mermaid_block(&mut o, "Data model", &er);
    b.files.insert("diagrams/data-model.mmd".into(), er);
    let _ = writeln!(o, "## 4. Sequences\n");
    for q in &l.sequences {
        let src = mermaid::sequence(q, &name_of);
        let req = if q.requirements.is_empty() {
            String::new()
        } else {
            format!(" (requirements: {})", q.requirements.join(", "))
        };
        mermaid_block(&mut o, &format!("{} – {}{req}", q.id, cell(&q.title)), &src);
        b.files
            .insert(format!("diagrams/sequence-{}.mmd", slug(&q.id)), src);
    }
    let dep = mermaid::deployment(&l.deployment, &name_of);
    let _ = writeln!(o, "## 5. Deployment design\n");
    let _ = writeln!(
        o,
        "| Node | Environment | Technology | Hosts |\n|---|---|---|---|"
    );
    for n in &l.deployment {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} |",
            cell(&n.name),
            cell(&n.environment),
            cell(&n.technology),
            n.hosts.join(", ")
        );
    }
    o.push('\n');
    mermaid_block(&mut o, "Deployment diagram", &dep);
    if !l.decisions.is_empty() {
        let _ = writeln!(o, "## 6. Design decisions\n");
        decisions_index(&mut o, &l.decisions);
    }
    if !l.risks.is_empty() {
        let _ = writeln!(o, "## 7. Risks\n");
        risks_table(&mut o, &l.risks);
    }
    if !l.compliance.is_empty() {
        let _ = writeln!(o, "## 8. Standards compliance\n");
        compliance_table(&mut o, &l.compliance);
    }
    if !l.citations.is_empty() {
        let _ = writeln!(o, "## Sources\n");
        for c in &l.citations {
            let _ = writeln!(o, "* {c}");
        }
    }
    b.files.insert("lld.md".into(), o);
    b.files.insert("diagrams/component-internals.mmd".into(), mods);
    b.files.insert("diagrams/deployment.mmd".into(), dep);
    for d in &l.decisions {
        b.files.insert(adr_path(d), adr(d, "LLD"));
    }
    b
}

/// HLD trace links: requirement → component / interface / decision, decision → standard,
/// standard → compliance status.
pub fn hld_trace(h: &Hld) -> Vec<TraceLink> {
    let mut out = BTreeSet::new();
    for c in &h.components {
        for r in &c.requirements {
            out.insert(TraceLink::new(
                "requirement",
                r,
                "allocated_to",
                "hld_component",
                &c.id,
            ));
        }
    }
    for i in &h.interfaces {
        for r in &i.requirements {
            out.insert(TraceLink::new(
                "requirement",
                r,
                "allocated_to",
                "hld_interface",
                &i.id,
            ));
        }
    }
    for d in &h.decisions {
        for r in &d.requirements {
            out.insert(TraceLink::new(
                "requirement",
                r,
                "addressed_by",
                "decision",
                &d.id,
            ));
        }
        for s in &d.standards {
            out.insert(TraceLink::new("decision", &d.id, "relates_to", "standard", s));
        }
    }
    for e in &h.compliance {
        let rel = match e.status {
            ComplianceStatus::Compliant => "complied_with_by",
            ComplianceStatus::Deviation => "deviated_from_by",
            ComplianceStatus::NotApplicable => "not_applicable_to",
        };
        out.insert(TraceLink::new("standard", &e.standard, rel, "hld", &h.title));
        if let Some(d) = &e.decision {
            out.insert(TraceLink::new(
                "standard",
                &e.standard,
                "deviation_decided_in",
                "decision",
                d,
            ));
        }
    }
    out.into_iter().collect()
}

/// LLD trace links: HLD component → LLD component design, requirement → API / sequence /
/// design, API → component.
pub fn lld_trace(l: &Lld) -> Vec<TraceLink> {
    let mut out = BTreeSet::new();
    for c in &l.components {
        out.insert(TraceLink::new(
            "hld_component",
            &c.component,
            "refined_by",
            "lld_component",
            &c.component,
        ));
        for r in &c.requirements {
            out.insert(TraceLink::new(
                "requirement",
                r,
                "realised_by",
                "lld_component",
                &c.component,
            ));
        }
    }
    for a in &l.apis {
        out.insert(TraceLink::new(
            "lld_component",
            &a.component,
            "exposes",
            "api_operation",
            &a.id,
        ));
        for r in &a.requirements {
            out.insert(TraceLink::new(
                "requirement",
                r,
                "realised_by",
                "api_operation",
                &a.id,
            ));
        }
    }
    for q in &l.sequences {
        for r in &q.requirements {
            out.insert(TraceLink::new("requirement", r, "realised_by", "sequence", &q.id));
        }
    }
    for e in &l.data {
        out.insert(TraceLink::new(
            "lld_component",
            &e.owner,
            "owns",
            "entity",
            &e.name,
        ));
    }
    for d in &l.decisions {
        for r in &d.requirements {
            out.insert(TraceLink::new(
                "requirement",
                r,
                "addressed_by",
                "decision",
                &d.id,
            ));
        }
    }
    out.into_iter().collect()
}
