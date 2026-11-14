//! Content-addressed, versioned artifacts with provenance.

use eaeh_core::hash::sha256_hex;
use eaeh_core::{ArtifactId, HarnessError, Result, Scope, TraceContext};
use eaeh_events::catalog::ArtifactGenerated;
use eaeh_events::{EventEnvelope, outbox};
use eaeh_extensibility::{HookContext, HookPayload, HookPoint, HookRegistry};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, from_json, sqlx, to_json, ts};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;

/// Where an artifact came from (traceability, master §2 & §42).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub produced_by: String,
    pub agent_instance: Option<String>,
    pub agent_definition: Option<String>,
    pub task: Option<String>,
    pub workflow_instance: Option<String>,
    #[serde(default)]
    pub tool_invocations: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub prompts: Vec<String>,
    pub context_snapshot: Option<String>,
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Skills applied (`id@version`), Phase 2.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Knowledge citations available to the producing run, Phase 2.
    #[serde(default)]
    pub knowledge: Vec<String>,
    /// Mandatory standards supplied to the producing run (`src/subject@vN`), Phase 3.
    #[serde(default)]
    pub standards: Vec<String>,
    /// Quality gates passed before storing (`gate:passed (n blocking, m advisory)`), Phase 3.
    #[serde(default)]
    pub quality_gates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactMeta {
    pub id: ArtifactId,
    pub scope: Scope,
    pub name: String,
    pub version: i64,
    pub content_hash: String,
    pub size_bytes: i64,
    pub media_type: String,
    pub provenance: Provenance,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    db: Db,
    root: PathBuf,
    hooks: Arc<HookRegistry>,
}

fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 400
        && !name.contains("..")
        && !name.contains("//")
        && !name.starts_with('/')
        // ASCII letters (source files such as `OrderService.java`), digits and `.-_/`.
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/'));
    if ok {
        Ok(())
    } else {
        Err(HarnessError::invalid_input(format!(
            "invalid artifact name '{name}'"
        )))
    }
}

impl ArtifactStore {
    pub fn new(db: Db, root: &Path, hooks: Arc<HookRegistry>) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        Ok(Self {
            db,
            root: root.canonicalize()?,
            hooks,
        })
    }

    fn blob_path(&self, scope: &Scope, hash: &str) -> PathBuf {
        self.root
            .join(scope.tenant.to_string())
            .join("sha256")
            .join(&hash[..2])
            .join(hash)
    }

    pub async fn store(
        &self,
        scope: Scope,
        name: &str,
        media_type: &str,
        content: &[u8],
        provenance: Provenance,
        trace: TraceContext,
    ) -> Result<ArtifactMeta> {
        validate_name(name)?;
        if content.len() > MAX_ARTIFACT_BYTES {
            return Err(HarnessError::invalid_input("artifact too large"));
        }
        let hash = sha256_hex(content);
        let ctx = |point| HookContext {
            point,
            scope: Some(scope),
            trace,
            actor: Some(provenance.produced_by.clone()),
            subject: name.to_string(),
            payload: HookPayload::Artifact {
                name: name.into(),
                content_hash: Some(hash.clone()),
                size_bytes: Some(content.len() as u64),
            },
        };
        self.hooks
            .dispatch(&ctx(HookPoint::BeforeArtifactCreation))
            .await
            .into_result()?;

        let blob = self.blob_path(&scope, &hash);
        if !blob.exists() {
            if let Some(parent) = blob.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp = blob.with_extension(format!("tmp-{}", uuid::Uuid::now_v7()));
            std::fs::write(&tmp, content)?;
            std::fs::rename(&tmp, &blob)?;
        }

        let id = ArtifactId::new();
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        let (max,): (Option<i64>,) = sqlx::query_as(
            "SELECT MAX(version) FROM artifacts WHERE tenant_id = ? AND project_id = ? AND name = ?",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(name)
        .fetch_one(tx.conn())
        .await
        .db()?;
        let version = max.unwrap_or(0) + 1;
        let size = i64::try_from(content.len()).unwrap_or(i64::MAX);
        sqlx::query(
            "INSERT INTO artifacts (id, tenant_id, project_id, name, version, content_hash, size_bytes, media_type, provenance, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(name)
        .bind(version)
        .bind(&hash)
        .bind(size)
        .bind(media_type)
        .bind(to_json(&provenance)?)
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system("artifact-store", "artifact.create", "artifact", id.to_string(), AuditOutcome::Succeeded)
                    .scope(scope)
                    .correlation(trace.correlation)
                    .details(serde_json::json!({"name": name, "version": version, "content_hash": hash, "provenance": provenance})),
            )
            .await?;
        let ev = EventEnvelope::new(
            &ArtifactGenerated {
                artifact_id: id.to_string(),
                name: name.into(),
                version,
                content_hash: hash.clone(),
            },
            "artifact-store",
            Some(scope),
            trace,
            self.db.now(),
        )?;
        outbox::enqueue_in(&mut tx, &ev).await?;
        tx.commit().await?;
        self.hooks.dispatch(&ctx(HookPoint::AfterArtifactCreation)).await;
        Ok(ArtifactMeta {
            id,
            scope,
            name: name.into(),
            version,
            content_hash: hash,
            size_bytes: size,
            media_type: media_type.into(),
            provenance,
            created_at: now,
        })
    }

    pub async fn meta(&self, scope: Scope, id: ArtifactId) -> Result<ArtifactMeta> {
        let row = sqlx::query("SELECT * FROM artifacts WHERE id = ? AND tenant_id = ? AND project_id = ?")
            .bind(id.to_string())
            .bind(scope.tenant.to_string())
            .bind(scope.project.to_string())
            .fetch_optional(self.db.pool())
            .await
            .db()?
            .ok_or_else(|| HarnessError::not_found("artifact not found in scope"))?;
        row_to_meta(&row, scope)
    }

    /// Read content, verifying it against the recorded hash.
    pub async fn read(&self, scope: Scope, id: ArtifactId) -> Result<(ArtifactMeta, Vec<u8>)> {
        let meta = self.meta(scope, id).await?;
        let bytes = std::fs::read(self.blob_path(&scope, &meta.content_hash))?;
        if sha256_hex(&bytes) != meta.content_hash {
            return Err(HarnessError::integrity(format!(
                "artifact {id} content does not match its hash"
            )));
        }
        Ok((meta, bytes))
    }

    pub async fn versions(&self, scope: Scope, name: &str) -> Result<Vec<ArtifactMeta>> {
        let rows = sqlx::query(
            "SELECT * FROM artifacts WHERE tenant_id = ? AND project_id = ? AND name = ? ORDER BY version",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(name)
        .fetch_all(self.db.pool())
        .await
        .db()?;
        rows.iter().map(|r| row_to_meta(r, scope)).collect()
    }
}

fn row_to_meta(row: &sqlx::sqlite::SqliteRow, scope: Scope) -> Result<ArtifactMeta> {
    use sqlx::Row;
    Ok(ArtifactMeta {
        id: row.try_get::<String, _>("id").db()?.parse()?,
        scope,
        name: row.try_get("name").db()?,
        version: row.try_get("version").db()?,
        content_hash: row.try_get("content_hash").db()?,
        size_bytes: row.try_get("size_bytes").db()?,
        media_type: row.try_get("media_type").db()?,
        provenance: from_json(&row.try_get::<String, _>("provenance").db()?)?,
        created_at: row.try_get("created_at").db()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eaeh_core::{ErrorKind, ProjectId, TenantId};

    #[tokio::test]
    async fn versions_integrity_and_scope() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("a.db")).await.unwrap();
        let s = ArtifactStore::new(db, &d.path().join("art"), Arc::new(HookRegistry::new())).unwrap();
        let scope = Scope::new(TenantId::new(), ProjectId::new());
        let p = Provenance {
            produced_by: "agent:x".into(),
            ..Default::default()
        };
        let a1 = s
            .store(
                scope,
                "design/hld.md",
                "text/markdown",
                b"v1",
                p.clone(),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
        let a2 = s
            .store(
                scope,
                "design/hld.md",
                "text/markdown",
                b"v2",
                p,
                TraceContext::new_root(),
            )
            .await
            .unwrap();
        assert_eq!((a1.version, a2.version), (1, 2));
        assert_eq!(s.read(scope, a1.id).await.unwrap().1, b"v1");
        assert_eq!(s.versions(scope, "design/hld.md").await.unwrap().len(), 2);
        let other = Scope::new(scope.tenant, ProjectId::new());
        assert_eq!(s.read(other, a1.id).await.unwrap_err().kind, ErrorKind::NotFound);
        // Tamper with stored blob → integrity error.
        std::fs::write(s.blob_path(&scope, &a2.content_hash), b"evil").unwrap();
        assert_eq!(s.read(scope, a2.id).await.unwrap_err().kind, ErrorKind::Integrity);
        assert!(
            s.store(
                scope,
                "../x",
                "t",
                b"",
                Provenance::default(),
                TraceContext::new_root()
            )
            .await
            .is_err()
        );
    }
}
