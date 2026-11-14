//! Idempotency keys: a side-effecting operation records its result under a key inside
//! the same transaction as its effect; replays return the recorded result instead of
//! repeating the effect.

use crate::{Db, DbResultExt, WriteTx, ts};
use eaeh_core::Result;
use serde_json::Value;

impl Db {
    pub async fn idempotent_result(&self, key: &str) -> Result<Option<Value>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT result FROM idempotency_keys WHERE key = ?")
            .bind(key)
            .fetch_optional(self.pool())
            .await
            .db()?;
        row.map(|(s,)| crate::from_json(&s)).transpose()
    }

    /// Record a result. Returns `false` if the key already existed (the first result wins).
    pub async fn record_idempotent_in(&self, tx: &mut WriteTx, key: &str, result: &Value) -> Result<bool> {
        let r =
            sqlx::query("INSERT OR IGNORE INTO idempotency_keys (key, result, created_at) VALUES (?, ?, ?)")
                .bind(key)
                .bind(crate::to_json(result)?)
                .bind(ts(self.now()))
                .execute(tx.conn())
                .await
                .db()?;
        Ok(r.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_result_wins() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("i.db")).await.unwrap();
        let mut tx = db.write().await.unwrap();
        assert!(
            db.record_idempotent_in(&mut tx, "k", &serde_json::json!(1))
                .await
                .unwrap()
        );
        assert!(
            !db.record_idempotent_in(&mut tx, "k", &serde_json::json!(2))
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert_eq!(
            db.idempotent_result("k").await.unwrap(),
            Some(serde_json::json!(1))
        );
        assert_eq!(db.idempotent_result("missing").await.unwrap(), None);
    }
}
