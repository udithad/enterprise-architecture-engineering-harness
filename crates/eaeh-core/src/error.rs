//! Platform-wide error taxonomy.
//!
//! Every crate returns [`HarnessError`]. The [`ErrorKind`] is the stable, machine-readable
//! classification used for failure handling (retry decisions, API status codes, audit outcome).

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Input failed validation (schema, format, constraint).
    InvalidInput,
    /// No valid credentials were presented.
    Unauthenticated,
    /// Authenticated principal is not allowed to perform the action.
    Forbidden,
    /// A deterministic policy denied the operation.
    PolicyDenied,
    /// The operation requires a human approval that has not been granted.
    ApprovalRequired,
    NotFound,
    /// Optimistic concurrency, uniqueness or lock conflict.
    Conflict,
    /// An execution budget would be exceeded.
    BudgetExceeded,
    Timeout,
    Cancelled,
    /// A hook with a blocking failure policy vetoed or failed.
    HookBlocked,
    /// Execution isolation could not be provided or failed.
    Isolation,
    /// The requested capability is intentionally not supported in this build/phase.
    NotSupported,
    /// A dependency is temporarily unavailable; the operation may be retried.
    Unavailable,
    /// Loop, repeated failure or stall detected by supervision.
    SupervisionHalt,
    /// Stored data failed an integrity check.
    Integrity,
    /// The work was handed off to an external AI client and waits for its result
    /// (`external_ai` execution mode). Not a failure: the workflow suspends durably.
    AwaitingExternal,
    Internal,
}

impl ErrorKind {
    /// Whether an operation that failed with this kind may be retried automatically.
    pub fn is_retryable(self) -> bool {
        matches!(self, ErrorKind::Unavailable | ErrorKind::Timeout)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::InvalidInput => "invalid_input",
            ErrorKind::Unauthenticated => "unauthenticated",
            ErrorKind::Forbidden => "forbidden",
            ErrorKind::PolicyDenied => "policy_denied",
            ErrorKind::ApprovalRequired => "approval_required",
            ErrorKind::NotFound => "not_found",
            ErrorKind::Conflict => "conflict",
            ErrorKind::BudgetExceeded => "budget_exceeded",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::HookBlocked => "hook_blocked",
            ErrorKind::Isolation => "isolation",
            ErrorKind::NotSupported => "not_supported",
            ErrorKind::Unavailable => "unavailable",
            ErrorKind::SupervisionHalt => "supervision_halt",
            ErrorKind::Integrity => "integrity",
            ErrorKind::AwaitingExternal => "awaiting_external",
            ErrorKind::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("{kind}: {message}")]
pub struct HarnessError {
    pub kind: ErrorKind,
    pub message: String,
}

pub type Result<T, E = HarnessError> = std::result::Result<T, E>;

macro_rules! ctor {
    ($($fn:ident => $kind:ident),* $(,)?) => {
        $(
            pub fn $fn(message: impl Into<String>) -> Self {
                Self::new(ErrorKind::$kind, message)
            }
        )*
    };
}

impl HarnessError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    ctor! {
        invalid_input => InvalidInput,
        unauthenticated => Unauthenticated,
        forbidden => Forbidden,
        policy_denied => PolicyDenied,
        approval_required => ApprovalRequired,
        not_found => NotFound,
        conflict => Conflict,
        budget_exceeded => BudgetExceeded,
        timeout => Timeout,
        cancelled => Cancelled,
        hook_blocked => HookBlocked,
        isolation => Isolation,
        not_supported => NotSupported,
        unavailable => Unavailable,
        supervision_halt => SupervisionHalt,
        integrity => Integrity,
        awaiting_external => AwaitingExternal,
        internal => Internal,
    }

    pub fn is_retryable(&self) -> bool {
        self.kind.is_retryable()
    }

    /// Prefix the message with additional context, keeping the kind.
    pub fn context(mut self, ctx: impl fmt::Display) -> Self {
        self.message = format!("{ctx}: {}", self.message);
        self
    }
}

impl From<serde_json::Error> for HarnessError {
    fn from(e: serde_json::Error) -> Self {
        HarnessError::invalid_input(format!("json: {e}"))
    }
}

impl From<std::io::Error> for HarnessError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => HarnessError::not_found(e.to_string()),
            std::io::ErrorKind::PermissionDenied => HarnessError::forbidden(e.to_string()),
            std::io::ErrorKind::AlreadyExists => HarnessError::conflict(e.to_string()),
            std::io::ErrorKind::TimedOut => HarnessError::timeout(e.to_string()),
            _ => HarnessError::internal(format!("io: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryability_is_limited_to_transient_kinds() {
        assert!(ErrorKind::Unavailable.is_retryable());
        assert!(ErrorKind::Timeout.is_retryable());
        assert!(!ErrorKind::Forbidden.is_retryable());
        assert!(!ErrorKind::PolicyDenied.is_retryable());
        assert!(!ErrorKind::BudgetExceeded.is_retryable());
    }

    #[test]
    fn context_preserves_kind() {
        let e = HarnessError::forbidden("x").context("tool fs.write");
        assert_eq!(e.kind, ErrorKind::Forbidden);
        assert_eq!(e.message, "tool fs.write: x");
    }
}
