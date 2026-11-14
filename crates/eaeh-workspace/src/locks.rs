//! Ownership locks over workspace paths. A lock on a directory covers its subtree; `*`
//! covers the whole workspace. Locks expire (TTL) so crashed holders cannot block forever.

use chrono::Duration;
use eaeh_core::{HarnessError, Result, WorkspaceId};
use eaeh_store::{Db, DbResultExt, parse_ts, sqlx, ts};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub path: String,
    pub holder: String,
    pub expires_at: String,
}

pub(crate) fn overlaps(a: &str, b: &str) -> bool {
    a == "*" || b == "*" || a == b || b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/"))
}

pub(crate) fn covers(lock: &str, path: &str) -> bool {
    lock == "*" || lock == path || path.starts_with(&format!("{lock}/"))
}

pub(crate) async fn acquire(
    db: &Db,
    ws: WorkspaceId,
    path: &str,
    holder: &str,
    ttl: Duration,
) -> Result<LockInfo> {
    let now = db.now();
    let mut tx = db.write().await?;
    sqlx::query("DELETE FROM workspace_locks WHERE workspace_id = ? AND expires_at < ?")
        .bind(ws.to_string())
        .bind(ts(now))
        .execute(tx.conn())
        .await
        .db()?;
    let existing: Vec<(String, String)> =
        sqlx::query_as("SELECT path, holder FROM workspace_locks WHERE workspace_id = ?")
            .bind(ws.to_string())
            .fetch_all(tx.conn())
            .await
            .db()?;
    if let Some((p, h)) = existing.iter().find(|(p, h)| h != holder && overlaps(p, path)) {
        return Err(HarnessError::conflict(format!(
            "'{path}' is locked by {h} (lock on '{p}')"
        )));
    }
    let expires = now + ttl;
    sqlx::query(
        "INSERT INTO workspace_locks (workspace_id, path, holder, acquired_at, expires_at) VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(workspace_id, path) DO UPDATE SET expires_at = excluded.expires_at WHERE holder = excluded.holder",
    )
    .bind(ws.to_string())
    .bind(path)
    .bind(holder)
    .bind(ts(now))
    .bind(ts(expires))
    .execute(tx.conn())
    .await
    .db()?;
    tx.commit().await?;
    Ok(LockInfo {
        path: path.into(),
        holder: holder.into(),
        expires_at: ts(expires),
    })
}

pub(crate) async fn release(db: &Db, ws: WorkspaceId, path: &str, holder: &str) -> Result<bool> {
    let mut tx = db.write().await?;
    let r = sqlx::query("DELETE FROM workspace_locks WHERE workspace_id = ? AND path = ? AND holder = ?")
        .bind(ws.to_string())
        .bind(path)
        .bind(holder)
        .execute(tx.conn())
        .await
        .db()?;
    tx.commit().await?;
    Ok(r.rows_affected() > 0)
}

pub(crate) async fn release_all(db: &Db, ws: WorkspaceId, holder: &str) -> Result<()> {
    let mut tx = db.write().await?;
    sqlx::query("DELETE FROM workspace_locks WHERE workspace_id = ? AND holder = ?")
        .bind(ws.to_string())
        .bind(holder)
        .execute(tx.conn())
        .await
        .db()?;
    tx.commit().await
}

/// Whether `holder` holds an unexpired lock covering `path`.
pub(crate) async fn holds(db: &Db, ws: WorkspaceId, path: &str, holder: &str) -> Result<bool> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT path, expires_at FROM workspace_locks WHERE workspace_id = ? AND holder = ?")
            .bind(ws.to_string())
            .bind(holder)
            .fetch_all(db.pool())
            .await
            .db()?;
    let now = db.now();
    for (p, exp) in rows {
        if covers(&p, path) && parse_ts(&exp)? >= now {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) async fn list(db: &Db, ws: WorkspaceId) -> Result<Vec<LockInfo>> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT path, holder, expires_at FROM workspace_locks WHERE workspace_id = ? ORDER BY path",
    )
    .bind(ws.to_string())
    .fetch_all(db.pool())
    .await
    .db()?;
    Ok(rows
        .into_iter()
        .map(|(path, holder, expires_at)| LockInfo {
            path,
            holder,
            expires_at,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_rules() {
        assert!(overlaps("src", "src/a.rs"));
        assert!(overlaps("src/a.rs", "src"));
        assert!(overlaps("*", "x"));
        assert!(!overlaps("src", "srcx/a"));
        assert!(!overlaps("a", "b"));
        assert!(covers("src", "src/x/y"));
        assert!(!covers("src/x", "src"));
    }
}
