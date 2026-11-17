//! Installation maintenance used by `eaeh setup`: verified database backup and audit records
//! for setup actions. Lives here because only the runtime layer may touch the store.

use crate::Harness;
use eaeh_core::{HarnessError, Principal, Result, Scope};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::sqlx::{self, ConnectOptions, Connection};
use std::path::Path;

fn open(path: &Path) -> sqlx::sqlite::SqliteConnectOptions {
    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
}

/// Copy the database at `db` to `dest` (consistent `VACUUM INTO`; plain copy with its
/// write-ahead log if it cannot be opened) and verify the copy with SQLite's integrity check.
pub async fn backup_database(db: &Path, dest: &Path) -> Result<()> {
    let vac = async {
        let mut c = open(db).connect().await?;
        sqlx::query("VACUUM INTO ?")
            .bind(dest.display().to_string())
            .execute(&mut c)
            .await?;
        let _ = c.close().await;
        Ok::<(), sqlx::Error>(())
    }
    .await;
    if vac.is_err() {
        let _ = std::fs::remove_file(dest);
        std::fs::copy(db, dest)?;
        let name = dest.display().to_string();
        for ext in ["-wal", "-shm"] {
            let s = std::path::PathBuf::from(format!("{}{ext}", db.display()));
            if s.is_file() {
                std::fs::copy(&s, format!("{name}{ext}"))?;
            }
        }
    }
    let mut c = open(dest)
        .connect()
        .await
        .map_err(|e| HarnessError::integrity(format!("backup {} cannot be opened: {e}", dest.display())))?;
    let (res,): (String,) = sqlx::query_as("PRAGMA integrity_check")
        .fetch_one(&mut c)
        .await
        .map_err(|e| HarnessError::integrity(format!("backup integrity check failed: {e}")))?;
    let _ = c.close().await;
    if res != "ok" {
        return Err(HarnessError::integrity(format!("backup integrity check: {res}")));
    }
    Ok(())
}

/// Audit record of a platform component (`system:<component>`), optionally project-scoped.
#[allow(clippy::too_many_arguments)]
pub async fn audit_system(
    h: &Harness,
    component: &str,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    ok: bool,
    scope: Option<Scope>,
    details: serde_json::Value,
) -> Result<()> {
    let outcome = if ok {
        AuditOutcome::Succeeded
    } else {
        AuditOutcome::Failed
    };
    let mut e = AuditEntry::system(component, action, resource_type, resource_id, outcome);
    if let Some(s) = scope {
        e = e.scope(s);
    }
    h.db.audit(e.details(details)).await.map(|_| ())
}

/// Audit record attributed to `actor`.
pub async fn audit_actor(
    h: &Harness,
    actor: &Principal,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    details: serde_json::Value,
) -> Result<()> {
    h.db.audit(
        AuditEntry::new(actor, action, resource_type, resource_id, AuditOutcome::Succeeded).details(details),
    )
    .await
    .map(|_| ())
}
