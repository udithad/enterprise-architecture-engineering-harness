//! Typed requirements model. Agents *draft* instances of these types as JSON; the
//! platform parses them strictly (`deny_unknown_fields`) and validates them
//! deterministically.

use serde::{Deserialize, Serialize};

/// Discriminator value of [`RequirementSet::document`].
pub const DOCUMENT_KIND: &str = "requirements";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementSet {
    /// Always `"requirements"`.
    pub document: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub requirements: Vec<Requirement>,
    #[serde(default)]
    pub business_rules: Vec<BusinessRule>,
    #[serde(default)]
    pub domain_concepts: Vec<DomainConcept>,
    /// Open questions / defects raised by the analyst (ambiguities, conflicts, gaps).
    #[serde(default)]
    pub issues: Vec<Issue>,
    /// BRD sections deliberately not turned into requirements (with a reason).
    #[serde(default)]
    pub out_of_scope: Vec<ScopeExclusion>,
    /// Every knowledge citation used (validated against what was retrieved).
    #[serde(default)]
    pub citations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    Functional,
    NonFunctional,
}

impl RequirementKind {
    pub fn id_prefix(self) -> &'static str {
        match self {
            RequirementKind::Functional => "REQ-F-",
            RequirementKind::NonFunctional => "REQ-NF-",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RequirementKind::Functional => "functional",
            RequirementKind::NonFunctional => "non-functional",
        }
    }
}

/// MoSCoW priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Must,
    Should,
    Could,
    Wont,
}

impl Priority {
    pub fn label(self) -> &'static str {
        match self {
            Priority::Must => "Must",
            Priority::Should => "Should",
            Priority::Could => "Could",
            Priority::Wont => "Won't",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    /// `REQ-F-nnn` or `REQ-NF-nnn`.
    pub id: String,
    pub kind: RequirementKind,
    /// Quality characteristic for non-functional requirements (e.g. `performance_efficiency`).
    #[serde(default)]
    pub category: Option<String>,
    pub title: String,
    pub statement: String,
    #[serde(default)]
    pub rationale: String,
    pub priority: Priority,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    /// BRD citations (`source/subject@vN#chunk`) this requirement is derived from.
    pub sources: Vec<String>,
    /// Business rule ids (`BR-nnn`) applied.
    #[serde(default)]
    pub business_rules: Vec<String>,
    /// Domain concept names involved.
    #[serde(default)]
    pub concepts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessRule {
    /// `BR-nnn`.
    pub id: String,
    pub statement: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainConcept {
    pub name: String,
    pub definition: String,
    #[serde(default)]
    pub attributes: Vec<String>,
    /// Free-text relationships, e.g. "Order contains OrderLine".
    #[serde(default)]
    pub relationships: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    Ambiguity,
    Conflict,
    Missing,
    Duplicate,
}

impl IssueKind {
    pub fn label(self) -> &'static str {
        match self {
            IssueKind::Ambiguity => "ambiguity",
            IssueKind::Conflict => "conflict",
            IssueKind::Missing => "missing",
            IssueKind::Duplicate => "duplicate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Raised by an (intelligent) analyst agent.
    #[default]
    Agent,
    /// Detected by the deterministic analyzer.
    Analyzer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    pub id: String,
    pub kind: IssueKind,
    pub severity: IssueSeverity,
    pub description: String,
    /// Requirement / rule ids or BRD citations concerned.
    #[serde(default)]
    pub refs: Vec<String>,
    /// Clarification question for stakeholders.
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeExclusion {
    /// BRD citation.
    pub source: String,
    pub reason: String,
}

/// A BRD section as provided by the platform (from governed knowledge).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrdSection {
    pub citation: String,
    #[serde(default)]
    pub heading: String,
}

/// `src/subject@vN#chunk` (chunk citation) – the format produced by `eaeh-knowledge`.
pub fn is_chunk_citation(c: &str) -> bool {
    let Some((doc, chunk)) = c.split_once('#') else {
        return false;
    };
    is_document_citation(doc) && !chunk.is_empty() && chunk.bytes().all(|b| b.is_ascii_digit())
}

/// `src/subject@vN`.
pub fn is_document_citation(c: &str) -> bool {
    let Some((path, ver)) = c.rsplit_once("@v") else {
        return false;
    };
    let Some((src, subject)) = path.split_once('/') else {
        return false;
    };
    !src.is_empty()
        && !subject.is_empty()
        && !ver.is_empty()
        && ver.bytes().all(|b| b.is_ascii_digit())
        && !c.contains(char::is_whitespace)
}

/// Document part (`src/subject@vN`) of a chunk or document citation.
pub fn document_of(c: &str) -> &str {
    c.split('#').next().unwrap_or(c)
}

/// `PREFIX` followed by at least three digits.
pub fn has_numbered_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|n| n.len() >= 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citation_and_id_formats() {
        assert!(is_chunk_citation("brd/orders@v1#3"));
        assert!(!is_chunk_citation("brd/orders@v1"));
        assert!(!is_chunk_citation("brd/orders@v1#x"));
        assert!(is_document_citation("demo-api-standards/error-format@v12"));
        assert!(!is_document_citation("error-format@v1"));
        assert!(!is_document_citation("a/b@v1 x"));
        assert_eq!(document_of("a/b@v2#7"), "a/b@v2");
        assert!(has_numbered_id("REQ-F-001", "REQ-F-"));
        assert!(!has_numbered_id("REQ-F-01", "REQ-F-"));
        assert!(!has_numbered_id("REQ-NF-001", "REQ-F-"));
    }
}

/// A traceability link between engineering items (BRD section, requirement, business
/// rule, HLD component, decision, LLD element, standard). Links are *computed* by the
/// platform from approved documents, never asserted by a model.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TraceLink {
    pub from_kind: String,
    pub from_id: String,
    pub relation: String,
    pub to_kind: String,
    pub to_id: String,
}

impl TraceLink {
    pub fn new(from_kind: &str, from_id: &str, relation: &str, to_kind: &str, to_id: &str) -> Self {
        Self {
            from_kind: from_kind.into(),
            from_id: from_id.into(),
            relation: relation.into(),
            to_kind: to_kind.into(),
            to_id: to_id.into(),
        }
    }
}

/// Links BRD section → requirement / business rule (`derives`) and requirement →
/// business rule (`applies`).
pub fn trace_links(set: &RequirementSet) -> Vec<TraceLink> {
    let mut out = std::collections::BTreeSet::new();
    for r in &set.requirements {
        for s in &r.sources {
            out.insert(TraceLink::new("brd_section", s, "derives", "requirement", &r.id));
        }
        for b in &r.business_rules {
            out.insert(TraceLink::new(
                "requirement",
                &r.id,
                "applies",
                "business_rule",
                b,
            ));
        }
    }
    for b in &set.business_rules {
        for s in &b.sources {
            out.insert(TraceLink::new(
                "brd_section",
                s,
                "derives",
                "business_rule",
                &b.id,
            ));
        }
    }
    out.into_iter().collect()
}
