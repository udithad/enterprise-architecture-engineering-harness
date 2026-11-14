//! EAEH agent runtime and management (master §10–§12).
//!
//! * [`definition`] – typed, versioned agent definitions (capabilities, allowed tools,
//!   model preferences, I/O schemas, autonomy, delegation policy, budgets).
//! * [`registry`] – persisted, immutable-per-version registry with capability discovery
//!   preferring the *smallest effective* agent.
//! * [`runtime`] – dynamic instantiation, governed execution loop (context → model →
//!   gateway-mediated tool calls → validated output), bounded correction, loop detection,
//!   termination, and controlled delegation (authorization, allowlist, depth, cycle,
//!   fan-out and duplicate-task checks).

pub mod definition;
pub mod external;
pub mod registry;
pub mod runtime;

pub use definition::{AgentDefinition, DelegationPolicy, ModelPreferences};
pub use external::{ExecutionMode, ExternalSubmission, ExternalTaskView};
pub use registry::{AgentRegistry, CapabilityQuery};
pub use runtime::{
    AgentInstance, AgentResult, AgentRuntime, AgentTask, KnowledgeIntegration, validate_citations,
};
