//! At-least-once event dispatcher.
//!
//! Delivery state is tracked per (event, consumer) in `event_deliveries`:
//! `pending → done` on success, `pending → dead` after `max_attempts` failures
//! (dead-letter). A `done` delivery is never re-delivered, so duplicate dispatch passes do
//! not duplicate side effects; a crash between handler success and the `done` write can
//! cause one redelivery, so consumers with external side effects must use idempotency
//! keys derived from `event_id` (see `eaeh_store::idempotency`).

use crate::{EventEnvelope, outbox};
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use eaeh_core::{HarnessError, Result};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, sqlx, ts};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[async_trait]
pub trait EventSubscriber: Send + Sync {
    /// Stable consumer name (delivery tracking key).
    fn name(&self) -> &str;
    fn accepts(&self, event_type: &str) -> bool;
    /// If true, a failing event blocks later events for this consumer until it is
    /// delivered or dead-lettered (per-consumer ordering).
    fn ordered(&self) -> bool {
        false
    }
    async fn handle(&self, event: &EventEnvelope) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub handler_timeout: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(60),
            handler_timeout: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// Exponential backoff with full jitter in [delay/2, delay].
    pub fn backoff(&self, attempt: u32) -> Duration {
        let exp = self
            .base_delay
            .saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1).min(16)));
        let capped = exp.min(self.max_delay);
        let jitter_unit = eaeh_core::hash::random_bytes::<2>()
            .map(|b| f64::from(u16::from_le_bytes(b)) / f64::from(u16::MAX))
            .unwrap_or(0.5);
        capped.mul_f64(0.5 + 0.5 * jitter_unit)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DispatchStats {
    pub delivered: u32,
    pub failed: u32,
    pub dead_lettered: u32,
    pub skipped: u32,
    pub events_completed: u32,
}

pub struct EventDispatcher {
    db: Db,
    subscribers: RwLock<Vec<Arc<dyn EventSubscriber>>>,
    retry: RetryPolicy,
}

impl std::fmt::Debug for EventDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventDispatcher")
            .field("retry", &self.retry)
            .finish_non_exhaustive()
    }
}

impl EventDispatcher {
    pub fn new(db: Db, retry: RetryPolicy) -> Self {
        Self {
            db,
            subscribers: RwLock::new(Vec::new()),
            retry,
        }
    }

    pub fn subscribe(&self, sub: Arc<dyn EventSubscriber>) -> Result<()> {
        let mut subs = self
            .subscribers
            .write()
            .map_err(|_| HarnessError::internal("subscriber lock poisoned"))?;
        if subs.iter().any(|s| s.name() == sub.name()) {
            return Err(HarnessError::conflict(format!(
                "subscriber '{}' already registered",
                sub.name()
            )));
        }
        subs.push(sub);
        Ok(())
    }

    fn snapshot(&self) -> Vec<Arc<dyn EventSubscriber>> {
        self.subscribers.read().map(|s| s.clone()).unwrap_or_default()
    }

    /// Process up to `limit` pending outbox events once.
    pub async fn dispatch_pending(&self, limit: u32) -> Result<DispatchStats> {
        let subs = self.snapshot();
        let events = outbox::pending(&self.db, limit).await?;
        let mut stats = DispatchStats::default();
        let mut blocked: HashSet<String> = HashSet::new();

        for ev in &events {
            let mut all_final = true;
            for sub in subs.iter().filter(|s| s.accepts(&ev.event_type)) {
                let consumer = sub.name().to_string();
                if blocked.contains(&consumer) {
                    all_final = false;
                    stats.skipped += 1;
                    continue;
                }
                let (status, attempts, next_at) = self.delivery_state(ev, &consumer).await?;
                match status.as_str() {
                    "done" | "dead" => continue,
                    _ => {}
                }
                if next_at > self.db.now() {
                    all_final = false;
                    stats.skipped += 1;
                    if sub.ordered() {
                        blocked.insert(consumer);
                    }
                    continue;
                }
                let res = match tokio::time::timeout(self.retry.handler_timeout, sub.handle(ev)).await {
                    Ok(r) => r,
                    Err(_) => Err(HarnessError::timeout("event handler timed out")),
                };
                match res {
                    Ok(()) => {
                        self.set_delivery(ev, &consumer, "done", attempts + 1, None, None)
                            .await?;
                        stats.delivered += 1;
                    }
                    Err(e) => {
                        let attempt = attempts + 1;
                        if attempt >= self.retry.max_attempts {
                            self.dead_letter(ev, &consumer, attempt, &e).await?;
                            stats.dead_lettered += 1;
                        } else {
                            let delay = ChronoDuration::from_std(self.retry.backoff(attempt))
                                .unwrap_or_else(|_| ChronoDuration::seconds(1));
                            self.set_delivery(
                                ev,
                                &consumer,
                                "pending",
                                attempt,
                                Some(delay),
                                Some(&e.to_string()),
                            )
                            .await?;
                            stats.failed += 1;
                            all_final = false;
                            if sub.ordered() {
                                blocked.insert(consumer);
                            }
                        }
                        tracing::warn!(event_id = %ev.event_id, consumer = %sub.name(), attempt, error = %e, "event delivery failed");
                    }
                }
            }
            if all_final {
                let mut tx = self.db.write().await?;
                sqlx::query("UPDATE outbox SET status = 'dispatched' WHERE event_id = ?")
                    .bind(ev.event_id.to_string())
                    .execute(tx.conn())
                    .await
                    .db()?;
                tx.commit().await?;
                stats.events_completed += 1;
            }
        }
        Ok(stats)
    }

    /// Repeatedly dispatch until no immediately-deliverable work remains (tests, shutdown).
    pub async fn drain(&self, max_passes: u32) -> Result<DispatchStats> {
        let mut total = DispatchStats::default();
        for _ in 0..max_passes {
            let s = self.dispatch_pending(500).await?;
            total.delivered += s.delivered;
            total.failed += s.failed;
            total.dead_lettered += s.dead_lettered;
            total.skipped += s.skipped;
            total.events_completed += s.events_completed;
            if s.delivered == 0 && s.failed == 0 && s.dead_lettered == 0 && s.events_completed == 0 {
                break;
            }
        }
        Ok(total)
    }

    async fn delivery_state(
        &self,
        ev: &EventEnvelope,
        consumer: &str,
    ) -> Result<(String, u32, chrono::DateTime<chrono::Utc>)> {
        let row: Option<(String, i64, String)> = sqlx::query_as(
            "SELECT status, attempts, next_attempt_at FROM event_deliveries WHERE event_id = ? AND consumer = ?",
        )
        .bind(ev.event_id.to_string())
        .bind(consumer)
        .fetch_optional(self.db.pool())
        .await
        .db()?;
        match row {
            Some((status, attempts, next)) => Ok((
                status,
                u32::try_from(attempts).unwrap_or(u32::MAX),
                eaeh_store::parse_ts(&next)?,
            )),
            None => Ok(("pending".into(), 0, self.db.now())),
        }
    }

    async fn set_delivery(
        &self,
        ev: &EventEnvelope,
        consumer: &str,
        status: &str,
        attempts: u32,
        delay: Option<ChronoDuration>,
        error: Option<&str>,
    ) -> Result<()> {
        let now = self.db.now();
        let next = now + delay.unwrap_or_else(ChronoDuration::zero);
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO event_deliveries (event_id, consumer, status, attempts, next_attempt_at, last_error, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(event_id, consumer) DO UPDATE SET status = excluded.status, attempts = excluded.attempts, \
             next_attempt_at = excluded.next_attempt_at, last_error = excluded.last_error, updated_at = excluded.updated_at",
        )
        .bind(ev.event_id.to_string())
        .bind(consumer)
        .bind(status)
        .bind(i64::from(attempts))
        .bind(ts(next))
        .bind(error.map(|e| self.db.redactor().redact_str(e)))
        .bind(ts(now))
        .execute(tx.conn())
        .await
        .db()?;
        tx.commit().await
    }

    async fn dead_letter(
        &self,
        ev: &EventEnvelope,
        consumer: &str,
        attempts: u32,
        err: &HarnessError,
    ) -> Result<()> {
        self.set_delivery(ev, consumer, "dead", attempts, None, Some(&err.to_string()))
            .await?;
        self.db
            .audit(
                AuditEntry::system(
                    "event-dispatcher",
                    "event.dead_lettered",
                    "event",
                    ev.event_id.to_string(),
                    AuditOutcome::Failed,
                )
                .correlation(ev.correlation)
                .details(serde_json::json!({
                    "consumer": consumer,
                    "event_type": ev.event_type,
                    "attempts": attempts,
                    "error": err.to_string(),
                })),
            )
            .await?;
        Ok(())
    }

    /// Dead-lettered deliveries: (event_id, consumer, last_error).
    pub async fn dead_letters(&self) -> Result<Vec<(String, String, Option<String>)>> {
        sqlx::query_as("SELECT event_id, consumer, last_error FROM event_deliveries WHERE status = 'dead' ORDER BY updated_at")
            .fetch_all(self.db.pool())
            .await
            .db()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::TaskAssigned;
    use crate::outbox::enqueue;
    use eaeh_core::TraceContext;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Recorder {
        name: String,
        fail_times: AtomicU32,
        seen: Mutex<Vec<String>>,
        ordered: bool,
    }

    impl Recorder {
        fn new(name: &str, fail_times: u32, ordered: bool) -> Arc<Self> {
            Arc::new(Self {
                name: name.into(),
                fail_times: AtomicU32::new(fail_times),
                seen: Mutex::new(vec![]),
                ordered,
            })
        }
    }

    #[async_trait]
    impl EventSubscriber for Recorder {
        fn name(&self) -> &str {
            &self.name
        }
        fn accepts(&self, t: &str) -> bool {
            t == "task.assigned"
        }
        fn ordered(&self) -> bool {
            self.ordered
        }
        async fn handle(&self, e: &EventEnvelope) -> Result<()> {
            if self.fail_times.load(Ordering::SeqCst) > 0 {
                self.fail_times.fetch_sub(1, Ordering::SeqCst);
                return Err(HarnessError::unavailable("transient"));
            }
            let t: TaskAssigned = e.decode()?;
            self.seen.lock().unwrap().push(t.task_id);
            Ok(())
        }
    }

    fn fast_retry(max: u32) -> RetryPolicy {
        RetryPolicy {
            max_attempts: max,
            base_delay: Duration::from_millis(0),
            max_delay: Duration::from_millis(0),
            handler_timeout: Duration::from_secs(1),
        }
    }

    async fn setup() -> (tempfile::TempDir, Db) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("e.db")).await.unwrap();
        (d, db)
    }

    fn ev(db: &Db, id: &str) -> EventEnvelope {
        EventEnvelope::new(
            &TaskAssigned {
                task_id: id.into(),
                agent_instance_id: None,
                kind: "k".into(),
            },
            "test",
            None,
            TraceContext::new_root(),
            db.now(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn duplicate_publish_and_redispatch_deliver_once() {
        let (_d, db) = setup().await;
        let disp = EventDispatcher::new(db.clone(), fast_retry(3));
        let rec = Recorder::new("r", 0, false);
        disp.subscribe(rec.clone()).unwrap();
        let e = ev(&db, "t1");
        assert!(enqueue(&db, &e).await.unwrap());
        assert!(!enqueue(&db, &e).await.unwrap(), "same event id must be ignored");
        disp.drain(5).await.unwrap();
        disp.drain(5).await.unwrap();
        assert_eq!(rec.seen.lock().unwrap().as_slice(), ["t1"]);
    }

    #[tokio::test]
    async fn transient_failures_are_retried_then_delivered() {
        let (_d, db) = setup().await;
        let disp = EventDispatcher::new(db.clone(), fast_retry(5));
        let rec = Recorder::new("r", 2, false);
        disp.subscribe(rec.clone()).unwrap();
        enqueue(&db, &ev(&db, "t1")).await.unwrap();
        let s = disp.drain(10).await.unwrap();
        assert_eq!(s.failed, 2);
        assert_eq!(s.delivered, 1);
        assert_eq!(rec.seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn persistent_failure_is_dead_lettered_and_audited() {
        let (_d, db) = setup().await;
        let disp = EventDispatcher::new(db.clone(), fast_retry(3));
        disp.subscribe(Recorder::new("r", 100, false)).unwrap();
        enqueue(&db, &ev(&db, "t1")).await.unwrap();
        let s = disp.drain(10).await.unwrap();
        assert_eq!(s.dead_lettered, 1);
        assert_eq!(disp.dead_letters().await.unwrap().len(), 1);
        let audit = db.audit_records(None, 0, 10).await.unwrap();
        assert!(audit.iter().any(|a| a.action == "event.dead_lettered"));
        // Dead-lettered events are complete; nothing remains pending.
        assert_eq!(disp.dispatch_pending(10).await.unwrap(), DispatchStats::default());
    }

    #[tokio::test]
    async fn ordered_consumer_does_not_skip_ahead() {
        let (_d, db) = setup().await;
        let disp = EventDispatcher::new(db.clone(), fast_retry(5));
        let rec = Recorder::new("ordered", 1, true);
        disp.subscribe(rec.clone()).unwrap();
        enqueue(&db, &ev(&db, "first")).await.unwrap();
        enqueue(&db, &ev(&db, "second")).await.unwrap();
        // Pass 1: "first" fails, "second" must be held back.
        disp.dispatch_pending(10).await.unwrap();
        assert!(rec.seen.lock().unwrap().is_empty());
        disp.drain(5).await.unwrap();
        assert_eq!(rec.seen.lock().unwrap().as_slice(), ["first", "second"]);
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        let p = RetryPolicy {
            max_attempts: 10,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(1000),
            handler_timeout: Duration::from_secs(1),
        };
        assert!(p.backoff(1) <= Duration::from_millis(100));
        assert!(p.backoff(1) >= Duration::from_millis(50));
        assert!(p.backoff(10) <= Duration::from_millis(1000));
        assert!(p.backoff(10) >= Duration::from_millis(500));
    }
}
