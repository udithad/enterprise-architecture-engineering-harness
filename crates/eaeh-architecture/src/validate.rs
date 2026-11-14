//! Deterministic HLD / LLD validation: referential integrity, requirement coverage,
//! LLD-refines-HLD, decision (ADR) and risk/trade-off completeness, and compliance with the
//! mandatory standards that were supplied to the architect.

use crate::model::*;
use eaeh_requirements::{Priority, RequirementKind, RequirementSet, has_numbered_id, is_document_citation};
use eaeh_validation::{Finding, Severity};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const HLD_VALIDATOR: &str = "architecture.hld";
pub const LLD_VALIDATOR: &str = "architecture.lld";

/// Platform-supplied context (never asserted by the model).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DesignContext {
    /// The approved requirements the design must satisfy.
    #[serde(default)]
    pub requirements: Option<RequirementSet>,
    /// Mandatory standards (`src/subject@vN`) in force for the project.
    #[serde(default)]
    pub standards: Vec<String>,
    /// The approved HLD (for LLD validation).
    #[serde(default)]
    pub hld: Option<Hld>,
}

struct Sink {
    validator: &'static str,
    out: Vec<Finding>,
}

impl Sink {
    fn add(&mut self, severity: Severity, loc: impl Into<String>, msg: impl Into<String>) {
        self.out.push(Finding {
            validator: self.validator.into(),
            severity,
            message: msg.into(),
            location: Some(loc.into()),
            evidence: Value::Null,
        });
    }
    fn err(&mut self, loc: impl Into<String>, msg: impl Into<String>) {
        self.add(Severity::Error, loc, msg);
    }
    fn warn(&mut self, loc: impl Into<String>, msg: impl Into<String>) {
        self.add(Severity::Warning, loc, msg);
    }
}

pub fn is_slug(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_lowercase())
        && s.len() <= 64
        && s.chars()
            .all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == '-')
}

fn nonempty(v: &[String]) -> bool {
    v.iter().any(|x| !x.trim().is_empty())
}

fn requirement_ids(ctx: &DesignContext) -> Option<BTreeMap<&str, (RequirementKind, Priority)>> {
    ctx.requirements.as_ref().map(|r| {
        r.requirements
            .iter()
            .map(|q| (q.id.as_str(), (q.kind, q.priority)))
            .collect()
    })
}

fn check_req_refs(
    s: &mut Sink,
    loc: &str,
    refs: &[String],
    known: &Option<BTreeMap<&str, (RequirementKind, Priority)>>,
) {
    for r in refs {
        match known {
            Some(k) if !k.contains_key(r.as_str()) => s.err(loc, format!("unknown requirement {r}")),
            None if !(r.starts_with("REQ-F-") || r.starts_with("REQ-NF-")) => {
                s.err(loc, format!("'{r}' is not a requirement id"))
            }
            _ => {}
        }
    }
}

fn check_decisions(
    s: &mut Sink,
    prefix: &str,
    decisions: &[Decision],
    known: &Option<BTreeMap<&str, (RequirementKind, Priority)>>,
    citations: &BTreeSet<&str>,
) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for d in decisions {
        let loc = format!("decisions[{}]", d.id);
        if !ids.insert(d.id.clone()) {
            s.err(&loc, format!("duplicate decision {}", d.id));
        }
        if !has_numbered_id(&d.id, prefix) {
            s.err(&loc, format!("decision id must look like {prefix}nnn"));
        }
        if d.title.trim().is_empty() || d.context.trim().is_empty() || d.decision.trim().is_empty() {
            s.err(&loc, "title, context and decision are required");
        }
        if !d.alternatives.iter().any(|a| !a.option.trim().is_empty()) {
            s.err(&loc, "no alternatives considered (trade-off analysis required)");
        }
        if d.alternatives
            .iter()
            .all(|a| a.cons.is_empty() && a.pros.is_empty())
        {
            s.warn(&loc, "alternatives have no pros/cons");
        }
        if !nonempty(&d.consequences) {
            s.warn(&loc, "no consequences recorded");
        }
        check_req_refs(s, &loc, &d.requirements, known);
        for st in &d.standards {
            if !is_document_citation(st) {
                s.err(
                    &loc,
                    format!("standard '{st}' is not a document citation (src/subject@vN)"),
                );
            } else if !citations.contains(st.as_str()) {
                s.err(&loc, format!("standard '{st}' is not listed in 'citations'"));
            }
        }
    }
    ids
}

fn check_risks(s: &mut Sink, risks: &[Risk], comps: &BTreeSet<&str>) {
    let mut ids = BTreeSet::new();
    for r in risks {
        let loc = format!("risks[{}]", r.id);
        if !ids.insert(r.id.as_str()) {
            s.err(&loc, format!("duplicate risk {}", r.id));
        }
        if !has_numbered_id(&r.id, "R-") {
            s.err(&loc, "risk id must look like R-nnn");
        }
        if r.description.trim().is_empty() || r.mitigation.trim().is_empty() {
            s.err(&loc, "description and mitigation are required");
        }
        for c in &r.components {
            if !comps.contains(c.as_str()) {
                s.err(&loc, format!("unknown component '{c}'"));
            }
        }
    }
}

fn check_compliance(
    s: &mut Sink,
    entries: &[ComplianceEntry],
    standards: &[String],
    decisions: &BTreeSet<String>,
    comps: &BTreeSet<&str>,
    citations: &BTreeSet<&str>,
) {
    let mut seen = BTreeSet::new();
    for e in entries {
        let loc = format!("compliance[{}]", e.standard);
        if !is_document_citation(&e.standard) {
            s.err(&loc, "standard must be a document citation (src/subject@vN)");
        } else if !citations.contains(e.standard.as_str()) {
            s.err(&loc, "standard is not listed in 'citations'");
        }
        if !seen.insert(e.standard.as_str()) {
            s.err(&loc, "duplicate compliance entry");
        }
        match e.status {
            ComplianceStatus::Deviation => {
                if e.justification.trim().is_empty() {
                    s.err(&loc, "deviation needs a justification");
                }
                match &e.decision {
                    Some(d) if decisions.contains(d) => {}
                    Some(d) => s.err(&loc, format!("deviation references unknown decision {d}")),
                    None => s.err(&loc, "deviation must be documented by a decision (ADR)"),
                }
            }
            ComplianceStatus::NotApplicable if e.justification.trim().is_empty() => {
                s.err(&loc, "not-applicable needs a justification")
            }
            ComplianceStatus::Compliant if e.justification.trim().is_empty() => {
                s.warn(&loc, "compliant without evidence")
            }
            _ => {}
        }
        for c in &e.components {
            if !comps.contains(c.as_str()) {
                s.err(&loc, format!("unknown component '{c}'"));
            }
        }
    }
    for st in standards {
        if !seen.contains(st.as_str()) {
            s.err(
                "compliance",
                format!("mandatory standard {st} is not addressed (compliant / deviation / not_applicable)"),
            );
        }
    }
}

pub fn validate_hld(h: &Hld, ctx: &DesignContext) -> Vec<Finding> {
    let mut s = Sink {
        validator: HLD_VALIDATOR,
        out: vec![],
    };
    let known = requirement_ids(ctx);
    let citations: BTreeSet<&str> = h.citations.iter().map(String::as_str).collect();
    if h.document != HLD_KIND {
        s.err("document", "document must be 'hld'");
    }
    if h.title.trim().is_empty() || h.context.system_name.trim().is_empty() {
        s.err("title", "title and context.system_name are required");
    }
    if h.components.is_empty() {
        s.err("components", "no components");
    }
    let mut nodes: BTreeSet<&str> = BTreeSet::new();
    for a in &h.context.actors {
        if !is_slug(&a.id) {
            s.err(format!("actors[{}]", a.id), "id must be a lower-case slug");
        }
        if !nodes.insert(a.id.as_str()) {
            s.err(format!("actors[{}]", a.id), format!("duplicate id '{}'", a.id));
        }
    }
    let actors: BTreeSet<&str> = h.context.actors.iter().map(|a| a.id.as_str()).collect();
    let comps: BTreeSet<&str> = h.components.iter().map(|c| c.id.as_str()).collect();
    for c in &h.components {
        let loc = format!("components[{}]", c.id);
        if !is_slug(&c.id) {
            s.err(&loc, "id must be a lower-case slug");
        }
        if !nodes.insert(c.id.as_str()) {
            s.err(&loc, format!("duplicate id '{}'", c.id));
        }
        if c.name.trim().is_empty() || !nonempty(&c.responsibilities) {
            s.err(&loc, "name and at least one responsibility are required");
        }
        check_req_refs(&mut s, &loc, &c.requirements, &known);
    }
    let mut ids = BTreeSet::new();
    for i in &h.interfaces {
        let loc = format!("interfaces[{}]", i.id);
        if i.id.trim().is_empty() || !ids.insert(i.id.as_str()) {
            s.err(&loc, "interface id missing or duplicate");
        }
        if !comps.contains(i.provider.as_str()) {
            s.err(&loc, format!("provider '{}' is not a component", i.provider));
        }
        if i.consumers.is_empty() {
            s.err(&loc, "no consumers");
        }
        for c in &i.consumers {
            if !comps.contains(c.as_str()) && !actors.contains(c.as_str()) {
                s.err(&loc, format!("consumer '{c}' is not a component or actor"));
            }
        }
        if i.protocol.trim().is_empty() {
            s.err(&loc, "protocol is required");
        }
        check_req_refs(&mut s, &loc, &i.requirements, &known);
    }
    let mut fids = BTreeSet::new();
    for f in &h.data_flows {
        let loc = format!("data_flows[{}]", f.id);
        if f.id.trim().is_empty() || !fids.insert(f.id.as_str()) {
            s.err(&loc, "data flow id missing or duplicate");
        }
        for end in [&f.from, &f.to] {
            if !nodes.contains(end.as_str()) {
                s.err(&loc, format!("'{end}' is not a component or actor"));
            }
        }
        if f.data.trim().is_empty() {
            s.err(&loc, "data is required");
        }
    }
    let deployed = check_deployment(&mut s, &h.deployment, &comps);
    if h.deployment.is_empty() {
        s.warn("deployment", "no deployment view");
    } else {
        for c in h.components.iter().filter(|c| c.kind.is_deployable()) {
            if !deployed.contains(c.id.as_str()) {
                s.warn(
                    format!("components[{}]", c.id),
                    "component is not deployed on any node",
                );
            }
        }
    }
    let decisions = check_decisions(&mut s, "D-", &h.decisions, &known, &citations);
    if h.decisions.is_empty() {
        s.err("decisions", "no architecture decisions recorded");
    }
    check_risks(&mut s, &h.risks, &comps);
    if h.risks.is_empty() {
        s.err("risks", "no risks identified");
    }
    let mut tids = BTreeSet::new();
    for t in &h.tradeoffs {
        let loc = format!("tradeoffs[{}]", t.id);
        if !has_numbered_id(&t.id, "T-") || !tids.insert(t.id.as_str()) {
            s.err(&loc, "trade-off id must be a unique T-nnn");
        }
        if t.options.len() < 2 {
            s.err(&loc, "a trade-off needs at least two options");
        }
        if !t.options.contains(&t.choice) {
            s.err(&loc, "choice must be one of the options");
        }
        if t.rationale.trim().is_empty() {
            s.err(&loc, "rationale is required");
        }
    }
    check_compliance(
        &mut s,
        &h.compliance,
        &ctx.standards,
        &decisions,
        &comps,
        &citations,
    );

    // Requirement coverage: every Must (Should) requirement allocated somewhere.
    if let Some(reqs) = &ctx.requirements {
        let mut allocated: BTreeSet<&str> = BTreeSet::new();
        for c in &h.components {
            allocated.extend(c.requirements.iter().map(String::as_str));
        }
        for i in &h.interfaces {
            allocated.extend(i.requirements.iter().map(String::as_str));
        }
        for d in &h.decisions {
            allocated.extend(d.requirements.iter().map(String::as_str));
        }
        for r in &reqs.requirements {
            if allocated.contains(r.id.as_str()) {
                continue;
            }
            match r.priority {
                Priority::Must => s.err(
                    "coverage",
                    format!(
                        "Must requirement {} is not allocated to any component, interface or decision",
                        r.id
                    ),
                ),
                Priority::Should => s.warn(
                    "coverage",
                    format!("Should requirement {} is not allocated", r.id),
                ),
                _ => {}
            }
        }
    }
    s.out
}

fn check_deployment<'a>(
    s: &mut Sink,
    nodes: &'a [DeploymentNode],
    comps: &BTreeSet<&str>,
) -> BTreeSet<&'a str> {
    let mut ids = BTreeSet::new();
    let mut deployed = BTreeSet::new();
    for n in nodes {
        let loc = format!("deployment[{}]", n.id);
        if !is_slug(&n.id) || !ids.insert(n.id.as_str()) {
            s.err(&loc, "node id must be a unique lower-case slug");
        }
        if n.hosts.is_empty() {
            s.warn(&loc, "node hosts nothing");
        }
        for h in &n.hosts {
            if !comps.contains(h.as_str()) {
                s.err(&loc, format!("hosted '{h}' is not a component"));
            }
            deployed.insert(h.as_str());
        }
    }
    deployed
}

const HTTP: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];
const EVENT: &[&str] = &["PUBLISH", "SUBSCRIBE"];

pub fn validate_lld(l: &Lld, ctx: &DesignContext) -> Vec<Finding> {
    let mut s = Sink {
        validator: LLD_VALIDATOR,
        out: vec![],
    };
    let known = requirement_ids(ctx);
    let citations: BTreeSet<&str> = l.citations.iter().map(String::as_str).collect();
    if l.document != LLD_KIND {
        s.err("document", "document must be 'lld'");
    }
    if l.title.trim().is_empty() {
        s.err("title", "title is required");
    }
    if l.components.is_empty() {
        s.err("components", "no component designs");
    }
    let hld_comps: BTreeMap<&str, ComponentKind> = ctx
        .hld
        .as_ref()
        .map(|h| h.components.iter().map(|c| (c.id.as_str(), c.kind)).collect())
        .unwrap_or_default();
    let hld_actors: BTreeSet<&str> = ctx
        .hld
        .as_ref()
        .map(|h| h.context.actors.iter().map(|a| a.id.as_str()).collect())
        .unwrap_or_default();
    let mut designed: BTreeSet<&str> = BTreeSet::new();
    for c in &l.components {
        let loc = format!("components[{}]", c.component);
        if !designed.insert(c.component.as_str()) {
            s.err(&loc, "component designed twice");
        }
        if ctx.hld.is_some() && !hld_comps.contains_key(c.component.as_str()) {
            s.err(
                &loc,
                format!(
                    "'{}' does not refine any component of the approved HLD",
                    c.component
                ),
            );
        }
        if !c
            .modules
            .iter()
            .any(|m| !m.name.trim().is_empty() && !m.responsibility.trim().is_empty())
        {
            s.err(&loc, "at least one module with a responsibility is required");
        }
        check_req_refs(&mut s, &loc, &c.requirements, &known);
    }
    for (id, kind) in &hld_comps {
        if kind.is_buildable() && !designed.contains(id) {
            s.err(
                "components",
                format!("HLD component '{id}' ({}) has no detailed design", kind.label()),
            );
        }
    }
    let entities: BTreeSet<&str> = l.data.iter().map(|e| e.name.as_str()).collect();
    let mut api_ids: BTreeMap<&str, &ApiOperation> = BTreeMap::new();
    let mut endpoints = BTreeSet::new();
    for a in &l.apis {
        let loc = format!("apis[{}]", a.id);
        if a.id.trim().is_empty() || api_ids.insert(a.id.as_str(), a).is_some() {
            s.err(&loc, "operation id missing or duplicate");
        }
        if !designed.contains(a.component.as_str()) {
            s.err(
                &loc,
                format!("component '{}' has no design in this LLD", a.component),
            );
        }
        let m = a.method.as_str();
        if HTTP.contains(&m) {
            if !a.path.starts_with('/') || a.path.contains(char::is_whitespace) {
                s.err(&loc, "HTTP path must start with '/' and contain no whitespace");
            }
        } else if EVENT.contains(&m) {
            if a.path.trim().is_empty() {
                s.err(&loc, "topic is required");
            }
        } else {
            s.err(
                &loc,
                format!(
                    "method '{m}' must be one of {}",
                    [HTTP, EVENT].concat().join(", ")
                ),
            );
        }
        if !endpoints.insert((a.component.as_str(), m, a.path.as_str())) {
            s.err(&loc, format!("duplicate endpoint {m} {}", a.path));
        }
        for p in [&a.request, &a.response].into_iter().flatten() {
            if !entities.contains(p.as_str()) {
                s.err(&loc, format!("payload '{p}' is not a defined entity"));
            }
        }
        if a.summary.trim().is_empty() {
            s.err(&loc, "summary is required");
        }
        check_req_refs(&mut s, &loc, &a.requirements, &known);
    }
    let mut names = BTreeSet::new();
    for e in &l.data {
        let loc = format!("data[{}]", e.name);
        if e.name.trim().is_empty() || !names.insert(e.name.as_str()) {
            s.err(&loc, "entity name missing or duplicate");
        }
        if !designed.contains(e.owner.as_str()) && !hld_comps.contains_key(e.owner.as_str()) {
            s.err(&loc, format!("owner '{}' is not a component", e.owner));
        }
        if e.attributes.is_empty() {
            s.err(&loc, "entity has no attributes");
        }
        let mut an = BTreeSet::new();
        for at in &e.attributes {
            if at.name.trim().is_empty() || at.kind.trim().is_empty() || !an.insert(at.name.as_str()) {
                s.err(
                    &loc,
                    format!("attribute '{}' missing name/type or duplicate", at.name),
                );
            }
        }
        for r in &e.relationships {
            if !entities.contains(r.target.as_str()) {
                s.err(
                    &loc,
                    format!("relationship target '{}' is not an entity", r.target),
                );
            }
        }
    }
    let participants: BTreeSet<&str> = designed
        .iter()
        .copied()
        .chain(hld_comps.keys().copied())
        .chain(hld_actors.iter().copied())
        .collect();
    let mut sids = BTreeSet::new();
    for q in &l.sequences {
        let loc = format!("sequences[{}]", q.id);
        if !has_numbered_id(&q.id, "S-") || !sids.insert(q.id.as_str()) {
            s.err(&loc, "sequence id must be a unique S-nnn");
        }
        if q.steps.is_empty() {
            s.err(&loc, "sequence has no steps");
        }
        for (n, st) in q.steps.iter().enumerate() {
            for p in [&st.from, &st.to] {
                if !participants.contains(p.as_str()) {
                    s.err(
                        &loc,
                        format!("step {}: participant '{p}' is not a component or actor", n + 1),
                    );
                }
            }
            if let Some(op) = &st.operation {
                match api_ids.get(op.as_str()) {
                    None => s.err(&loc, format!("step {}: unknown operation '{op}'", n + 1)),
                    Some(a) if a.component != st.to => s.err(
                        &loc,
                        format!(
                            "step {}: operation '{op}' belongs to '{}', not '{}'",
                            n + 1,
                            a.component,
                            st.to
                        ),
                    ),
                    _ => {}
                }
            }
        }
        check_req_refs(&mut s, &loc, &q.requirements, &known);
    }
    let all_comps: BTreeSet<&str> = designed
        .iter()
        .copied()
        .chain(hld_comps.keys().copied())
        .collect();
    let deployed = check_deployment(&mut s, &l.deployment, &all_comps);
    for c in &designed {
        if !l.deployment.is_empty() && !deployed.contains(c) {
            s.warn(format!("components[{c}]"), "designed component is not deployed");
        }
    }
    if l.deployment.is_empty() {
        s.err("deployment", "no deployment design");
    }
    let decisions = check_decisions(&mut s, "LD-", &l.decisions, &known, &citations);
    check_risks(&mut s, &l.risks, &all_comps);
    check_compliance(
        &mut s,
        &l.compliance,
        &ctx.standards,
        &decisions,
        &all_comps,
        &citations,
    );
    if let Some(reqs) = &ctx.requirements {
        let mut covered: BTreeSet<&str> = BTreeSet::new();
        for c in &l.components {
            covered.extend(c.requirements.iter().map(String::as_str));
        }
        for a in &l.apis {
            covered.extend(a.requirements.iter().map(String::as_str));
        }
        for q in &l.sequences {
            covered.extend(q.requirements.iter().map(String::as_str));
        }
        for d in &l.decisions {
            covered.extend(d.requirements.iter().map(String::as_str));
        }
        for r in &reqs.requirements {
            if r.priority == Priority::Must && !covered.contains(r.id.as_str()) {
                s.err("coverage", format!("Must requirement {} is not realised by any component design, API, sequence or decision", r.id));
            }
        }
    }
    s.out
}
