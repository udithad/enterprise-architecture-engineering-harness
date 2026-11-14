//! EAEH intelligent-execution foundations (Layer 2): models, prompts, context.
//!
//! * [`model`] – provider-independent model abstraction, registry, task-based routing
//!   (cheapest model satisfying requirements, never "most expensive by default"),
//!   provider health/circuit breaking and fallback.
//! * [`client`] – the governed model client: budgets, LLM hooks, usage/cost/version
//!   recording, `ModelInvoked` events. Model output is returned as `Untrusted`.
//! * [`prompt`] – versioned prompt templates with typed variables, approval and rollback.
//! * [`context`] – context assembly with explicit trust precedence, provenance, scope
//!   filtering, token budgets, conflict detection, untrusted-data fencing and snapshots.
//!
//! No production model provider is included in Phase 1 (D-04). Providers are registered
//! by the composition root; tests use the scripted provider in `eaeh-testkit`.

pub mod client;
pub mod context;
pub mod embedding;
pub mod model;
pub mod prompt;
pub mod replay;

pub use client::{EXTERNAL_AI_CLIENT, ModelCall, ModelClient, ModelOutcome};
pub use context::{AssembledContext, ContextAssembler, ContextItem, MemoryKind, TrustLevel};
pub use embedding::{EmbeddingProvider, HashedNgramEmbedder};
pub use model::{
    Egress, Message, MessageRole, ModelDescriptor, ModelProvider, ModelRegistry, ModelRequest, ModelResponse,
    ProposedToolCall, RoutingRequirements, ToolSpec, Usage,
};
pub use prompt::{PromptRegistry, PromptStatus, PromptTemplate, RenderedPrompt, TrustedText};

/// Heuristic token estimate (≈4 characters per token). Documented approximation used
/// for budgeting before a provider reports real usage.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}
