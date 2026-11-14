//! EAEH deterministic control and governance (Layer 4).
//!
//! Nothing in this crate calls a model, a hook or a plugin: every decision here is a
//! pure function of stored state and the request (ADR-0002 forbids dependencies on
//! intelligence/extensibility crates, enforced by the architecture fitness test).

pub mod approval;
pub mod authz;
pub mod budget;
pub mod identity;
pub mod policy;

pub use approval::{
    ApprovalDecision, ApprovalManager, ApprovalRecord, ApprovalRequest, ApprovalStatus, ApprovalSubject,
};
pub use authz::{Action, Authorizer, RbacAuthorizer, Target};
pub use budget::{BudgetLedger, BudgetLimits, BudgetResource};
pub use identity::{IdentityService, IssuedToken, NewPrincipal, TokenInfo};
pub use policy::{Effect, PolicyDecision, PolicyDocument, PolicyEngine, PolicyRequest, RuleEngine};
