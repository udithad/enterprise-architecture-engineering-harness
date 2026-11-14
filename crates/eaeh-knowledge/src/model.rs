use eaeh_core::{Classification, HarnessError, ProjectId, Result, TenantId, WorkflowInstanceId};
use serde::{Deserialize, Serialize};

/// Visibility scope of a knowledge document (master §17: enterprise, domain, project,
/// team, workflow). The tenant boundary is always applied in addition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "level", content = "ref")]
pub enum KnowledgeScope {
    Enterprise,
    Domain(String),
    Project(ProjectId),
    Team(String),
    Workflow(WorkflowInstanceId),
}

impl KnowledgeScope {
    pub fn level(&self) -> &'static str {
        match self {
            KnowledgeScope::Enterprise => "enterprise",
            KnowledgeScope::Domain(_) => "domain",
            KnowledgeScope::Project(_) => "project",
            KnowledgeScope::Team(_) => "team",
            KnowledgeScope::Workflow(_) => "workflow",
        }
    }

    pub fn reference(&self) -> String {
        match self {
            KnowledgeScope::Enterprise => String::new(),
            KnowledgeScope::Domain(d) | KnowledgeScope::Team(d) => d.clone(),
            KnowledgeScope::Project(p) => p.to_string(),
            KnowledgeScope::Workflow(w) => w.to_string(),
        }
    }

    /// Precedence used when documents conflict (higher wins).
    pub fn precedence(level: &str) -> u8 {
        match level {
            "enterprise" => 5,
            "domain" => 4,
            "project" => 3,
            "team" => 2,
            _ => 1,
        }
    }

    pub fn parse(level: &str, reference: &str) -> Result<Self> {
        Ok(match level {
            "enterprise" => Self::Enterprise,
            "domain" => Self::Domain(reference.into()),
            "project" => Self::Project(reference.parse()?),
            "team" => Self::Team(reference.into()),
            "workflow" => Self::Workflow(reference.parse()?),
            _ => return Err(HarnessError::integrity("unknown knowledge scope level")),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Standard,
    Reference,
    ApiSpec,
    Adr,
    Process,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Standard => "standard",
            SourceKind::Reference => "reference",
            SourceKind::ApiSpec => "api_spec",
            SourceKind::Adr => "adr",
            SourceKind::Process => "process",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "standard" => Self::Standard,
            "reference" => Self::Reference,
            "api_spec" => Self::ApiSpec,
            "adr" => Self::Adr,
            "process" => Self::Process,
            _ => return Err(HarnessError::integrity("unknown source kind")),
        })
    }

    /// Authority weight used as a tie-breaker by the deterministic reranker.
    pub fn authority(self) -> f64 {
        match self {
            SourceKind::Standard => 0.25,
            SourceKind::Adr => 0.15,
            SourceKind::ApiSpec | SourceKind::Process => 0.05,
            SourceKind::Reference => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Markdown,
    Text,
    Json,
    Openapi,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Markdown => "markdown",
            Format::Text => "text",
            Format::Json => "json",
            Format::Openapi => "openapi",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocStatus {
    Quarantined,
    PendingApproval,
    Active,
    Rejected,
    Superseded,
    Retired,
    Deleted,
}

impl DocStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DocStatus::Quarantined => "quarantined",
            DocStatus::PendingApproval => "pending_approval",
            DocStatus::Active => "active",
            DocStatus::Rejected => "rejected",
            DocStatus::Superseded => "superseded",
            DocStatus::Retired => "retired",
            DocStatus::Deleted => "deleted",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "quarantined" => Self::Quarantined,
            "pending_approval" => Self::PendingApproval,
            "active" => Self::Active,
            "rejected" => Self::Rejected,
            "superseded" => Self::Superseded,
            "retired" => Self::Retired,
            "deleted" => Self::Deleted,
            _ => return Err(HarnessError::integrity("unknown document status")),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub description: String,
    pub owner_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestRequest {
    /// Source name (unique within the tenant).
    pub source: String,
    /// Stable subject key (slug); versions accumulate per (source, subject).
    pub subject: String,
    pub title: String,
    pub format: Format,
    pub content: String,
    pub scope: KnowledgeScope,
    #[serde(default)]
    pub classification: Classification,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Mandatory enterprise/domain standard (only for `standard` sources).
    #[serde(default)]
    pub mandatory: bool,
    /// Review-by date (RFC3339); after it, the document is flagged stale.
    #[serde(default)]
    pub review_by: Option<String>,
    /// Project used as the administrative scope of the approval request.
    pub governance_project: ProjectId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub id: String,
    pub source: String,
    pub source_kind: SourceKind,
    pub subject: String,
    pub version: i64,
    pub title: String,
    pub format: Format,
    pub content_hash: String,
    pub scope: KnowledgeScope,
    pub classification: Classification,
    pub tags: Vec<String>,
    pub mandatory: bool,
    pub review_by: Option<String>,
    pub status: DocStatus,
    pub validation: serde_json::Value,
    pub approval_id: Option<String>,
    pub chunks: i64,
    pub created_by: String,
    pub created_at: String,
}

impl DocumentMeta {
    pub fn citation_base(&self) -> String {
        citation_base(&self.source, &self.subject, self.version)
    }
}

pub fn citation_base(source: &str, subject: &str, version: i64) -> String {
    format!("{source}/{subject}@v{version}")
}

/// Who is retrieving, and therefore what is visible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalContext {
    pub tenant: TenantId,
    pub project: Option<ProjectId>,
    pub domain: Option<String>,
    pub teams: Vec<String>,
    pub workflow: Option<WorkflowInstanceId>,
    pub max_classification: Classification,
    pub principal_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Lexical,
    Vector,
    #[default]
    Hybrid,
}

impl SearchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchMode::Lexical => "lexical",
            SearchMode::Vector => "vector",
            SearchMode::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    #[serde(default)]
    pub mode: SearchMode,
    #[serde(default = "default_k")]
    pub k: usize,
    /// Restrict to these source kinds (empty = all).
    #[serde(default)]
    pub kinds: Vec<SourceKind>,
    #[serde(default)]
    pub include_stale: bool,
}

fn default_k() -> usize {
    5
}

impl SearchQuery {
    pub fn new(text: &str, mode: SearchMode, k: usize) -> Self {
        Self {
            text: text.into(),
            mode,
            k,
            kinds: vec![],
            include_stale: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub citation: String,
    pub chunk_id: String,
    pub document_id: String,
    pub source: String,
    pub source_kind: SourceKind,
    pub subject: String,
    pub version: i64,
    pub title: String,
    pub heading: String,
    pub text: String,
    pub content_hash: String,
    pub classification: Classification,
    pub mandatory: bool,
    pub stale: bool,
    pub conflict: bool,
    pub score: f64,
}
