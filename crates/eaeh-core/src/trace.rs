use crate::ids::CorrelationId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Correlation/causation propagated through workflows, agents, tools, events and audit.
/// `causation` is the id of the immediate cause (an event, task, invocation or request).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceContext {
    pub correlation: CorrelationId,
    pub causation: Option<Uuid>,
}

impl TraceContext {
    pub fn new_root() -> Self {
        Self {
            correlation: CorrelationId::new(),
            causation: None,
        }
    }

    /// Same correlation, with `cause` as the new causation id.
    pub fn caused_by(&self, cause: Uuid) -> Self {
        Self {
            correlation: self.correlation,
            causation: Some(cause),
        }
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new_root()
    }
}
