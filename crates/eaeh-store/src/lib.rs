//! EAEH persistence foundation (ADR-0004).
//!
//! * [`Db`] – SQLite connection pool (WAL, foreign keys, busy timeout) plus embedded,
//!   ordered migrations.
//! * [`WriteTx`] – all writes go through a serialized write transaction. SQLite permits a
//!   single writer; serializing in-process makes multi-statement invariants (optimistic
//!   version checks, gapless audit sequence) deterministic and avoids `SQLITE_BUSY` upgrades.
//! * [`audit`] – append-only, SHA-256 hash-chained audit log with verification.
//! * [`idempotency`] – idempotency key records preventing duplicate side effects.
//!
//! Domain crates own their SQL (repository implementations) and use the re-exported
//! [`sqlx`] so the whole workspace shares one database layer. Only static SQL strings are
//! accepted by sqlx 0.9 (`SqlSafeStr`), so queries cannot be assembled from input.

pub mod audit;
pub mod idempotency;

pub use sqlx;

use chrono::{DateTime, SecondsFormat, Utc};
use eaeh_core::clock::{SharedClock, system_clock};
use eaeh_core::{HarnessError, Redactor, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqliteConnection, SqlitePool, Transaction};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, OwnedMutexGuard};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
    write_lock: Arc<Mutex<()>>,
    clock: SharedClock,
    redactor: Redactor,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").finish_non_exhaustive()
    }
}

impl Db {
    /// Open (creating if missing) the database file and apply pending migrations.
    pub async fn open(path: &Path) -> Result<Self> {
        Self::open_with(path, system_clock(), Redactor::new()).await
    }

    pub async fn open_with(path: &Path, clock: SharedClock, redactor: Redactor) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
            // Data directory created by EAEH: owner-only access (mitigation for A-10).
            restrict_dir_permissions(parent);
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts)
            .await
            .map_err(|e| HarnessError::unavailable(format!("open database {}: {e}", path.display())))?;
        MIGRATOR
            .run(&pool)
            .await
            .map_err(|e| HarnessError::internal(format!("migration failed: {e}")))?;
        restrict_permissions(path);
        Ok(Self {
            pool,
            write_lock: Arc::new(Mutex::new(())),
            clock,
            redactor,
        })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn clock(&self) -> &SharedClock {
        &self.clock
    }

    pub fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// Begin a serialized write transaction. **Every write in the platform goes through
    /// this method** (enforced by review and the fitness test). Hold it only for short,
    /// I/O-free critical sections (no model calls, no tool execution).
    pub async fn write(&self) -> Result<WriteTx> {
        let guard = self.write_lock.clone().lock_owned().await;
        // IMMEDIATE takes SQLite's write lock up front (waiting up to busy_timeout), so a
        // transaction can never fail later with SQLITE_BUSY_SNAPSHOT when another
        // connection or process commits between its first read and first write.
        let tx = self.pool.begin_with("BEGIN IMMEDIATE").await.db()?;
        Ok(WriteTx { tx, _guard: guard })
    }

    /// Applied migration versions (for diagnostics and the completion report).
    pub async fn applied_migrations(&self) -> Result<Vec<i64>> {
        let rows: Vec<(i64,)> = sqlx::query_as("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&self.pool)
            .await
            .db()?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    // The database contains token hashes and audit data: owner read/write only.
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        tracing::warn!(error = %e, "could not restrict database file permissions");
    }
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(unix)]
fn restrict_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)) {
        tracing::warn!(error = %e, "could not restrict data directory permissions");
    }
}

#[cfg(not(unix))]
fn restrict_dir_permissions(_path: &Path) {}

pub struct WriteTx {
    tx: Transaction<'static, Sqlite>,
    _guard: OwnedMutexGuard<()>,
}

impl WriteTx {
    pub fn conn(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }

    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await.db()
    }

    pub async fn rollback(self) -> Result<()> {
        self.tx.rollback().await.db()
    }
}

/// Map sqlx errors into the platform taxonomy.
pub fn map_db_error(e: sqlx::Error) -> HarnessError {
    match &e {
        sqlx::Error::RowNotFound => HarnessError::not_found("record not found"),
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            HarnessError::conflict(format!("uniqueness violation: {}", d.message()))
        }
        sqlx::Error::Database(d) if d.is_foreign_key_violation() => {
            HarnessError::invalid_input(format!("reference violation: {}", d.message()))
        }
        sqlx::Error::Database(d) if d.is_check_violation() => {
            HarnessError::invalid_input(format!("constraint violation: {}", d.message()))
        }
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_) => {
            HarnessError::unavailable(format!("database unavailable: {e}"))
        }
        _ => HarnessError::internal(format!("database: {e}")),
    }
}

pub trait DbResultExt<T> {
    fn db(self) -> Result<T>;
}

impl<T> DbResultExt<T> for std::result::Result<T, sqlx::Error> {
    fn db(self) -> Result<T> {
        self.map_err(map_db_error)
    }
}

/// Canonical timestamp encoding (lexicographically sortable RFC3339, microseconds, `Z`).
pub fn ts(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub fn parse_ts(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| HarnessError::integrity(format!("invalid stored timestamp '{s}': {e}")))
}

pub fn to_json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| HarnessError::internal(format!("serialize: {e}")))
}

pub fn from_json<T: serde::de::DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(|e| HarnessError::integrity(format!("stored json invalid: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_and_migrates_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eaeh.db");
        let db = Db::open(&path).await.unwrap();
        assert_eq!(db.applied_migrations().await.unwrap(), vec![1, 2, 3, 4, 5]);
        db.close().await;
        let nested = dir.path().join("new-data-dir").join("eaeh.db");
        Db::open(&nested).await.unwrap().close().await;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(nested.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700, "data directory created by EAEH must be owner-only");
        }
        let db = Db::open(&path).await.unwrap();
        assert_eq!(db.applied_migrations().await.unwrap(), vec![1, 2, 3, 4, 5]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[tokio::test]
    async fn foreign_keys_are_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("eaeh.db")).await.unwrap();
        let mut tx = db.write().await.unwrap();
        let r = sqlx::query(
            "INSERT INTO projects (id, tenant_id, name, created_at) VALUES ('p', 'missing', 'n', 'x')",
        )
        .execute(tx.conn())
        .await
        .db();
        assert_eq!(r.unwrap_err().kind, eaeh_core::ErrorKind::InvalidInput);
    }

    #[test]
    fn timestamps_roundtrip_and_sort() {
        let a = Utc::now();
        let b = a + chrono::Duration::milliseconds(5);
        assert!(ts(a) < ts(b));
        assert_eq!(parse_ts(&ts(a)).unwrap().timestamp_micros(), a.timestamp_micros());
    }
}
