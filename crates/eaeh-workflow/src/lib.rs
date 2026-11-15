//! EAEH deterministic workflow engine and hybrid orchestration (master §3.1, §24).
//!
//! The engine is the **authoritative** half of hybrid orchestration: it owns lifecycle
//! state, transitions, approval gates, retries, timeouts, cancellation, durable
//! checkpoints and recovery. Intelligent work is delegated through two traits —
//! [`TaskExecutor`] (agents/tools) and [`Planner`] (task decomposition) — whose outputs
//! the engine validates before acting on them. This crate deliberately does not depend
//! on `eaeh-intelligence` or `eaeh-agents` (architecture fitness test).

pub mod definition;
pub mod engine;

pub use definition::{
    Outcome, PlanProposal, PlanStep, RetryPolicy, StateDef, StateKind, Trigger, WorkflowDefinition,
};
pub use engine::{
    ActivityItem, BlockedGate, GateCheck, GateDocument, InstanceStatus, Planner, PlanningContext,
    SyncOutcome, TaskExecution, TaskExecutor, TaskOutput, TaskSpec, TaskSummary, WorkflowData,
    WorkflowEngine, WorkflowInstance,
};
