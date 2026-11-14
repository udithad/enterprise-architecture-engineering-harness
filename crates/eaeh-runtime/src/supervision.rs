//! Execution supervision: stalled-task and repeated-failure detection with escalation
//! to humans (workflow status `escalated`, audit record, `SupervisionEscalated` event).

use chrono::Duration;
use eaeh_core::{Result, TraceContext};
use eaeh_events::catalog::SupervisionEscalated;
use eaeh_events::{EventEnvelope, outbox};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, sqlx, ts};
use serde_json::json;

#[derive(Debug, Clone)]
pub struct Supervisor {
    db: Db,
    stall_after: Duration,
    max_failed_tasks: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupervisionReport {
    pub stalled_tasks: Vec<String>,
    pub escalated_instances: Vec<String>,
}

impl Supervisor {
    pub fn new(db: Db, stall_after: Duration, max_failed_tasks: u32) -> Self {
        Self {
            db,
            stall_after,
            max_failed_tasks,
        }
    }

    pub async fn scan_once(&self) -> Result<SupervisionReport> {
        let now = self.db.now();
        let stale: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, workflow_instance_id FROM tasks WHERE status = 'running' AND heartbeat_at < ?",
        )
        .bind(ts(now - self.stall_after))
        .fetch_all(self.db.pool())
        .await
        .db()?;
        let failing: Vec<(String, i64)> = sqlx::query_as(
            "SELECT t.workflow_instance_id, COUNT(*) FROM tasks t JOIN workflow_instances w ON w.id = t.workflow_instance_id \
             WHERE t.status = 'failed' AND w.status = 'running' GROUP BY t.workflow_instance_id HAVING COUNT(*) >= ?",
        )
        .bind(i64::from(self.max_failed_tasks))
        .fetch_all(self.db.pool())
        .await
        .db()?;
        let mut report = SupervisionReport::default();
        for (task, instance) in stale {
            self.escalate(
                Some(&task),
                instance.as_deref(),
                "stalled: no heartbeat within threshold",
            )
            .await?;
            report.stalled_tasks.push(task);
            if let Some(i) = instance {
                report.escalated_instances.push(i);
            }
        }
        for (instance, n) in failing {
            self.escalate(
                None,
                Some(&instance),
                &format!("repeated failures: {n} failed tasks"),
            )
            .await?;
            report.escalated_instances.push(instance);
        }
        Ok(report)
    }

    async fn escalate(&self, task: Option<&str>, instance: Option<&str>, reason: &str) -> Result<()> {
        let mut tx = self.db.write().await?;
        if let Some(t) = task {
            sqlx::query("UPDATE tasks SET status = 'escalated', error = ?, updated_at = ? WHERE id = ?")
                .bind(reason)
                .bind(ts(self.db.now()))
                .bind(t)
                .execute(tx.conn())
                .await
                .db()?;
        }
        if let Some(i) = instance {
            sqlx::query("UPDATE workflow_instances SET status = 'escalated', version = version + 1, updated_at = ? WHERE id = ? AND status = 'running'")
                .bind(ts(self.db.now()))
                .bind(i)
                .execute(tx.conn())
                .await
                .db()?;
        }
        let subject = task.or(instance).unwrap_or("unknown").to_string();
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    "supervisor",
                    "supervision.escalate",
                    "task",
                    subject.clone(),
                    AuditOutcome::Recorded,
                )
                .details(json!({"task": task, "workflow_instance": instance, "reason": reason})),
            )
            .await?;
        let ev = EventEnvelope::new(
            &SupervisionEscalated {
                subject_id: subject,
                reason: reason.into(),
            },
            "supervisor",
            None,
            TraceContext::new_root(),
            self.db.now(),
        )?;
        outbox::enqueue_in(&mut tx, &ev).await?;
        tx.commit().await?;
        tracing::warn!(?task, ?instance, %reason, "escalated to human intervention");
        Ok(())
    }
}
