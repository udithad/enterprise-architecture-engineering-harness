//! EAEH tool registry and governed tool gateway (master §13).
//!
//! Agents (and plugins) never reach system capabilities directly: every invocation goes
//! through [`ToolGateway::invoke`], which applies, in order, the deterministic controls
//! (definition allowlist → authorization → autonomy ceiling → input contract → policy →
//! approval → isolation availability → rate limit → budget) **before** any hook,
//! middleware or tool code runs. See `docs/architecture/agents-and-tools.md`.

pub mod builtin;
pub mod gateway;
pub mod registry;

pub use gateway::{ToolGateway, ToolRequest, ToolResponse};
pub use registry::{Tool, ToolContext, ToolDescriptor, ToolRegistry};
