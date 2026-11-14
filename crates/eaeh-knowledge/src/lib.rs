//! EAEH enterprise knowledge management (Phase 2, master §17).
//!
//! * Governed sources and immutable, versioned documents (Markdown, text, JSON,
//!   OpenAPI JSON) with scope, classification, tags, review-by date and mandatory flag.
//! * Validation gate on ingestion: documents carrying credential material are
//!   **quarantined** (content not stored, never indexed, never retrievable; metadata,
//!   hash and findings are kept for the steward).
//! * Lifecycle: quarantined | pending_approval → active → superseded | retired → deleted
//!   (purge). Documents of `standard` sources require human approval bound to the
//!   content hash before they become active.
//! * Indexing: SQLite FTS5 (BM25) + stored vectors from an [`eaeh_intelligence::EmbeddingProvider`].
//! * Retrieval: lexical, vector, hybrid (RRF), deterministic reranking; every query is
//!   filtered by tenant, scope (enterprise/domain/project/team/workflow), status and
//!   classification ceiling, and audited. FTS query syntax cannot be injected.
//! * Mandatory enterprise standards enter agent context as `enterprise_policy`
//!   instructions (this crate is the only producer, fitness-enforced); everything else
//!   enters as untrusted, cited data.

pub mod model;
pub mod parse;
pub mod search;
pub mod service;
pub mod tool;

pub use model::*;
pub use service::{Conflict, KnowledgeConfig, KnowledgeService};
pub use tool::{KnowledgeReadTool, KnowledgeSearchTool};
