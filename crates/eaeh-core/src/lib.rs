//! EAEH core primitives.
//!
//! This crate is the bottom of the dependency graph (see ADR-0002). It contains only
//! types and pure functions shared by every other crate: typed identifiers, the error
//! taxonomy, identities and scopes, risk/autonomy classification, secret handling,
//! trace context and time abstraction. It performs no I/O apart from reading the
//! system clock, OS randomness and (for [`secret::EnvSecretsProvider`]) environment variables.

pub mod classification;
pub mod clock;
pub mod error;
pub mod hash;
pub mod ids;
pub mod principal;
pub mod risk;
pub mod scope;
pub mod secret;
pub mod trace;
pub mod untrusted;

pub use classification::Classification;
pub use error::{ErrorKind, HarnessError, Result};
pub use ids::*;
pub use principal::{Principal, PrincipalKind, ProjectAccess, Role};
pub use risk::{AutonomyLevel, RiskClass};
pub use scope::Scope;
pub use secret::{Redactor, SecretString};
pub use trace::TraceContext;
pub use untrusted::Untrusted;
