//! Append-only, hash-chained audit log.
//!
//! `hash_n = SHA-256(canonical_json({seq, id, occurred_at, …, details, prev_hash}))` with
//! `prev_hash_1 = 64 × '0'`. Sequence numbers are gapless; modification, deletion or
//! re-ordering of any record breaks verification from that record onward.
//! Details are redacted by the shared [`eaeh_core::Redactor`] before hashing and storage.

use crate::{Db, DbResultExt, WriteTx, ts};
use eaeh_core::hash::sha256_hex;
use eaeh_core::{AuditId, CorrelationId, Principal, ProjectId, Result, Scope, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Allowed,
    Denied,
    Succeeded,
    Failed,
    Requested,
    Recorded,
}

impl AuditOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditOutcome::Allowed => "allowed",
            AuditOutcome::Denied => "denied",
            AuditOutcome::Succeeded => "succeeded",
            AuditOutcome::Failed => "failed",
            AuditOutcome::Requested => "requested",
            AuditOutcome::Recorded => "recorded",
        }
    }
}

/// An audit entry to append. Construct with [`AuditEntry::new`] and builder methods.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub tenant: Option<TenantId>,
    pub project: Option<ProjectId>,
    pub actor_id: String,
    pub actor_kind: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub outcome: AuditOutcome,
    pub correlation: Option<CorrelationId>,
    pub details: Value,
}

impl AuditEntry {
    pub fn new(
        actor: &Principal,
        action: impl Into<String>,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        outcome: AuditOutcome,
    ) -> Self {
        Self {
            tenant: Some(actor.tenant),
            project: None,
            actor_id: actor.id.to_string(),
            actor_kind: actor.kind_str().to_string(),
            action: action.into(),
            resource_type: resource_type.into(),
            resource_id: resource_id.into(),
            outcome,
            correlation: None,
            details: Value::Object(Default::default()),
        }
    }

    /// Entry attributed to a platform component (e.g. `system:workflow-engine`).
    pub fn system(
        component: &str,
        action: impl Into<String>,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        outcome: AuditOutcome,
    ) -> Self {
        Self {
            tenant: None,
            project: None,
            actor_id: format!("system:{component}"),
            actor_kind: "system".into(),
            action: action.into(),
            resource_type: resource_type.into(),
            resource_id: resource_id.into(),
            outcome,
            correlation: None,
            details: Value::Object(Default::default()),
        }
    }

    pub fn scope(mut self, scope: Scope) -> Self {
        self.tenant = Some(scope.tenant);
        self.project = Some(scope.project);
        self
    }

    pub fn correlation(mut self, c: CorrelationId) -> Self {
        self.correlation = Some(c);
        self
    }

    pub fn details(mut self, d: Value) -> Self {
        self.details = d;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub seq: i64,
    pub id: String,
    pub occurred_at: String,
    pub tenant_id: Option<String>,
    pub project_id: Option<String>,
    pub actor_id: String,
    pub actor_kind: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub outcome: String,
    pub correlation_id: Option<String>,
    pub details: String,
    pub prev_hash: String,
    pub hash: String,
}

impl AuditRecord {
    fn compute_hash(&self) -> String {
        let canonical = json!({
            "seq": self.seq,
            "id": self.id,
            "occurred_at": self.occurred_at,
            "tenant_id": self.tenant_id,
            "project_id": self.project_id,
            "actor_id": self.actor_id,
            "actor_kind": self.actor_kind,
            "action": self.action,
            "resource_type": self.resource_type,
            "resource_id": self.resource_id,
            "outcome": self.outcome,
            "correlation_id": self.correlation_id,
            "details": self.details,
            "prev_hash": self.prev_hash,
        });
        sha256_hex(eaeh_core::hash::canonical_json(&canonical).as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifyReport {
    pub records_checked: u64,
    pub valid: bool,
    pub first_invalid_seq: Option<i64>,
    pub reason: Option<String>,
    pub head_hash: String,
}

impl Db {
    /// Append within an existing write transaction (atomic with the audited change).
    pub async fn audit_in(&self, tx: &mut WriteTx, entry: AuditEntry) -> Result<AuditRecord> {
        let last: Option<(i64, String)> =
            sqlx::query_as("SELECT seq, hash FROM audit_log ORDER BY seq DESC LIMIT 1")
                .fetch_optional(tx.conn())
                .await
                .db()?;
        let (prev_seq, prev_hash) = last.unwrap_or((0, GENESIS_HASH.to_string()));
        let details = self.redactor().redact_json(&entry.details);
        let mut rec = AuditRecord {
            seq: prev_seq + 1,
            id: AuditId::new().to_string(),
            occurred_at: ts(self.now()),
            tenant_id: entry.tenant.map(|t| t.to_string()),
            project_id: entry.project.map(|p| p.to_string()),
            actor_id: self.redactor().redact_str(&entry.actor_id),
            actor_kind: entry.actor_kind,
            action: entry.action,
            resource_type: entry.resource_type,
            resource_id: entry.resource_id,
            outcome: entry.outcome.as_str().to_string(),
            correlation_id: entry.correlation.map(|c| c.to_string()),
            details: eaeh_core::hash::canonical_json(&details),
            prev_hash,
            hash: String::new(),
        };
        rec.hash = rec.compute_hash();
        sqlx::query(
            "INSERT INTO audit_log (seq, id, occurred_at, tenant_id, project_id, actor_id, actor_kind, action, \
             resource_type, resource_id, outcome, correlation_id, details, prev_hash, hash) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(rec.seq)
        .bind(&rec.id)
        .bind(&rec.occurred_at)
        .bind(&rec.tenant_id)
        .bind(&rec.project_id)
        .bind(&rec.actor_id)
        .bind(&rec.actor_kind)
        .bind(&rec.action)
        .bind(&rec.resource_type)
        .bind(&rec.resource_id)
        .bind(&rec.outcome)
        .bind(&rec.correlation_id)
        .bind(&rec.details)
        .bind(&rec.prev_hash)
        .bind(&rec.hash)
        .execute(tx.conn())
        .await
        .db()?;
        tracing::debug!(seq = rec.seq, action = %rec.action, outcome = %rec.outcome, "audit appended");
        Ok(rec)
    }

    /// Append in its own transaction.
    pub async fn audit(&self, entry: AuditEntry) -> Result<AuditRecord> {
        let mut tx = self.write().await?;
        let rec = self.audit_in(&mut tx, entry).await?;
        tx.commit().await?;
        Ok(rec)
    }

    /// Page through audit records (ascending), optionally filtered by tenant.
    pub async fn audit_records(
        &self,
        tenant: Option<TenantId>,
        after_seq: i64,
        limit: u32,
    ) -> Result<Vec<AuditRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM audit_log WHERE seq > ? AND (? IS NULL OR tenant_id = ?) ORDER BY seq LIMIT ?",
        )
        .bind(after_seq)
        .bind(tenant.map(|t| t.to_string()))
        .bind(tenant.map(|t| t.to_string()))
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .db()?;
        rows.iter().map(row_to_record).collect()
    }

    /// Newest first, strictly before `before_seq` when given (latest-activity views).
    pub async fn audit_recent(
        &self,
        tenant: Option<TenantId>,
        before_seq: Option<i64>,
        limit: u32,
    ) -> Result<Vec<AuditRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM audit_log WHERE (? IS NULL OR seq < ?) AND (? IS NULL OR tenant_id = ?) ORDER BY seq DESC LIMIT ?",
        )
        .bind(before_seq)
        .bind(before_seq)
        .bind(tenant.map(|t| t.to_string()))
        .bind(tenant.map(|t| t.to_string()))
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .db()?;
        rows.iter().map(row_to_record).collect()
    }

    /// Records of one action on one resource, oldest first.
    pub async fn audit_by_resource(&self, resource_id: &str, action: &str) -> Result<Vec<AuditRecord>> {
        let rows = sqlx::query("SELECT * FROM audit_log WHERE resource_id = ? AND action = ? ORDER BY seq")
            .bind(resource_id)
            .bind(action)
            .fetch_all(self.pool())
            .await
            .db()?;
        rows.iter().map(row_to_record).collect()
    }

    pub async fn audit_by_correlation(&self, correlation: CorrelationId) -> Result<Vec<AuditRecord>> {
        let rows = sqlx::query("SELECT * FROM audit_log WHERE correlation_id = ? ORDER BY seq")
            .bind(correlation.to_string())
            .fetch_all(self.pool())
            .await
            .db()?;
        rows.iter().map(row_to_record).collect()
    }

    /// Recompute the full chain and report the first inconsistency, if any.
    pub async fn verify_audit_chain(&self) -> Result<VerifyReport> {
        let rows = sqlx::query("SELECT * FROM audit_log ORDER BY seq")
            .fetch_all(self.pool())
            .await
            .db()?;
        let mut expected_prev = GENESIS_HASH.to_string();
        let mut checked = 0u64;
        for (expected_seq, row) in (1i64..).zip(rows.iter()) {
            let rec = row_to_record(row)?;
            let fail = |reason: String| VerifyReport {
                records_checked: checked,
                valid: false,
                first_invalid_seq: Some(rec.seq),
                reason: Some(reason),
                head_hash: expected_prev.clone(),
            };
            if rec.seq != expected_seq {
                return Ok(fail(format!(
                    "sequence gap: expected {expected_seq}, found {}",
                    rec.seq
                )));
            }
            if rec.prev_hash != expected_prev {
                return Ok(fail("prev_hash does not match previous record".into()));
            }
            if rec.compute_hash() != rec.hash {
                return Ok(fail("record hash mismatch (content modified)".into()));
            }
            expected_prev = rec.hash.clone();
            checked += 1;
        }
        Ok(VerifyReport {
            records_checked: checked,
            valid: true,
            first_invalid_seq: None,
            reason: None,
            head_hash: expected_prev,
        })
    }
}

fn row_to_record(row: &sqlx::sqlite::SqliteRow) -> Result<AuditRecord> {
    let g = |c: &str| -> Result<String> { row.try_get::<String, _>(c).db() };
    let go = |c: &str| -> Result<Option<String>> { row.try_get::<Option<String>, _>(c).db() };
    Ok(AuditRecord {
        seq: row.try_get("seq").db()?,
        id: g("id")?,
        occurred_at: g("occurred_at")?,
        tenant_id: go("tenant_id")?,
        project_id: go("project_id")?,
        actor_id: g("actor_id")?,
        actor_kind: g("actor_kind")?,
        action: g("action")?,
        resource_type: g("resource_type")?,
        resource_id: g("resource_id")?,
        outcome: g("outcome")?,
        correlation_id: go("correlation_id")?,
        details: g("details")?,
        prev_hash: g("prev_hash")?,
        hash: g("hash")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eaeh_core::SecretString;

    async fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("a.db")).await.unwrap();
        (dir, db)
    }

    fn entry(i: usize) -> AuditEntry {
        AuditEntry::system(
            "test",
            format!("action.{i}"),
            "thing",
            i.to_string(),
            AuditOutcome::Recorded,
        )
        .details(json!({"i": i}))
    }

    #[tokio::test]
    async fn chain_verifies_and_detects_modification() {
        let (_d, db) = db().await;
        for i in 0..5 {
            db.audit(entry(i)).await.unwrap();
        }
        let ok = db.verify_audit_chain().await.unwrap();
        assert!(ok.valid);
        assert_eq!(ok.records_checked, 5);

        sqlx::query("UPDATE audit_log SET details = '{\"i\":99}' WHERE seq = 3")
            .execute(db.pool())
            .await
            .unwrap();
        let bad = db.verify_audit_chain().await.unwrap();
        assert!(!bad.valid);
        assert_eq!(bad.first_invalid_seq, Some(3));
    }

    #[tokio::test]
    async fn chain_detects_deletion() {
        let (_d, db) = db().await;
        for i in 0..4 {
            db.audit(entry(i)).await.unwrap();
        }
        sqlx::query("DELETE FROM audit_log WHERE seq = 2")
            .execute(db.pool())
            .await
            .unwrap();
        let bad = db.verify_audit_chain().await.unwrap();
        assert!(!bad.valid);
        assert_eq!(bad.first_invalid_seq, Some(3));
    }

    #[tokio::test]
    async fn concurrent_appends_remain_gapless() {
        let (_d, db) = db().await;
        let mut handles = vec![];
        for i in 0..20 {
            let db = db.clone();
            handles.push(tokio::spawn(async move { db.audit(entry(i)).await.unwrap() }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let r = db.verify_audit_chain().await.unwrap();
        assert!(r.valid);
        assert_eq!(r.records_checked, 20);
    }

    #[tokio::test]
    async fn details_are_redacted_before_storage() {
        let (_d, db) = db().await;
        db.redactor().register(&SecretString::new("very-secret-value"));
        db.audit(entry(0).details(json!({"note": "contains very-secret-value", "password": "p@ss"})))
            .await
            .unwrap();
        let recs = db.audit_records(None, 0, 10).await.unwrap();
        assert!(!recs[0].details.contains("very-secret-value"));
        assert!(!recs[0].details.contains("p@ss"));
        assert!(db.verify_audit_chain().await.unwrap().valid);
    }

    #[tokio::test]
    async fn rolled_back_transaction_leaves_no_audit() {
        let (_d, db) = db().await;
        let mut tx = db.write().await.unwrap();
        db.audit_in(&mut tx, entry(1)).await.unwrap();
        tx.rollback().await.unwrap();
        assert!(db.audit_records(None, 0, 10).await.unwrap().is_empty());
    }
}
