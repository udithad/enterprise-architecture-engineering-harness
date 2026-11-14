//! Deterministic requirements checks (master §30): structural validation, ambiguity,
//! conflict, duplicate and missing-requirement detection, BRD coverage and traceability.
//!
//! Everything here is pure and reproducible: identical input ⇒ identical findings.
//! Heuristics (lexicon, similarity thresholds) are configurable and documented in
//! ADR-0018; they flag candidates for human review, they do not "understand" text.

use crate::model::*;
use eaeh_validation::{Finding, Severity};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const VALIDATOR_ID: &str = "requirements.set";

/// Analyzer configuration. Defaults are generic requirements-engineering heuristics
/// (vague-term guidance in ISO/IEC/IEEE 29148, quality characteristics of ISO/IEC 25010);
/// they are **not** enterprise standards and can be replaced per deployment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnalysisConfig {
    /// Vague / unverifiable phrases (case-insensitive, whole words).
    pub weak_phrases: Vec<String>,
    /// Unresolved-content markers (case-insensitive, whole words).
    pub unresolved_markers: Vec<String>,
    /// Expected non-functional categories; a missing category is reported.
    pub nfr_categories: Vec<String>,
    /// Content-token Jaccard similarity at or above which two statements are duplicates.
    pub duplicate_threshold: f64,
    /// Similarity at or above which a negated/affirmed pair is a contradiction.
    pub contradiction_threshold: f64,
    /// Similarity at or above which two quantified NFRs with different values conflict.
    pub numeric_conflict_threshold: f64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect();
        Self {
            weak_phrases: s(&[
                "user friendly",
                "user-friendly",
                "easy to use",
                "easy",
                "fast",
                "quickly",
                "efficient",
                "flexible",
                "robust",
                "seamless",
                "intuitive",
                "adequate",
                "sufficient",
                "appropriate",
                "as appropriate",
                "as needed",
                "if possible",
                "where possible",
                "etc",
                "and/or",
                "approximately",
                "state of the art",
                "minimal",
                "maximize",
                "minimize",
                "best",
                "normally",
                "typically",
            ]),
            unresolved_markers: s(&["tbd", "tbc", "to be determined", "to be confirmed", "xxx"]),
            nfr_categories: s(&[
                "performance_efficiency",
                "compatibility",
                "usability",
                "reliability",
                "security",
                "maintainability",
                "portability",
            ]),
            duplicate_threshold: 0.9,
            contradiction_threshold: 0.8,
            numeric_conflict_threshold: 0.6,
        }
    }
}

/// Context the platform supplies (never the model): the BRD sections in scope.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnalysisContext {
    /// Sections of the governed BRD document(s). When empty, coverage is not evaluated.
    #[serde(default)]
    pub brd_sections: Vec<BrdSection>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    pub brd_sections: usize,
    pub covered: Vec<String>,
    pub excluded: Vec<String>,
    pub uncovered: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub functional: usize,
    pub non_functional: usize,
    pub business_rules: usize,
    pub domain_concepts: usize,
    pub must: usize,
    pub with_acceptance_criteria: usize,
    pub agent_issues: usize,
    pub analyzer_issues: usize,
}

/// Result of [`analyze`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    /// Deterministic issues (origin `analyzer`, ids `AN-nnn`).
    pub issues: Vec<Issue>,
    /// Validation findings (structural + analysis) for quality gates.
    pub findings: Vec<Finding>,
    pub coverage: Coverage,
    /// BRD citation → requirement / rule ids derived from it.
    pub trace: BTreeMap<String, Vec<String>>,
    pub metrics: Metrics,
}

impl Analysis {
    pub fn blocking(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity >= Severity::Error)
    }
}

fn finding(severity: Severity, location: &str, message: impl Into<String>, evidence: Value) -> Finding {
    Finding {
        validator: VALIDATOR_ID.into(),
        severity,
        message: message.into(),
        location: Some(location.into()),
        evidence,
    }
}

/// Parse a JSON value into a [`RequirementSet`] (strict); errors become a finding.
pub fn parse(value: &Value) -> Result<RequirementSet, Finding> {
    serde_json::from_value::<RequirementSet>(value.clone()).map_err(|e| {
        finding(
            Severity::Error,
            "$",
            format!("not a valid requirements document: {e}"),
            Value::Null,
        )
    })
}

/// Structural validation (ids, references, citations, acceptance criteria).
pub fn validate_structure(set: &RequirementSet) -> Vec<Finding> {
    let mut out = vec![];
    let mut err = |sev: Severity, loc: &str, msg: String| out.push(finding(sev, loc, msg, Value::Null));
    if set.document != DOCUMENT_KIND {
        err(
            Severity::Error,
            "document",
            format!("document must be '{DOCUMENT_KIND}'"),
        );
    }
    if set.title.trim().is_empty() {
        err(Severity::Error, "title", "title is empty".into());
    }
    if set.requirements.is_empty() {
        err(Severity::Error, "requirements", "no requirements".into());
    }
    let mut ids: BTreeSet<&str> = BTreeSet::new();
    let rule_ids: BTreeSet<&str> = set.business_rules.iter().map(|r| r.id.as_str()).collect();
    let concept_names: BTreeSet<String> = set
        .domain_concepts
        .iter()
        .map(|c| c.name.trim().to_lowercase())
        .collect();
    let cited: BTreeSet<&str> = set.citations.iter().map(String::as_str).collect();
    let check_sources = |loc: &str, sources: &[String], required: bool, out: &mut Vec<Finding>| {
        if required && sources.is_empty() {
            out.push(finding(
                Severity::Error,
                loc,
                "no BRD source citation (every item must be traceable)",
                Value::Null,
            ));
        }
        for s in sources {
            if !is_chunk_citation(s) {
                out.push(finding(
                    Severity::Error,
                    loc,
                    format!("source '{s}' is not a chunk citation (src/subject@vN#chunk)"),
                    Value::Null,
                ));
            } else if !cited.contains(s.as_str()) {
                out.push(finding(
                    Severity::Error,
                    loc,
                    format!(
                        "source '{s}' is not listed in 'citations' (citations are verified against retrieval)"
                    ),
                    Value::Null,
                ));
            }
        }
    };
    let mut src_findings = vec![];
    for r in &set.requirements {
        let loc = format!("requirements[{}]", r.id);
        if !ids.insert(r.id.as_str()) {
            err(Severity::Error, &loc, format!("duplicate id {}", r.id));
        }
        if !has_numbered_id(&r.id, r.kind.id_prefix()) {
            err(
                Severity::Error,
                &loc,
                format!(
                    "id must look like {}nnn for a {} requirement",
                    r.kind.id_prefix(),
                    r.kind.label()
                ),
            );
        }
        if r.title.trim().is_empty() || r.statement.trim().is_empty() {
            err(Severity::Error, &loc, "title and statement are required".into());
        }
        if r.kind == RequirementKind::NonFunctional
            && r.category.as_deref().is_none_or(|c| c.trim().is_empty())
        {
            err(
                Severity::Error,
                &loc,
                "non-functional requirement needs a quality category".into(),
            );
        }
        let has_ac = r.acceptance_criteria.iter().any(|a| !a.trim().is_empty());
        match r.priority {
            Priority::Must if !has_ac => err(
                Severity::Error,
                &loc,
                "Must requirement has no acceptance criteria".into(),
            ),
            Priority::Should if !has_ac => err(
                Severity::Warning,
                &loc,
                "Should requirement has no acceptance criteria".into(),
            ),
            _ => {}
        }
        for b in &r.business_rules {
            if !rule_ids.contains(b.as_str()) {
                err(Severity::Error, &loc, format!("unknown business rule {b}"));
            }
        }
        for c in &r.concepts {
            if !concept_names.contains(&c.trim().to_lowercase()) {
                err(Severity::Error, &loc, format!("unknown domain concept '{c}'"));
            }
        }
        check_sources(&format!("{loc}.sources"), &r.sources, true, &mut src_findings);
    }
    for b in &set.business_rules {
        let loc = format!("business_rules[{}]", b.id);
        if !ids.insert(b.id.as_str()) {
            err(Severity::Error, &loc, format!("duplicate id {}", b.id));
        }
        if !has_numbered_id(&b.id, "BR-") {
            err(Severity::Error, &loc, "id must look like BR-nnn".into());
        }
        if b.statement.trim().is_empty() {
            err(Severity::Error, &loc, "statement is required".into());
        }
        check_sources(&format!("{loc}.sources"), &b.sources, true, &mut src_findings);
    }
    let mut names = BTreeSet::new();
    for c in &set.domain_concepts {
        let loc = format!("domain_concepts[{}]", c.name);
        if !names.insert(c.name.trim().to_lowercase()) {
            err(Severity::Error, &loc, format!("duplicate concept '{}'", c.name));
        }
        if c.name.trim().is_empty() || c.definition.trim().is_empty() {
            err(Severity::Error, &loc, "name and definition are required".into());
        }
        check_sources(&format!("{loc}.sources"), &c.sources, false, &mut src_findings);
    }
    for i in &set.issues {
        let loc = format!("issues[{}]", i.id);
        if !ids.insert(i.id.as_str()) {
            err(Severity::Error, &loc, format!("duplicate id {}", i.id));
        }
        if i.origin == Origin::Agent && !has_numbered_id(&i.id, "ISS-") {
            err(Severity::Error, &loc, "id must look like ISS-nnn".into());
        }
        if i.description.trim().is_empty() {
            err(Severity::Error, &loc, "description is required".into());
        }
    }
    let all_ids: BTreeSet<&str> = ids.iter().copied().collect();
    for i in &set.issues {
        for r in &i.refs {
            let looks_like_id = r.starts_with("REQ-") || r.starts_with("BR-");
            if looks_like_id && !all_ids.contains(r.as_str()) {
                err(
                    Severity::Warning,
                    &format!("issues[{}]", i.id),
                    format!("reference {r} does not exist"),
                );
            }
        }
    }
    for x in &set.out_of_scope {
        let loc = format!("out_of_scope[{}]", x.source);
        if x.reason.trim().is_empty() {
            err(Severity::Error, &loc, "exclusion needs a reason".into());
        }
        check_sources(&loc, std::slice::from_ref(&x.source), true, &mut src_findings);
    }
    out.extend(src_findings);
    out
}

// ---------------------------------------------------------------- text heuristics

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '.'))
        .map(|t| t.trim_matches('.').to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Whole-word phrase match on token sequences.
fn contains_phrase(text_tokens: &[String], phrase: &str) -> bool {
    let p = tokens(&phrase.replace(['/', '-'], " "));
    if p.is_empty() || p.len() > text_tokens.len() {
        return false;
    }
    text_tokens.windows(p.len()).any(|w| w == p.as_slice())
}

const MODAL: &[&str] = &[
    "must", "shall", "should", "will", "can", "may", "be", "able", "to", "the", "a", "an", "is", "are", "of",
    "and", "for", "in", "on", "at", "by", "with",
];
const NEGATION: &[&str] = &["not", "never", "cannot", "no"];

fn negated(t: &[String]) -> bool {
    t.iter().any(|x| NEGATION.contains(&x.as_str()))
}

fn is_number(t: &str) -> bool {
    !t.is_empty()
        && t.chars().all(|c| c.is_ascii_digit() || c == '.')
        && t.chars().any(|c| c.is_ascii_digit())
}

fn content(t: &[String]) -> BTreeSet<String> {
    t.iter()
        .filter(|x| !MODAL.contains(&x.as_str()) && !NEGATION.contains(&x.as_str()) && !is_number(x))
        .filter(|x| unit_factor(x).is_none())
        .cloned()
        .collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let i = a.intersection(b).count() as f64;
    let u = a.union(b).count() as f64;
    i / u
}

/// (unit class, factor to base unit)
fn unit_factor(u: &str) -> Option<(&'static str, f64)> {
    Some(match u {
        "ms" | "millisecond" | "milliseconds" => ("time", 1.0),
        "s" | "sec" | "secs" | "second" | "seconds" => ("time", 1000.0),
        "min" | "mins" | "minute" | "minutes" => ("time", 60_000.0),
        "h" | "hr" | "hrs" | "hour" | "hours" => ("time", 3_600_000.0),
        "percent" | "%" => ("percent", 1.0),
        _ => return None,
    })
}

/// Quantities `(unit class, value in base unit)` found in a statement.
fn quantities(s: &str) -> Vec<(&'static str, f64)> {
    let spaced = s.replace('%', " % ");
    let t: Vec<&str> = spaced
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '(' || c == ')')
        .filter(|x| !x.is_empty())
        .collect();
    let mut out = vec![];
    for (i, w) in t.iter().enumerate() {
        let w = w.trim_end_matches('.');
        // "200ms" or "200 ms"
        let digits: String = w
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if digits.is_empty() || !digits.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(v) = digits.parse::<f64>() else { continue };
        let rest = w[digits.len()..].to_lowercase();
        let unit = if !rest.is_empty() {
            rest
        } else {
            t.get(i + 1)
                .map(|x| x.trim_end_matches('.').to_lowercase())
                .unwrap_or_default()
        };
        if let Some((class, f)) = unit_factor(&unit) {
            out.push((class, v * f));
        }
    }
    out
}

// ---------------------------------------------------------------- analysis

/// Full deterministic analysis: structure + ambiguity + conflicts + duplicates +
/// missing items + BRD coverage + traceability.
pub fn analyze(set: &RequirementSet, ctx: &AnalysisContext, cfg: &AnalysisConfig) -> Analysis {
    let mut a = Analysis {
        findings: validate_structure(set),
        ..Default::default()
    };
    let mut next = 0u32;
    let mut issue = |a: &mut Analysis,
                     kind: IssueKind,
                     severity: IssueSeverity,
                     fsev: Severity,
                     loc: &str,
                     description: String,
                     refs: Vec<String>| {
        next += 1;
        let id = format!("AN-{next:03}");
        a.findings.push(finding(
            fsev,
            loc,
            format!("{} ({id}): {description}", kind.label()),
            json!({"issue": id, "refs": refs}),
        ));
        a.issues.push(Issue {
            id,
            kind,
            severity,
            description,
            refs,
            question: String::new(),
            origin: Origin::Analyzer,
        });
    };
    // Ambiguities acknowledged by the analyst (agent issue referencing the requirement)
    // are reported as information only: they are already on the clarification list.
    let acknowledged: BTreeMap<&str, &str> = set
        .issues
        .iter()
        .filter(|i| i.kind == IssueKind::Ambiguity)
        .flat_map(|i| i.refs.iter().map(move |r| (r.as_str(), i.id.as_str())))
        .collect();

    for r in &set.requirements {
        let loc = format!("requirements[{}]", r.id);
        let mut text = r.statement.clone();
        for ac in &r.acceptance_criteria {
            text.push_str(" \n ");
            text.push_str(ac);
        }
        let t = tokens(&text.replace(['/', '-'], " "));
        let ack = acknowledged.get(r.id.as_str());
        let sev = |unacked: Severity| if ack.is_some() { Severity::Info } else { unacked };
        for m in &cfg.unresolved_markers {
            if contains_phrase(&t, m) {
                issue(
                    &mut a,
                    IssueKind::Ambiguity,
                    IssueSeverity::High,
                    sev(Severity::Error),
                    &loc,
                    format!(
                        "{} contains unresolved marker '{m}'{}",
                        r.id,
                        ack.map(|i| format!(" (acknowledged in {i})")).unwrap_or_default()
                    ),
                    vec![r.id.clone()],
                );
            }
        }
        let weak: Vec<&String> = cfg
            .weak_phrases
            .iter()
            .filter(|p| contains_phrase(&t, p))
            .collect();
        if !weak.is_empty() {
            issue(
                &mut a,
                IssueKind::Ambiguity,
                IssueSeverity::Medium,
                sev(Severity::Warning),
                &loc,
                format!(
                    "{} uses vague wording: {}{}",
                    r.id,
                    weak.iter()
                        .map(|w| format!("'{w}'"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    ack.map(|i| format!(" (acknowledged in {i})")).unwrap_or_default()
                ),
                vec![r.id.clone()],
            );
        }
        if r.kind == RequirementKind::NonFunctional && !text.chars().any(|c| c.is_ascii_digit()) {
            issue(
                &mut a,
                IssueKind::Ambiguity,
                IssueSeverity::Medium,
                sev(Severity::Warning),
                &loc,
                format!("{} is not measurable (no quantified target)", r.id),
                vec![r.id.clone()],
            );
        }
        if let Some(c) = &r.category
            && r.kind == RequirementKind::NonFunctional
            && !cfg.nfr_categories.is_empty()
            && !cfg.nfr_categories.iter().any(|x| x == c)
        {
            a.findings.push(finding(
                Severity::Warning,
                &loc,
                format!(
                    "unknown quality category '{c}' (expected one of: {})",
                    cfg.nfr_categories.join(", ")
                ),
                Value::Null,
            ));
        }
    }

    // Pairwise: contradictions, numeric conflicts, duplicates.
    let reqs = &set.requirements;
    for i in 0..reqs.len() {
        for j in (i + 1)..reqs.len() {
            let (x, y) = (&reqs[i], &reqs[j]);
            let (tx, ty) = (tokens(&x.statement), tokens(&y.statement));
            let (cx, cy) = (content(&tx), content(&ty));
            let sim = jaccard(&cx, &cy);
            let pair = vec![x.id.clone(), y.id.clone()];
            let loc = format!("requirements[{},{}]", x.id, y.id);
            if negated(&tx) != negated(&ty) && sim >= cfg.contradiction_threshold {
                issue(
                    &mut a,
                    IssueKind::Conflict,
                    IssueSeverity::High,
                    Severity::Error,
                    &loc,
                    format!(
                        "{} and {} state contradictory obligations (similarity {sim:.2})",
                        x.id, y.id
                    ),
                    pair,
                );
                continue;
            }
            let (qx, qy) = (quantities(&x.statement), quantities(&y.statement));
            if x.kind == RequirementKind::NonFunctional
                && y.kind == RequirementKind::NonFunctional
                && x.category == y.category
                && sim >= cfg.numeric_conflict_threshold
                && let (Some(a1), Some(b1)) = (qx.first(), qy.first())
                && a1.0 == b1.0
                && (a1.1 - b1.1).abs() > f64::EPSILON * a1.1.abs().max(1.0)
            {
                issue(
                    &mut a,
                    IssueKind::Conflict,
                    IssueSeverity::Medium,
                    Severity::Warning,
                    &loc,
                    format!(
                        "{} and {} set different targets for the same quality attribute",
                        x.id, y.id
                    ),
                    pair,
                );
                continue;
            }
            if negated(&tx) == negated(&ty) && sim >= cfg.duplicate_threshold && qx == qy {
                issue(
                    &mut a,
                    IssueKind::Duplicate,
                    IssueSeverity::Low,
                    Severity::Warning,
                    &loc,
                    format!(
                        "{} and {} appear to be duplicates (similarity {sim:.2})",
                        x.id, y.id
                    ),
                    pair,
                );
            }
        }
    }

    // Missing NFR categories.
    let present: BTreeSet<&str> = reqs
        .iter()
        .filter(|r| r.kind == RequirementKind::NonFunctional)
        .filter_map(|r| r.category.as_deref())
        .collect();
    for c in &cfg.nfr_categories {
        if !present.contains(c.as_str()) {
            issue(
                &mut a,
                IssueKind::Missing,
                IssueSeverity::Low,
                Severity::Warning,
                "requirements",
                format!("no non-functional requirement for quality category '{c}'"),
                vec![],
            );
        }
    }

    // Traceability + BRD coverage.
    let mut trace: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in reqs {
        for s in &r.sources {
            trace.entry(s.clone()).or_default().push(r.id.clone());
        }
    }
    for b in &set.business_rules {
        for s in &b.sources {
            trace.entry(s.clone()).or_default().push(b.id.clone());
        }
    }
    let concept_sources: BTreeSet<&str> = set
        .domain_concepts
        .iter()
        .flat_map(|c| c.sources.iter().map(String::as_str))
        .collect();
    if !ctx.brd_sections.is_empty() {
        let sections: BTreeSet<&str> = ctx.brd_sections.iter().map(|s| s.citation.as_str()).collect();
        let excluded: BTreeSet<&str> = set.out_of_scope.iter().map(|x| x.source.as_str()).collect();
        for r in reqs {
            if !r.sources.iter().any(|s| sections.contains(s.as_str())) {
                a.findings.push(finding(
                    Severity::Error,
                    &format!("requirements[{}].sources", r.id),
                    format!("{} is not traced to any section of the BRD under analysis", r.id),
                    Value::Null,
                ));
            }
        }
        for s in &ctx.brd_sections {
            let c = s.citation.as_str();
            if trace.contains_key(c) || concept_sources.contains(c) {
                a.coverage.covered.push(c.to_string());
            } else if excluded.contains(c) {
                a.coverage.excluded.push(c.to_string());
            } else {
                a.coverage.uncovered.push(c.to_string());
                issue(
                    &mut a,
                    IssueKind::Missing,
                    IssueSeverity::High,
                    Severity::Error,
                    &format!("brd[{c}]"),
                    format!(
                        "BRD section {c}{} has no derived requirement, rule or concept and is not excluded",
                        if s.heading.is_empty() {
                            String::new()
                        } else {
                            format!(" ('{}')", s.heading)
                        }
                    ),
                    vec![c.to_string()],
                );
            }
        }
        a.coverage.brd_sections = ctx.brd_sections.len();
    }
    a.trace = trace;
    a.metrics = Metrics {
        functional: reqs
            .iter()
            .filter(|r| r.kind == RequirementKind::Functional)
            .count(),
        non_functional: reqs
            .iter()
            .filter(|r| r.kind == RequirementKind::NonFunctional)
            .count(),
        business_rules: set.business_rules.len(),
        domain_concepts: set.domain_concepts.len(),
        must: reqs.iter().filter(|r| r.priority == Priority::Must).count(),
        with_acceptance_criteria: reqs
            .iter()
            .filter(|r| r.acceptance_criteria.iter().any(|x| !x.trim().is_empty()))
            .count(),
        agent_issues: set.issues.len(),
        analyzer_issues: a.issues.len(),
    };
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_matching_is_whole_word() {
        let t = tokens("The system must be fast and/or breakfast-ready etc.");
        assert!(contains_phrase(&t, "fast"));
        assert!(contains_phrase(&t, "and/or"));
        assert!(contains_phrase(&t, "etc"));
        assert!(!contains_phrase(&tokens("breakfast"), "fast"));
    }

    #[test]
    fn quantities_are_normalised() {
        assert_eq!(quantities("respond within 2 seconds"), vec![("time", 2000.0)]);
        assert_eq!(quantities("respond within 200ms (p95)"), vec![("time", 200.0)]);
        assert_eq!(
            quantities("available 99.9% of the month"),
            vec![("percent", 99.9)]
        );
        assert!(quantities("version 2 of the API").is_empty());
    }
}
