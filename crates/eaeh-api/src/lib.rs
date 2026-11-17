//! EAEH HTTP API v1 (master §26).
//!
//! * Authentication: `Authorization: Bearer eaeh_…` on every route except
//!   `GET /v1/health`. Tokens are resolved by `IdentityService`.
//! * Authorization: enforced in the governed services each handler calls (workflow
//!   engine, approval manager, identity, plugin registry) plus explicit checks for
//!   read-only listings — never only in the UI.
//! * Errors: `{"error": {"code", "message", "request_id"}}` with stable codes; internal
//!   error details are not leaked.
//! * Pagination: `limit` (1–100, default 50) and opaque `cursor`; responses carry
//!   `next_cursor`.
//! * Per-principal rate limiting and request body size limits.

mod error;
mod ops;
pub mod portal;
mod routes;
mod views;

pub use error::ApiError;
pub use routes::{AppState, router};
