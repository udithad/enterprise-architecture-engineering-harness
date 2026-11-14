//! EAEH harness runtime extensibility (Layer 3). Three distinct mechanisms (ADR-0005):
//!
//! | Mechanism | Purpose | Can affect outcome? |
//! |---|---|---|
//! | [`hooks`] | React to named lifecycle points | Only by *vetoing* at `Before*` points |
//! | [`middleware`] | Ordered around-processing of a request pipeline | Yes (may short-circuit / error) |
//! | [`interceptor`] | Inspect/transform a value crossing a boundary | Transforms data only |
//!
//! [`plugin`] provides manifests, integrity verification, lifecycle and registry.
//! Phase 1 executes only trusted in-process extensions (D-03); manifest signatures
//! establish provenance, **not** runtime isolation (clarification 1).
//!
//! Mandatory security controls are never implemented as hooks/middleware: callers run
//! authorization, policy, approval and budget checks *before* dispatching hooks.

pub mod hooks;
pub mod interceptor;
pub mod middleware;
pub mod plugin;

pub use hooks::{
    FailurePolicy, Hook, HookContext, HookDispatchReport, HookOutcome, HookPayload, HookPoint,
    HookRegistration, HookRegistry,
};
pub use interceptor::{Interceptor, InterceptorChain};
pub use middleware::{Endpoint, Middleware, Next, Pipeline};
pub use plugin::{PluginCapability, PluginKind, PluginManifest, PluginRegistry, PluginState, TrustPolicy};
