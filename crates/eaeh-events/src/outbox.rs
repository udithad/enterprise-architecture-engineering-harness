use crate::EventEnvelope;
use eaeh_core::{HarnessError, Result};
use eaeh_store::{Db, DbResultExt, WriteTx, sqlx, ts};
use sqlx::Row;

/// Insert an event into the outbox inside the caller's transaction.
/// Returns `false` if an event with the same id was already recorded (duplicate publish).
pub async fn enqueue_in(tx: &mut WriteTx, envelope: &EventEnvelope) -> Result<bool> {
    let r = sqlx::query(
        "INSERT OR IGNORE INTO outbox (event_id, event_type, schema_version, occurred_at, tenant_id, project_id, \
         correlation_id, causation_id, producer, payload, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')",
    )
    .bind(envelope.event_id.to_string())
    .bind(&envelope.event_type)
    .bind(i64::from(envelope.schema_version))
    .bind(ts(envelope.occurred_at))
    .bind(envelope.tenant.map(|t| t.to_string()))
    .bind(envelope.project.map(|p| p.to_string()))
    .bind(envelope.correlation.to_string())
    .bind(envelope.causation.map(|c| c.to_string()))
    .bind(&envelope.producer)
    .bind(eaeh_store::to_json(&envelope.payload)?)
    .execute(tx.conn())
    .await
    .db()?;
    Ok(r.rows_affected() == 1)
}

/// Convenience: enqueue in its own transaction.
pub async fn enqueue(db: &Db, envelope: &EventEnvelope) -> Result<bool> {
    let mut tx = db.write().await?;
    let inserted = enqueue_in(&mut tx, envelope).await?;
    tx.commit().await?;
    Ok(inserted)
}

pub(crate) async fn pending(db: &Db, limit: u32) -> Result<Vec<EventEnvelope>> {
    let rows = sqlx::query("SELECT * FROM outbox WHERE status = 'pending' ORDER BY rowid_seq LIMIT ?")
        .bind(i64::from(limit))
        .fetch_all(db.pool())
        .await
        .db()?;
    rows.iter().map(row_to_envelope).collect()
}

/// All events for a correlation id (diagnostics / traceability).
pub async fn by_correlation(db: &Db, correlation: eaeh_core::CorrelationId) -> Result<Vec<EventEnvelope>> {
    let rows = sqlx::query("SELECT * FROM outbox WHERE correlation_id = ? ORDER BY rowid_seq")
        .bind(correlation.to_string())
        .fetch_all(db.pool())
        .await
        .db()?;
    rows.iter().map(row_to_envelope).collect()
}

fn row_to_envelope(row: &sqlx::sqlite::SqliteRow) -> Result<EventEnvelope> {
    let parse_opt = |c: &str| -> Result<Option<String>> { row.try_get::<Option<String>, _>(c).db() };
    let version: i64 = row.try_get("schema_version").db()?;
    Ok(EventEnvelope {
        event_id: row.try_get::<String, _>("event_id").db()?.parse()?,
        event_type: row.try_get("event_type").db()?,
        schema_version: u16::try_from(version)
            .map_err(|_| HarnessError::integrity("schema_version range"))?,
        occurred_at: eaeh_store::parse_ts(&row.try_get::<String, _>("occurred_at").db()?)?,
        tenant: parse_opt("tenant_id")?.map(|s| s.parse()).transpose()?,
        project: parse_opt("project_id")?.map(|s| s.parse()).transpose()?,
        correlation: row.try_get::<String, _>("correlation_id").db()?.parse()?,
        causation: parse_opt("causation_id")?
            .map(|s| s.parse().map_err(|_| HarnessError::integrity("causation id")))
            .transpose()?,
        producer: row.try_get("producer").db()?,
        payload: eaeh_store::from_json(&row.try_get::<String, _>("payload").db()?)?,
    })
}
