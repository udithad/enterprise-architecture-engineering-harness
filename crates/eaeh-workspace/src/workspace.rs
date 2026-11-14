use crate::locks;
use crate::validate_relative;
use chrono::Duration;
use eaeh_core::{HarnessError, PrincipalId, Result, Scope, TraceContext, WorkspaceId};
use eaeh_extensibility::{HookContext, HookPayload, HookPoint, HookRegistry};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, sqlx, ts};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    db: Db,
    root: PathBuf,
    hooks: Arc<HookRegistry>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub scope: Scope,
    root: PathBuf,
    db: Db,
    hooks: Arc<HookRegistry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

impl WorkspaceManager {
    pub fn new(db: Db, root: &Path, hooks: Arc<HookRegistry>) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        Ok(Self { db, root, hooks })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn create(&self, scope: Scope, owner: &PrincipalId) -> Result<Workspace> {
        let id = WorkspaceId::new();
        let dir = self
            .root
            .join(scope.tenant.to_string())
            .join(scope.project.to_string())
            .join(id.to_string());
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let dir = dir.canonicalize()?;
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO workspaces (id, tenant_id, project_id, root_path, owner_id, state, created_at) VALUES (?, ?, ?, ?, ?, 'active', ?)",
        )
        .bind(id.to_string())
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(dir.to_string_lossy().to_string())
        .bind(owner.as_str())
        .bind(ts(self.db.now()))
        .execute(tx.conn())
        .await
        .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    "workspace-manager",
                    "workspace.create",
                    "workspace",
                    id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .scope(scope)
                .details(serde_json::json!({"owner": owner.as_str()})),
            )
            .await?;
        tx.commit().await?;
        Ok(Workspace {
            id,
            scope,
            root: dir,
            db: self.db.clone(),
            hooks: self.hooks.clone(),
        })
    }

    /// Open an active workspace. The scope must match the workspace's own scope.
    pub async fn open(&self, id: WorkspaceId, scope: Scope) -> Result<Workspace> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT root_path, state FROM workspaces WHERE id = ? AND tenant_id = ? AND project_id = ?",
        )
        .bind(id.to_string())
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .fetch_optional(self.db.pool())
        .await
        .db()?;
        let (root, state) = row.ok_or_else(|| HarnessError::not_found("workspace not found in scope"))?;
        if state != "active" {
            return Err(HarnessError::conflict("workspace has been cleaned up"));
        }
        let root = PathBuf::from(root);
        if !root.starts_with(&self.root) {
            return Err(HarnessError::integrity("workspace root outside managed root"));
        }
        Ok(Workspace {
            id,
            scope,
            root,
            db: self.db.clone(),
            hooks: self.hooks.clone(),
        })
    }

    /// Remove the workspace directory and mark it cleaned. Locks are dropped.
    pub async fn cleanup(&self, id: WorkspaceId, scope: Scope) -> Result<()> {
        let ws = self.open(id, scope).await?;
        if ws.root.exists() {
            std::fs::remove_dir_all(&ws.root)?;
        }
        let mut tx = self.db.write().await?;
        sqlx::query("DELETE FROM workspace_locks WHERE workspace_id = ?")
            .bind(id.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        sqlx::query("UPDATE workspaces SET state = 'cleaned', cleaned_at = ? WHERE id = ?")
            .bind(ts(self.db.now()))
            .bind(id.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    "workspace-manager",
                    "workspace.cleanup",
                    "workspace",
                    id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .scope(scope),
            )
            .await?;
        tx.commit().await
    }
}

impl Workspace {
    /// Host path of the workspace root (for isolation providers).
    pub fn host_root(&self) -> &Path {
        &self.root
    }

    /// Resolve a relative path, rejecting traversal and any symlink on the existing path.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let parts = validate_relative(rel)?;
        let mut cur = self.root.clone();
        for p in &parts {
            cur.push(p);
            match std::fs::symlink_metadata(&cur) {
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(HarnessError::forbidden(format!(
                        "symlink in workspace path rejected: {rel}"
                    )));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(cur)
    }

    pub async fn acquire_lock(&self, path: &str, holder: &str, ttl: Duration) -> Result<crate::LockInfo> {
        let key = if path == "*" {
            "*".to_string()
        } else {
            validate_relative(path)?.join("/")
        };
        locks::acquire(&self.db, self.id, &key, holder, ttl).await
    }

    pub async fn release_lock(&self, path: &str, holder: &str) -> Result<bool> {
        let key = if path == "*" {
            "*".to_string()
        } else {
            validate_relative(path)?.join("/")
        };
        locks::release(&self.db, self.id, &key, holder).await
    }

    pub async fn release_all_locks(&self, holder: &str) -> Result<()> {
        locks::release_all(&self.db, self.id, holder).await
    }

    pub async fn locks(&self) -> Result<Vec<crate::LockInfo>> {
        locks::list(&self.db, self.id).await
    }

    pub fn read_file(&self, rel: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let path = self.resolve(rel)?;
        let f = open_nofollow_read(&path)?;
        let len = f.metadata()?.len();
        if len > max_bytes as u64 {
            return Err(HarnessError::invalid_input(format!(
                "{rel} is {len} bytes (limit {max_bytes})"
            )));
        }
        let mut buf = Vec::with_capacity(len as usize);
        use std::io::Read;
        f.take(max_bytes as u64 + 1).read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// Write a file atomically. The caller must hold a lock covering `rel`.
    pub async fn write_file(&self, holder: &str, rel: &str, data: &[u8], trace: TraceContext) -> Result<()> {
        if data.len() > MAX_FILE_BYTES {
            return Err(HarnessError::invalid_input(format!(
                "file exceeds {MAX_FILE_BYTES} bytes"
            )));
        }
        let key = validate_relative(rel)?.join("/");
        if !locks::holds(&self.db, self.id, &key, holder).await? {
            return Err(HarnessError::conflict(format!(
                "write to '{key}' requires a lock held by {holder}"
            )));
        }
        let ctx = |point| HookContext {
            point,
            scope: Some(self.scope),
            trace,
            actor: Some(holder.to_string()),
            subject: key.clone(),
            payload: HookPayload::File {
                workspace_id: self.id.to_string(),
                path: key.clone(),
            },
        };
        self.hooks
            .dispatch(&ctx(HookPoint::BeforeFileModification))
            .await
            .into_result()?;
        let path = self.resolve(rel)?;
        let parent = path
            .parent()
            .ok_or_else(|| HarnessError::invalid_input("no parent directory"))?;
        std::fs::create_dir_all(parent)?;
        // Re-check after directory creation (no symlinks introduced on the path).
        let path = self.resolve(rel)?;
        let tmp = parent.join(format!(".eaeh-tmp-{}", uuid::Uuid::now_v7()));
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            f.write_all(data)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &path)?;
        self.hooks.dispatch(&ctx(HookPoint::AfterFileModification)).await;
        Ok(())
    }

    pub fn list(&self, rel_dir: Option<&str>) -> Result<Vec<DirEntry>> {
        let dir = match rel_dir {
            Some(r) => self.resolve(r)?,
            None => self.root.clone(),
        };
        let mut out = vec![];
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(".eaeh-") {
                continue;
            }
            let m = std::fs::symlink_metadata(e.path())?;
            if m.file_type().is_symlink() {
                continue;
            }
            let rel = e
                .path()
                .strip_prefix(&self.root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or(name);
            out.push(DirEntry {
                path: rel,
                is_dir: m.is_dir(),
                size: if m.is_dir() { 0 } else { m.len() },
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }
}

fn open_nofollow_read(path: &Path) -> Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    let f = o.open(path).map_err(|e| {
        if e.raw_os_error() == Some(libc::ELOOP) {
            HarnessError::forbidden("symlink rejected")
        } else {
            e.into()
        }
    })?;
    if !f.metadata()?.is_file() {
        return Err(HarnessError::invalid_input("not a regular file"));
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eaeh_core::{ErrorKind, ProjectId, TenantId};

    async fn setup() -> (tempfile::TempDir, WorkspaceManager, Workspace) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("w.db")).await.unwrap();
        let m = WorkspaceManager::new(db, &d.path().join("ws"), Arc::new(HookRegistry::new())).unwrap();
        let ws = m
            .create(
                Scope::new(TenantId::new(), ProjectId::new()),
                &PrincipalId::new("owner").unwrap(),
            )
            .await
            .unwrap();
        (d, m, ws)
    }

    #[tokio::test]
    async fn write_requires_lock_and_read_back() {
        let (_d, _m, ws) = setup().await;
        let t = TraceContext::new_root();
        assert_eq!(
            ws.write_file("a1", "src/x.txt", b"hi", t).await.unwrap_err().kind,
            ErrorKind::Conflict
        );
        ws.acquire_lock("src", "a1", Duration::minutes(5)).await.unwrap();
        ws.write_file("a1", "src/x.txt", b"hi", t).await.unwrap();
        assert_eq!(ws.read_file("src/x.txt", 100).unwrap(), b"hi");
        assert_eq!(ws.list(Some("src")).unwrap()[0].path, "src/x.txt");
    }

    #[tokio::test]
    async fn concurrent_holders_conflict() {
        let (_d, _m, ws) = setup().await;
        ws.acquire_lock("src", "a1", Duration::minutes(5)).await.unwrap();
        assert_eq!(
            ws.acquire_lock("src/lib.rs", "a2", Duration::minutes(5))
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
        assert_eq!(
            ws.acquire_lock("*", "a2", Duration::minutes(5))
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
        ws.acquire_lock("docs", "a2", Duration::minutes(5)).await.unwrap();
        assert_eq!(
            ws.write_file("a2", "src/lib.rs", b"x", TraceContext::new_root())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
        ws.release_lock("src", "a1").await.unwrap();
        ws.acquire_lock("src/lib.rs", "a2", Duration::minutes(5))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn expired_locks_can_be_taken_over() {
        let (_d, _m, ws) = setup().await;
        ws.acquire_lock("a", "h1", Duration::milliseconds(-1))
            .await
            .unwrap();
        ws.acquire_lock("a", "h2", Duration::minutes(1)).await.unwrap();
    }

    #[tokio::test]
    async fn traversal_and_symlink_escapes_rejected() {
        let (d, _m, ws) = setup().await;
        let secret = d.path().join("secret.txt");
        std::fs::write(&secret, "TOP SECRET").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&secret, ws.host_root().join("link.txt")).unwrap();
            std::os::unix::fs::symlink(d.path(), ws.host_root().join("linkdir")).unwrap();
        }
        assert_eq!(
            ws.read_file("../../../../secret.txt", 100).unwrap_err().kind,
            ErrorKind::Forbidden
        );
        assert_eq!(
            ws.read_file(secret.to_str().unwrap(), 100).unwrap_err().kind,
            ErrorKind::Forbidden
        );
        #[cfg(unix)]
        {
            assert_eq!(
                ws.read_file("link.txt", 100).unwrap_err().kind,
                ErrorKind::Forbidden
            );
            assert_eq!(
                ws.read_file("linkdir/secret.txt", 100).unwrap_err().kind,
                ErrorKind::Forbidden
            );
            ws.acquire_lock("*", "h", Duration::minutes(1)).await.unwrap();
            assert_eq!(
                ws.write_file("h", "linkdir/evil.txt", b"x", TraceContext::new_root())
                    .await
                    .unwrap_err()
                    .kind,
                ErrorKind::Forbidden
            );
            assert!(!d.path().join("evil.txt").exists());
        }
    }

    #[tokio::test]
    async fn scope_mismatch_and_cleanup() {
        let (_d, m, ws) = setup().await;
        let other = Scope::new(ws.scope.tenant, ProjectId::new());
        assert_eq!(m.open(ws.id, other).await.unwrap_err().kind, ErrorKind::NotFound);
        let root = ws.host_root().to_path_buf();
        m.cleanup(ws.id, ws.scope).await.unwrap();
        assert!(!root.exists());
        assert_eq!(
            m.open(ws.id, ws.scope).await.unwrap_err().kind,
            ErrorKind::Conflict
        );
    }
}
