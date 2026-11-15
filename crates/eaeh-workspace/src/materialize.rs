//! Materialization of approved artifact bundles into the registered application
//! repository of a project (`docs/implementation/stages/ARTIFACT-MATERIALIZATION-PLAN.md`).
//!
//! The [`ArtifactStore`] stays authoritative. A materialized file is a reproducible
//! projection of one stored file artifact: it is written only when its bytes equal the
//! stored hash, re-read and re-hashed after writing, and tracked in `materialized_files`
//! so that EAEH never silently replaces a file it did not write or that was edited locally
//! (decision M3).
//!
//! Safety properties:
//! * writes are confined to the registered root: relative paths only, no `..`, no
//!   symlinks anywhere on the path, and nothing under `.git/`;
//! * each file is written atomically (temporary file + `rename`) and verified;
//! * conflicts are detected for the whole bundle **before** anything is written;
//! * every attempt (succeeded, conflict, failed) is recorded, audited
//!   (`artifact.materialize`) and announced (`artifact.materialized`);
//! * `BeforeFileModification` / `AfterFileModification` hooks run per file. A blocking
//!   hook can veto a write; hooks cannot widen the confinement checks.

use crate::ArtifactStore;
use eaeh_core::hash::sha256_hex;
use eaeh_core::{ArtifactId, HarnessError, Result, Scope, TraceContext};
use eaeh_events::catalog::ArtifactMaterialized;
use eaeh_events::{EventEnvelope, outbox};
use eaeh_extensibility::{HookContext, HookPayload, HookPoint, HookRegistry};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, sqlx, to_json, ts};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Repository-relative path of the provenance index (decision M4).
pub const INDEX_FILE: &str = ".eaeh/artifacts.json";
const COMPONENT: &str = "artifact-materializer";

/// One materialization request: write the approved bundle `bundle` into `target_dir`.
#[derive(Debug, Clone)]
pub struct MaterializeRequest {
    pub scope: Scope,
    pub bundle: ArtifactId,
    /// Repository-relative directory, e.g. `docs/architecture/hld`.
    pub target_dir: String,
    pub workflow_instance: Option<String>,
    pub gate: Option<String>,
    /// `{id, approver, role, decided_at, artifact_hash}` of the approval that authorizes it.
    pub approval: Value,
    /// Approved artifacts this one is derived from (`[{gate, artifact_id, name, version, hash, approval_id}]`).
    pub sources: Vec<Value>,
    /// Explicit human override: replace files that were modified locally.
    pub replace_modified: bool,
    /// Audit actor (`system:workflow-engine` or a principal id).
    pub actor: String,
    pub trace: TraceContext,
}

/// Outcome per file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAction {
    pub path: String,
    pub hash: String,
    /// `written`, `replaced`, `replaced_modified`, `unchanged`, `removed`, `kept_modified`.
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterializeReport {
    pub root: String,
    pub target_dir: String,
    pub bundle_id: String,
    pub bundle_version: i64,
    pub bundle_hash: String,
    pub files: Vec<FileAction>,
    pub index: String,
    pub at: String,
}

/// Verified state of a target directory against one bundle ([`Materializer::status`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterializationStatus {
    /// `verified` (every file on disk re-hashed and equal), `modified`, `missing`,
    /// `outdated` (the folder holds another version), `failed`, `conflict`,
    /// `not_materialized`, `no_workspace`.
    pub state: String,
    pub explanation: String,
    pub root: Option<String>,
    pub target_dir: String,
    pub files: Vec<FileCheck>,
    /// Bundle currently materialized in `target_dir`, if any.
    pub materialized_bundle: Option<String>,
    pub last_success_at: Option<String>,
    pub last_attempt: Option<AttemptRecord>,
    /// When the files were re-hashed for this report.
    pub checked_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileCheck {
    pub path: String,
    pub expected_hash: String,
    pub actual_hash: Option<String>,
    /// `verified`, `modified`, `missing`.
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub id: String,
    pub bundle_id: String,
    pub bundle_version: i64,
    pub status: String,
    pub detail: String,
    pub actor_id: String,
    pub created_at: String,
}

/// Registered application repository of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRegistration {
    pub root: String,
    pub registered_by: String,
    pub registered_at: String,
}

#[derive(Debug, Clone)]
pub struct Materializer {
    db: Db,
    artifacts: ArtifactStore,
    hooks: Arc<HookRegistry>,
}

/// Validate a repository-relative directory or file path. Rejects absolute paths, `..`,
/// empty components, `.git` and EAEH temporary names.
fn safe_relative(rel: &str) -> Result<Vec<String>> {
    if rel.is_empty() || rel.len() > 1024 || rel.contains('\0') || rel.contains('\\') {
        return Err(HarnessError::forbidden(format!("unsafe workspace path '{rel}'")));
    }
    let mut parts = vec![];
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(s) => {
                let s = s
                    .to_str()
                    .ok_or_else(|| HarnessError::forbidden("workspace path must be UTF-8"))?;
                if s == ".git" || s.starts_with(".eaeh-") {
                    return Err(HarnessError::forbidden(format!(
                        "reserved path component in '{rel}'"
                    )));
                }
                parts.push(s.to_string());
            }
            _ => {
                return Err(HarnessError::forbidden(format!(
                    "path '{rel}' must be relative to the workspace without '.' or '..'"
                )));
            }
        }
    }
    if parts.is_empty() {
        return Err(HarnessError::forbidden(format!("unsafe workspace path '{rel}'")));
    }
    Ok(parts)
}

/// Resolve `rel` under `root`, rejecting any symlink on the existing part of the path.
fn resolve(root: &Path, rel: &str) -> Result<PathBuf> {
    let mut cur = root.to_path_buf();
    for p in safe_relative(rel)? {
        cur.push(p);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(HarnessError::forbidden(format!(
                    "symlink on workspace path rejected: {rel}"
                )));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(cur)
}

fn hash_file(path: &Path) -> Result<Option<String>> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Ok(Some(sha256_hex(&std::fs::read(path)?))),
        Ok(_) => Err(HarnessError::conflict(format!(
            "{} exists and is not a regular file",
            path.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Atomic write (temporary file in the same directory, fsync, rename), then re-read and
/// verify the hash.
fn write_atomic(root: &Path, rel: &str, data: &[u8], expected_hash: &str) -> Result<()> {
    let path = resolve(root, rel)?;
    let parent = path
        .parent()
        .ok_or_else(|| HarnessError::invalid_input("no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    // Re-check after directory creation: no symlink introduced on the path.
    let path = resolve(root, rel)?;
    let tmp = parent.join(format!(".eaeh-tmp-{}", uuid::Uuid::now_v7()));
    let res = (|| -> Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res?;
    match hash_file(&path)? {
        Some(h) if h == expected_hash => Ok(()),
        _ => Err(HarnessError::integrity(format!(
            "{rel}: written content does not match the approved hash"
        ))),
    }
}

impl Materializer {
    pub fn new(db: Db, artifacts: ArtifactStore, hooks: Arc<HookRegistry>) -> Self {
        Self { db, artifacts, hooks }
    }

    /// Register (or re-register) the application repository root of a project. The root
    /// must be an existing directory; it is stored canonicalized.
    pub async fn register_workspace(
        &self,
        scope: Scope,
        root: &Path,
        by: &str,
    ) -> Result<WorkspaceRegistration> {
        if !root.is_absolute() {
            return Err(HarnessError::invalid_input(
                "workspace root must be an absolute path",
            ));
        }
        let canon = root
            .canonicalize()
            .map_err(|e| HarnessError::invalid_input(format!("workspace root {}: {e}", root.display())))?;
        if !canon.is_dir() {
            return Err(HarnessError::invalid_input("workspace root must be a directory"));
        }
        let root_s = canon.to_string_lossy().to_string();
        if let Some(r) = self.workspace(scope).await?
            && r.root == root_s
        {
            return Ok(r);
        }
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO project_workspaces (tenant_id, project_id, root_path, registered_by, registered_at) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT (tenant_id, project_id) DO UPDATE SET root_path = excluded.root_path, \
             registered_by = excluded.registered_by, registered_at = excluded.registered_at",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(&root_s)
        .bind(by)
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        let mut e = AuditEntry::system(
            COMPONENT,
            "project.workspace_register",
            "project",
            scope.project.to_string(),
            AuditOutcome::Succeeded,
        )
        .scope(scope)
        .details(json!({"root": root_s}));
        e.actor_id = by.to_string();
        self.db.audit_in(&mut tx, e).await?;
        tx.commit().await?;
        Ok(WorkspaceRegistration {
            root: root_s,
            registered_by: by.into(),
            registered_at: now,
        })
    }

    pub async fn workspace(&self, scope: Scope) -> Result<Option<WorkspaceRegistration>> {
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT root_path, registered_by, registered_at FROM project_workspaces WHERE tenant_id = ? AND project_id = ?",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .fetch_optional(self.db.pool())
        .await
        .db()?;
        Ok(
            row.map(|(root, registered_by, registered_at)| WorkspaceRegistration {
                root,
                registered_by,
                registered_at,
            }),
        )
    }

    /// Materialize an approved bundle. Idempotent: re-running with the same bundle reports
    /// every file `unchanged`. On a conflict nothing is written.
    pub async fn materialize(&self, req: &MaterializeRequest) -> Result<MaterializeReport> {
        let mut root_seen = None;
        let mut bundle_seen = None;
        let res = self.try_materialize(req, &mut root_seen, &mut bundle_seen).await;
        let (status, detail, files) = match &res {
            Ok(r) => ("succeeded", "materialized".to_string(), r.files.clone()),
            Err(e) if e.kind == eaeh_core::ErrorKind::Conflict => ("conflict", e.message.clone(), vec![]),
            Err(e) => ("failed", e.message.clone(), vec![]),
        };
        let (bname, bver, bhash) = bundle_seen.unwrap_or_default();
        self.record_attempt(
            req,
            root_seen.as_deref(),
            (&bname, bver, &bhash),
            status,
            &detail,
            &files,
        )
        .await?;
        res
    }

    async fn try_materialize(
        &self,
        req: &MaterializeRequest,
        root_seen: &mut Option<String>,
        bundle_seen: &mut Option<(String, i64, String)>,
    ) -> Result<MaterializeReport> {
        let scope = req.scope;
        let dir = safe_relative(&req.target_dir)?.join("/");
        if dir == ".eaeh" || dir.starts_with(".eaeh/") {
            return Err(HarnessError::forbidden(
                "target directory must not be inside .eaeh/",
            ));
        }
        let reg = self.workspace(scope).await?.ok_or_else(|| {
            HarnessError::unavailable(
                "no application workspace is registered for this project; run `eaeh artifact sync` (or `eaeh project init`) inside the project repository",
            )
        })?;
        *root_seen = Some(reg.root.clone());
        let root = PathBuf::from(&reg.root);
        if !root.is_dir() {
            return Err(HarnessError::unavailable(format!(
                "registered workspace {} is not accessible from this process",
                reg.root
            )));
        }
        let root = root.canonicalize()?;

        // The authoritative bundle and its files (each verified against the manifest).
        let (bmeta, bytes) = self.artifacts.read(scope, req.bundle).await?;
        *bundle_seen = Some((bmeta.name.clone(), bmeta.version, bmeta.content_hash.clone()));
        let manifest: Value = serde_json::from_slice(&bytes)
            .map_err(|_| HarnessError::invalid_input("artifact is not a bundle manifest"))?;
        let entries = manifest
            .get("files")
            .and_then(Value::as_object)
            .ok_or_else(|| HarnessError::invalid_input("artifact is not a bundle manifest"))?;
        let mut planned: Vec<(String, String, String, Vec<u8>)> = vec![]; // (rel, artifact id, hash, bytes)
        for (path, f) in entries {
            safe_relative(path)?;
            let fid: ArtifactId = f
                .get("artifact_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .parse()?;
            let (fm, content) = self.artifacts.read(scope, fid).await?;
            if Some(fm.content_hash.as_str()) != f.get("hash").and_then(Value::as_str) {
                return Err(HarnessError::integrity(format!(
                    "{path}: hash differs from the bundle manifest"
                )));
            }
            planned.push((format!("{dir}/{path}"), fid.to_string(), fm.content_hash, content));
        }

        // Pre-flight: decide every file before writing anything (all or nothing).
        let owned = self.owned(scope).await?;
        let mut actions = vec![];
        let mut conflicts = vec![];
        for (rel, _, hash, _) in &planned {
            let target = resolve(&root, rel)?;
            let action = match hash_file(&target) {
                Err(e) => {
                    conflicts.push(format!("{rel} ({})", e.message));
                    continue;
                }
                Ok(None) => "written",
                Ok(Some(h)) if &h == hash => "unchanged",
                Ok(Some(h)) if owned.get(rel).map(|(oh, _)| oh == &h).unwrap_or(false) => "replaced",
                Ok(Some(_)) if req.replace_modified => "replaced_modified",
                Ok(Some(_)) => {
                    conflicts.push(if owned.contains_key(rel) {
                        format!("{rel} (modified locally since EAEH wrote it)")
                    } else {
                        format!("{rel} (exists and was not written by EAEH)")
                    });
                    continue;
                }
            };
            actions.push(action);
        }
        if !conflicts.is_empty() {
            return Err(HarnessError::conflict(format!(
                "not written – existing files would be overwritten: {}. Move or commit them, or re-run `eaeh artifact sync --replace-modified` to replace them with the approved version",
                conflicts.join(", ")
            )));
        }

        let now = ts(self.db.now());
        let mut files = vec![];
        for ((rel, fid, hash, content), action) in planned.iter().zip(actions) {
            if action != "unchanged" {
                self.file_hook(HookPoint::BeforeFileModification, req, rel)
                    .await
                    .into_result()?;
                write_atomic(&root, rel, content, hash)?;
                self.file_hook(HookPoint::AfterFileModification, req, rel).await;
            }
            self.own(scope, rel, hash, fid, &req.bundle.to_string(), &dir, &now)
                .await?;
            files.push(FileAction {
                path: rel.clone(),
                hash: hash.clone(),
                action: action.into(),
            });
        }
        // Files of a previous version in this folder that the new version no longer has.
        for (rel, (oh, odir)) in &owned {
            if odir != &dir || planned.iter().any(|p| &p.0 == rel) {
                continue;
            }
            let target = resolve(&root, rel)?;
            let action = match hash_file(&target)? {
                Some(h) if &h == oh => {
                    self.file_hook(HookPoint::BeforeFileModification, req, rel)
                        .await
                        .into_result()?;
                    std::fs::remove_file(&target)?;
                    self.file_hook(HookPoint::AfterFileModification, req, rel).await;
                    self.disown(scope, rel).await?;
                    "removed"
                }
                None => {
                    self.disown(scope, rel).await?;
                    "removed"
                }
                Some(_) => "kept_modified",
            };
            files.push(FileAction {
                path: rel.clone(),
                hash: oh.clone(),
                action: action.into(),
            });
        }

        self.write_index(&root, req, &bmeta, &dir, &files, &now)?;
        Ok(MaterializeReport {
            root: root.to_string_lossy().to_string(),
            target_dir: dir,
            bundle_id: req.bundle.to_string(),
            bundle_version: bmeta.version,
            bundle_hash: bmeta.content_hash,
            files,
            index: INDEX_FILE.into(),
            at: now,
        })
    }

    fn write_index(
        &self,
        root: &Path,
        req: &MaterializeRequest,
        bmeta: &crate::ArtifactMeta,
        dir: &str,
        files: &[FileAction],
        now: &str,
    ) -> Result<()> {
        let path = resolve(root, INDEX_FILE)?;
        let mut index = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice::<Value>(&b).map_err(|_| {
                HarnessError::conflict(format!(
                    "{INDEX_FILE} exists but is not valid JSON; fix or remove it"
                ))
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
            Err(e) => return Err(e.into()),
        };
        if !index.is_object() {
            return Err(HarnessError::conflict(format!(
                "{INDEX_FILE} is not a JSON object"
            )));
        }
        index["schema"] = json!(1);
        index["description"] = json!(
            "Approved EAEH artifacts materialized in this repository. Generated by EAEH; the EAEH artifact store is authoritative. Every file listed here is byte-identical to the approved artifact with the given hash."
        );
        index["project"] = json!(req.scope.project.to_string());
        if !index.get("documents").map(Value::is_object).unwrap_or(false) {
            index["documents"] = json!({});
        }
        let file_map: serde_json::Map<String, Value> = files
            .iter()
            .filter(|f| f.action != "removed" && f.action != "kept_modified")
            .map(|f| (f.path.clone(), json!(f.hash)))
            .collect();
        index["documents"][dir] = json!({
            "artifact": {"id": req.bundle.to_string(), "name": bmeta.name, "version": bmeta.version, "hash": bmeta.content_hash, "created_at": bmeta.created_at},
            "workflow_instance": req.workflow_instance,
            "gate": req.gate,
            "approval": req.approval,
            "sources": req.sources,
            "materialized_at": now,
            "files": file_map,
        });
        let bytes = serde_json::to_vec_pretty(&index)?;
        let hash = sha256_hex(&bytes);
        write_atomic(root, INDEX_FILE, &bytes, &hash)
    }

    async fn file_hook(
        &self,
        point: HookPoint,
        req: &MaterializeRequest,
        rel: &str,
    ) -> eaeh_extensibility::HookDispatchReport {
        self.hooks
            .dispatch(&HookContext {
                point,
                scope: Some(req.scope),
                trace: req.trace,
                actor: Some(req.actor.clone()),
                subject: rel.to_string(),
                payload: HookPayload::File {
                    workspace_id: format!("project:{}", req.scope.project),
                    path: rel.to_string(),
                },
            })
            .await
    }

    /// `path → (hash, target_dir)` of files EAEH wrote into the project's workspace.
    async fn owned(&self, scope: Scope) -> Result<std::collections::BTreeMap<String, (String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT path, content_hash, target_dir FROM materialized_files WHERE tenant_id = ? AND project_id = ?",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .fetch_all(self.db.pool())
        .await
        .db()?;
        Ok(rows.into_iter().map(|(p, h, d)| (p, (h, d))).collect())
    }

    #[allow(clippy::too_many_arguments)]
    async fn own(
        &self,
        scope: Scope,
        rel: &str,
        hash: &str,
        fid: &str,
        bundle: &str,
        dir: &str,
        now: &str,
    ) -> Result<()> {
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO materialized_files (tenant_id, project_id, path, content_hash, artifact_id, bundle_id, target_dir, written_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (tenant_id, project_id, path) DO UPDATE SET \
             content_hash = excluded.content_hash, artifact_id = excluded.artifact_id, bundle_id = excluded.bundle_id, \
             target_dir = excluded.target_dir, written_at = excluded.written_at",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(rel)
        .bind(hash)
        .bind(fid)
        .bind(bundle)
        .bind(dir)
        .bind(now)
        .execute(tx.conn())
        .await
        .db()?;
        tx.commit().await
    }

    async fn disown(&self, scope: Scope, rel: &str) -> Result<()> {
        let mut tx = self.db.write().await?;
        sqlx::query("DELETE FROM materialized_files WHERE tenant_id = ? AND project_id = ? AND path = ?")
            .bind(scope.tenant.to_string())
            .bind(scope.project.to_string())
            .bind(rel)
            .execute(tx.conn())
            .await
            .db()?;
        tx.commit().await
    }

    async fn record_attempt(
        &self,
        req: &MaterializeRequest,
        root: Option<&str>,
        bundle: (&str, i64, &str),
        status: &str,
        detail: &str,
        files: &[FileAction],
    ) -> Result<()> {
        let id = uuid::Uuid::now_v7().to_string();
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO artifact_materializations (id, tenant_id, project_id, bundle_id, bundle_name, bundle_version, bundle_hash, \
             workflow_instance_id, gate, approval_id, target_dir, root_path, status, detail, files, actor_id, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(req.scope.tenant.to_string())
        .bind(req.scope.project.to_string())
        .bind(req.bundle.to_string())
        .bind(bundle.0)
        .bind(bundle.1)
        .bind(bundle.2)
        .bind(&req.workflow_instance)
        .bind(&req.gate)
        .bind(req.approval.get("id").and_then(Value::as_str))
        .bind(&req.target_dir)
        .bind(root)
        .bind(status)
        .bind(detail)
        .bind(to_json(&files)?)
        .bind(&req.actor)
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        let outcome = match status {
            "succeeded" => AuditOutcome::Succeeded,
            "conflict" => AuditOutcome::Denied,
            _ => AuditOutcome::Failed,
        };
        let mut e = AuditEntry::system(
            COMPONENT,
            "artifact.materialize",
            "artifact",
            req.bundle.to_string(),
            outcome,
        )
        .scope(req.scope)
        .correlation(req.trace.correlation)
        .details(json!({
            "attempt": id, "target_dir": req.target_dir, "root": root, "status": status, "detail": detail,
            "workflow_instance": req.workflow_instance, "gate": req.gate, "approval": req.approval.get("id"),
            "replace_modified": req.replace_modified, "files": files,
        }));
        e.actor_id = req.actor.clone();
        if !req.actor.starts_with("system:") {
            e.actor_kind = "human".into();
        }
        self.db.audit_in(&mut tx, e).await?;
        let ev = EventEnvelope::new(
            &ArtifactMaterialized {
                bundle_id: req.bundle.to_string(),
                target_dir: req.target_dir.clone(),
                files: u32::try_from(files.len()).unwrap_or(u32::MAX),
                status: status.into(),
            },
            COMPONENT,
            Some(req.scope),
            req.trace,
            self.db.now(),
        )?;
        outbox::enqueue_in(&mut tx, &ev).await?;
        tx.commit().await
    }

    /// Latest attempts for a target directory (newest first).
    pub async fn attempts(&self, scope: Scope, target_dir: &str, limit: i64) -> Result<Vec<AttemptRecord>> {
        let rows: Vec<(String, String, i64, String, String, String, String)> = sqlx::query_as(
            "SELECT id, bundle_id, bundle_version, status, detail, actor_id, created_at FROM artifact_materializations \
             WHERE tenant_id = ? AND project_id = ? AND target_dir = ? ORDER BY created_at DESC, id DESC LIMIT ?",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(target_dir)
        .bind(limit)
        .fetch_all(self.db.pool())
        .await
        .db()?;
        Ok(rows
            .into_iter()
            .map(
                |(id, bundle_id, bundle_version, status, detail, actor_id, created_at)| AttemptRecord {
                    id,
                    bundle_id,
                    bundle_version,
                    status,
                    detail,
                    actor_id,
                    created_at,
                },
            )
            .collect())
    }

    /// Verified state of `target_dir` for `bundle`. Files on disk are re-hashed now; the
    /// state is `verified` only if every file of the bundle is present and equal.
    pub async fn status(
        &self,
        scope: Scope,
        bundle: ArtifactId,
        target_dir: &str,
    ) -> Result<MaterializationStatus> {
        let checked_at = ts(self.db.now());
        let attempts = self.attempts(scope, target_dir, 50).await?;
        let last_attempt = attempts.first().cloned();
        let last_success = attempts.iter().find(|a| a.status == "succeeded").cloned();
        let mut st = MaterializationStatus {
            state: String::new(),
            explanation: String::new(),
            root: None,
            target_dir: target_dir.into(),
            files: vec![],
            materialized_bundle: last_success.as_ref().map(|a| a.bundle_id.clone()),
            last_success_at: last_success.as_ref().map(|a| a.created_at.clone()),
            last_attempt: last_attempt.clone(),
            checked_at,
        };
        let Some(reg) = self.workspace(scope).await? else {
            st.state = "no_workspace".into();
            st.explanation = "No application workspace is registered for this project. Run `eaeh artifact sync` inside the project repository.".into();
            return Ok(st);
        };
        st.root = Some(reg.root.clone());
        let failed_after_success = match (&last_attempt, &last_success) {
            (Some(a), Some(s)) => a.id != s.id && a.bundle_id == bundle.to_string(),
            (Some(a), None) => a.bundle_id == bundle.to_string(),
            _ => false,
        };
        if failed_after_success && let Some(a) = &last_attempt {
            st.state = a.status.clone();
            st.explanation = a.detail.clone();
        }
        let bundle_s = bundle.to_string();
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT path, content_hash, bundle_id FROM materialized_files WHERE tenant_id = ? AND project_id = ? AND target_dir = ? ORDER BY path",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(target_dir)
        .fetch_all(self.db.pool())
        .await
        .db()?;
        if rows.is_empty() {
            if st.state.is_empty() {
                st.state = "not_materialized".into();
                st.explanation = format!("No approved version has been written to {target_dir} yet.");
            }
            return Ok(st);
        }
        if rows.iter().any(|r| r.2 != bundle_s) {
            if st.state.is_empty() {
                st.state = "outdated".into();
                st.explanation = format!(
                    "{target_dir} holds another version ({}); this version has not been written there.",
                    rows[0].2
                );
            }
            return Ok(st);
        }
        let root = PathBuf::from(&reg.root);
        for (path, expected, _) in rows {
            let actual = resolve(&root, &path)
                .ok()
                .and_then(|p| hash_file(&p).ok().flatten());
            let state = match &actual {
                None => "missing",
                Some(a) if a == &expected => "verified",
                Some(_) => "modified",
            };
            st.files.push(FileCheck {
                path,
                expected_hash: expected,
                actual_hash: actual,
                state: state.into(),
            });
        }
        if st.state.is_empty() {
            let missing = st.files.iter().filter(|f| f.state == "missing").count();
            let modified = st.files.iter().filter(|f| f.state == "modified").count();
            (st.state, st.explanation) = if missing > 0 {
                (
                    "missing".into(),
                    format!(
                        "{missing} file(s) are missing from {target_dir}; `eaeh artifact sync` restores them."
                    ),
                )
            } else if modified > 0 {
                (
                    "modified".into(),
                    format!("{modified} file(s) in {target_dir} differ from the approved version."),
                )
            } else {
                (
                    "verified".into(),
                    format!(
                        "All {} file(s) re-hashed and equal to the approved version.",
                        st.files.len()
                    ),
                )
            };
        }
        Ok(st)
    }

    /// The most recent successful attempt that wrote `bundle` (for the index and reports).
    pub async fn last_success(&self, scope: Scope, bundle: ArtifactId) -> Result<Option<AttemptRecord>> {
        let row: Option<(String, String, i64, String, String, String, String)> = sqlx::query_as(
            "SELECT id, bundle_id, bundle_version, status, detail, actor_id, created_at FROM artifact_materializations \
             WHERE tenant_id = ? AND project_id = ? AND bundle_id = ? AND status = 'succeeded' ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(bundle.to_string())
        .fetch_optional(self.db.pool())
        .await
        .db()?;
        Ok(row.map(
            |(id, bundle_id, bundle_version, status, detail, actor_id, created_at)| AttemptRecord {
                id,
                bundle_id,
                bundle_version,
                status,
                detail,
                actor_id,
                created_at,
            },
        ))
    }
}
