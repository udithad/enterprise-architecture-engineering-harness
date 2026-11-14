//! EAEH event management (ADR-0005).
//!
//! * [`EventEnvelope`] – versioned, typed envelope with correlation and causation ids.
//! * [`catalog`] – typed domain events implementing [`DomainEvent`].
//! * [`outbox`] – events are written in the *same transaction* as the state change that
//!   produced them (transactional outbox), so an event exists iff the change committed.
//!   Duplicate publication of the same `event_id` is ignored.
//! * [`dispatcher`] – at-least-once delivery to in-process subscribers with per-consumer
//!   delivery tracking, bounded retry with exponential backoff + jitter, dead-lettering,
//!   and optional per-consumer ordering.
//!
//! No external broker is introduced in Phase 1 (master instructions §8).

pub mod catalog;
pub mod dispatcher;
pub mod outbox;

use chrono::{DateTime, Utc};
use eaeh_core::{CorrelationId, EventId, ProjectId, Scope, TenantId, TraceContext};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use uuid::Uuid;

pub use dispatcher::{DispatchStats, EventDispatcher, EventSubscriber, RetryPolicy};

/// A typed event. `TYPE` + `VERSION` form the schema identity.
pub trait DomainEvent: Serialize + DeserializeOwned {
    const TYPE: &'static str;
    const VERSION: u16;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: EventId,
    pub event_type: String,
    pub schema_version: u16,
    pub occurred_at: DateTime<Utc>,
    pub tenant: Option<TenantId>,
    pub project: Option<ProjectId>,
    pub correlation: CorrelationId,
    pub causation: Option<Uuid>,
    pub producer: String,
    pub payload: Value,
}

impl EventEnvelope {
    pub fn new<E: DomainEvent>(
        event: &E,
        producer: &str,
        scope: Option<Scope>,
        trace: TraceContext,
        at: DateTime<Utc>,
    ) -> eaeh_core::Result<Self> {
        Ok(Self {
            event_id: EventId::new(),
            event_type: E::TYPE.to_string(),
            schema_version: E::VERSION,
            occurred_at: at,
            tenant: scope.map(|s| s.tenant),
            project: scope.map(|s| s.project),
            correlation: trace.correlation,
            causation: trace.causation,
            producer: producer.to_string(),
            payload: serde_json::to_value(event)
                .map_err(|e| eaeh_core::HarnessError::internal(format!("event serialize: {e}")))?,
        })
    }

    /// Decode the payload as a specific event type, checking type and schema version.
    pub fn decode<E: DomainEvent>(&self) -> eaeh_core::Result<E> {
        if self.event_type != E::TYPE || self.schema_version != E::VERSION {
            return Err(eaeh_core::HarnessError::invalid_input(format!(
                "event {}@{} is not {}@{}",
                self.event_type,
                self.schema_version,
                E::TYPE,
                E::VERSION
            )));
        }
        Ok(serde_json::from_value(self.payload.clone())?)
    }

    /// Trace context for work caused by this event.
    pub fn trace_for_consequences(&self) -> TraceContext {
        TraceContext {
            correlation: self.correlation,
            causation: Some(self.event_id.as_uuid()),
        }
    }
}
