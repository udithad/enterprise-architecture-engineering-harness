//! Knowledge service: sources, ingestion, lifecycle, approvals, conflicts.

use crate::model::*;
use crate::parse;
use eaeh_core::hash::sha256_hex;
use eaeh_core::{
    Classification, HarnessError, KnowledgeDocumentId, KnowledgeSourceId, Principal, ProjectId, Result, Role,
    Scope, TaskId, TraceContext,
};
use eaeh_events::catalog::{KnowledgeActivated, KnowledgeConflictDetected, KnowledgeIngested};
use eaeh_events::{DomainEvent, EventEnvelope, outbox};
use eaeh_extensibility::HookRegistry;
use eaeh_governance::authz::{Action, Authorizer, Target};
use eaeh_governance::{ApprovalManager, ApprovalRequest, ApprovalStatus, ApprovalSubject};
use eaeh_intelligence::EmbeddingProvider;
use eaeh_intelligence::embedding::to_bytes;
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, WriteTx, from_json, sqlx, to_json, ts};
use eaeh_validation::builtin::SecretLeakValidator;
use eaeh_validation::{Severity, ValidationSubject, Validator};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KnowledgeConfig {
    /// Dimensions of the built-in hashed n-gram embedder.
    pub embedding_dims: usize,
    /// Hits per query added automatically to agent context.
    pub retrieval_k: usize,
    pub max_document_bytes: usize,
    pub chunk_chars: usize,
    pub candidates: usize,
    pub rrf_k: f64,
    /// Highest classification ordinary human readers may retrieve.
    pub human_max_classification: Classification,
    /// Highest classification agents may retrieve into context.
    pub agent_max_classification: Classification,
    /// Exclude stale (past review-by) mandatory standards from context.
    pub exclude_stale_standards: bool,
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            embedding_dims: 384,
            retrieval_k: 4,
            max_document_bytes: 1024 * 1024,
            chunk_chars: 1200,
            candidates: 50,
            rrf_k: 60.0,
            human_max_classification: Classification::Confidential,
            agent_max_classification: Classification::Confidential,
            exclude_stale_standards: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub id: String,
    pub subject: String,
    pub document_a: String,
    pub document_b: String,
    pub detected_at: String,
    pub resolution: String,
}

#[derive(Clone)]
pub struct KnowledgeService {
    pub(crate) db: Db,
    pub(crate) authz: Arc<dyn Authorizer>,
    pub(crate) approvals: ApprovalManager,
    pub(crate) hooks: Arc<HookRegistry>,
    pub(crate) embedder: Arc<dyn EmbeddingProvider>,
    pub(crate) cfg: KnowledgeConfig,
}

impl std::fmt::Debug for KnowledgeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KnowledgeService")
            .field("embedder", &self.embedder.id())
            .finish_non_exhaustive()
    }
}

pub(crate) async fn emit<E: DomainEvent>(
    db: &Db,
    tx: &mut WriteTx,
    ev: &E,
    scope: Option<Scope>,
    trace: TraceContext,
) -> Result<()> {
    let env = EventEnvelope::new(ev, "knowledge", scope, trace, db.now())?;
    outbox::enqueue_in(tx, &env).await.map(|_| ())
}

impl KnowledgeService {
    pub fn new(
        db: Db,
        authz: Arc<dyn Authorizer>,
        approvals: ApprovalManager,
        hooks: Arc<HookRegistry>,
        embedder: Arc<dyn EmbeddingProvider>,
        cfg: KnowledgeConfig,
    ) -> Self {
        Self {
            db,
            authz,
            approvals,
            hooks,
            embedder,
            cfg,
        }
    }

    pub fn config(&self) -> &KnowledgeConfig {
        &self.cfg
    }

    // ------------------------------------------------------------------ sources

    pub async fn register_source(
        &self,
        actor: &Principal,
        name: &str,
        kind: SourceKind,
        description: &str,
    ) -> Result<Source> {
        self.authz
            .authorize(actor, Action::KnowledgeManage, Target::Tenant(actor.tenant))?;
        eaeh_core::ids::validate_slug(name)
            .map_err(|m| HarnessError::invalid_input(format!("source name: {m}")))?;
        if description.trim().is_empty() || description.len() > 2000 {
            return Err(HarnessError::invalid_input(
                "source description must be 1..=2000 bytes",
            ));
        }
        let id = KnowledgeSourceId::new();
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        sqlx::query("INSERT INTO knowledge_sources (id, tenant_id, name, kind, description, owner_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(id.to_string())
            .bind(actor.tenant.to_string())
            .bind(name)
            .bind(kind.as_str())
            .bind(description)
            .bind(actor.id.as_str())
            .bind(&now)
            .execute(tx.conn())
            .await
            .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "knowledge.source.register",
                    "knowledge_source",
                    name,
                    AuditOutcome::Succeeded,
                )
                .details(json!({"kind": kind.as_str()})),
            )
            .await?;
        tx.commit().await?;
        Ok(Source {
            id: id.to_string(),
            name: name.into(),
            kind,
            description: description.into(),
            owner_id: actor.id.to_string(),
            created_at: now,
        })
    }

    pub async fn sources(&self, actor: &Principal) -> Result<Vec<Source>> {
        self.authz
            .authorize(actor, Action::KnowledgeRead, Target::Tenant(actor.tenant))?;
        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, name, kind, description, owner_id, created_at FROM knowledge_sources WHERE tenant_id = ? ORDER BY name",
        )
        .bind(actor.tenant.to_string())
        .fetch_all(self.db.pool())
        .await
        .db()?;
        rows.into_iter()
            .map(|(id, name, kind, description, owner_id, created_at)| {
                Ok(Source {
                    id,
                    name,
                    kind: SourceKind::parse(&kind)?,
                    description,
                    owner_id,
                    created_at,
                })
            })
            .collect()
    }

    async fn source_by_name(&self, tenant: &str, name: &str) -> Result<(String, SourceKind)> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT id, kind FROM knowledge_sources WHERE tenant_id = ? AND name = ?")
                .bind(tenant)
                .bind(name)
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        let (id, kind) =
            row.ok_or_else(|| HarnessError::not_found(format!("knowledge source '{name}' not found")))?;
        Ok((id, SourceKind::parse(&kind)?))
    }

    // ------------------------------------------------------------------ ingestion

    /// Ingest a new document version. Validation failures quarantine the document (it is
    /// stored for inspection but never indexed). Documents from `standard` sources wait
    /// for human approval; all others become active immediately.
    pub async fn ingest(
        &self,
        actor: &Principal,
        req: IngestRequest,
        trace: TraceContext,
    ) -> Result<DocumentMeta> {
        let gov_scope = Scope::new(actor.tenant, req.governance_project);
        self.authz
            .authorize(actor, Action::KnowledgeManage, Target::Scope(gov_scope))?;
        eaeh_core::ids::validate_slug(&req.subject)
            .map_err(|m| HarnessError::invalid_input(format!("subject: {m}")))?;
        if req.title.trim().is_empty() || req.title.len() > 300 {
            return Err(HarnessError::invalid_input("title must be 1..=300 bytes"));
        }
        if req.content.len() > self.cfg.max_document_bytes {
            return Err(HarnessError::invalid_input(format!(
                "document exceeds {} bytes",
                self.cfg.max_document_bytes
            )));
        }
        for t in &req.tags {
            eaeh_core::ids::validate_slug(t)
                .map_err(|m| HarnessError::invalid_input(format!("tag '{t}': {m}")))?;
        }
        if let Some(r) = &req.review_by {
            eaeh_store::parse_ts(r).map_err(|_| HarnessError::invalid_input("review_by must be RFC3339"))?;
        }
        self.check_scope(actor, &req.scope).await?;
        let (source_id, kind) = self
            .source_by_name(&actor.tenant.to_string(), &req.source)
            .await?;
        if req.mandatory
            && !(kind == SourceKind::Standard
                && matches!(req.scope, KnowledgeScope::Enterprise | KnowledgeScope::Domain(_)))
        {
            return Err(HarnessError::invalid_input(
                "only enterprise- or domain-scoped documents of a 'standard' source can be mandatory",
            ));
        }

        // Validation gate.
        let mut findings = vec![];
        let report = SecretLeakValidator
            .validate(&ValidationSubject {
                kind: "knowledge_document".into(),
                name: req.subject.clone(),
                content: serde_json::Value::String(req.content.clone()),
                metadata: Default::default(),
            })
            .await;
        findings.extend(
            report
                .findings
                .iter()
                .filter(|f| f.severity >= Severity::Error)
                .cloned(),
        );
        let chunks = match parse::chunk(req.format, &req.content, self.cfg.chunk_chars) {
            Ok(c) => c,
            Err(e) => return Err(e),
        };
        let quarantined = !findings.is_empty();
        let status = if quarantined {
            DocStatus::Quarantined
        } else if kind == SourceKind::Standard {
            DocStatus::PendingApproval
        } else {
            DocStatus::Active
        };

        let content_hash = sha256_hex(req.content.as_bytes());
        let embeddings = if quarantined {
            vec![]
        } else {
            let texts: Vec<String> = chunks
                .iter()
                .map(|c| format!("{}\n{}", c.heading, c.text))
                .collect();
            self.embedder.embed(&texts).await?
        };
        let doc_id = KnowledgeDocumentId::new();
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        let (max_v,): (Option<i64>,) = sqlx::query_as(
            "SELECT MAX(version) FROM knowledge_documents WHERE source_id = ? AND subject = ?",
        )
        .bind(&source_id)
        .bind(&req.subject)
        .fetch_one(tx.conn())
        .await
        .db()?;
        let version = max_v.unwrap_or(0) + 1;
        // Quarantined content is NOT stored (it may contain credentials); only metadata,
        // the content hash and the validation findings are kept for the steward.
        let stored_content: Option<String> = if quarantined {
            None
        } else {
            Some(req.content.clone())
        };
        let validation = to_json(&findings)?;
        sqlx::query(
            "INSERT INTO knowledge_documents (id, tenant_id, source_id, subject, version, title, format, content, content_hash, scope_level, \
             scope_ref, classification, classification_rank, tags, mandatory, review_by, status, validation, governance_project, created_by, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(doc_id.to_string())
        .bind(actor.tenant.to_string())
        .bind(&source_id)
        .bind(&req.subject)
        .bind(version)
        .bind(&req.title)
        .bind(req.format.as_str())
        .bind(&stored_content)
        .bind(&content_hash)
        .bind(req.scope.level())
        .bind(req.scope.reference())
        .bind(req.classification.as_str())
        .bind(req.classification.rank())
        .bind(to_json(&req.tags)?)
        .bind(i64::from(req.mandatory))
        .bind(&req.review_by)
        .bind(status.as_str())
        .bind(&validation)
        .bind(req.governance_project.to_string())
        .bind(actor.id.as_str())
        .bind(&now)
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        if !quarantined {
            for (i, (c, vector)) in chunks.iter().zip(embeddings.iter()).enumerate() {
                let chunk_id = format!("{doc_id}:{i}");
                let (rowid,): (i64,) = sqlx::query_as(
                    "INSERT INTO knowledge_chunks (id, document_id, tenant_id, ordinal, heading, text, token_estimate) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING rowid",
                )
                .bind(&chunk_id)
                .bind(doc_id.to_string())
                .bind(actor.tenant.to_string())
                .bind(i64::try_from(i).unwrap_or(i64::MAX))
                .bind(&c.heading)
                .bind(&c.text)
                .bind(i64::try_from(eaeh_intelligence::estimate_tokens(&c.text)).unwrap_or(i64::MAX))
                .fetch_one(tx.conn())
                .await
                .db()?;
                sqlx::query("INSERT INTO knowledge_chunks_fts (rowid, text, heading) VALUES (?, ?, ?)")
                    .bind(rowid)
                    .bind(&c.text)
                    .bind(&c.heading)
                    .execute(tx.conn())
                    .await
                    .db()?;
                sqlx::query(
                    "INSERT INTO knowledge_embeddings (chunk_id, embedder, dims, vector) VALUES (?, ?, ?, ?)",
                )
                .bind(&chunk_id)
                .bind(self.embedder.id())
                .bind(i64::try_from(vector.len()).unwrap_or(0))
                .bind(to_bytes(vector))
                .execute(tx.conn())
                .await
                .db()?;
            }
        }
        let citation = citation_base(&req.source, &req.subject, version);
        let mut approval_id = None;
        if status == DocStatus::PendingApproval {
            let aid = self
                .approvals
                .request_in(
                    &mut tx,
                    ApprovalRequest {
                        scope: gov_scope,
                        subject: ApprovalSubject::KnowledgeDocument {
                            document: doc_id.to_string(),
                            content_hash: content_hash.clone(),
                        },
                        required_role: Role::Approver,
                        authors: vec![actor.id.clone()],
                        requested_by: actor.id.clone(),
                        trace,
                    },
                )
                .await?;
            sqlx::query("UPDATE knowledge_documents SET approval_id = ? WHERE id = ?")
                .bind(aid.to_string())
                .bind(doc_id.to_string())
                .execute(tx.conn())
                .await
                .db()?;
            approval_id = Some(aid.to_string());
        }
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "knowledge.ingest",
                    "knowledge_document",
                    doc_id.to_string(),
                    if quarantined {
                        AuditOutcome::Denied
                    } else {
                        AuditOutcome::Succeeded
                    },
                )
                .scope(gov_scope)
                .correlation(trace.correlation)
                .details(json!({
                    "citation": citation, "status": status.as_str(), "content_sha256": content_hash,
                    "scope": req.scope, "classification": req.classification, "chunks": chunks.len(),
                    "findings": findings.iter().map(|f| f.message.clone()).collect::<Vec<_>>(),
                })),
            )
            .await?;
        emit(
            &self.db,
            &mut tx,
            &KnowledgeIngested {
                document_id: doc_id.to_string(),
                citation_base: citation,
                status: status.as_str().into(),
                chunks: u32::try_from(chunks.len()).unwrap_or(u32::MAX),
            },
            Some(gov_scope),
            trace,
        )
        .await?;
        if status == DocStatus::Active {
            self.activate_in(&mut tx, &doc_id.to_string(), trace).await?;
        }
        tx.commit().await?;
        tracing::info!(document = %doc_id, status = status.as_str(), approval = ?approval_id, "knowledge document ingested");
        self.get_internal(&doc_id.to_string()).await
    }

    async fn check_scope(&self, actor: &Principal, scope: &KnowledgeScope) -> Result<()> {
        match scope {
            KnowledgeScope::Enterprise => Ok(()),
            KnowledgeScope::Domain(d) | KnowledgeScope::Team(d) => eaeh_core::ids::validate_slug(d)
                .map(|_| ())
                .map_err(|m| HarnessError::invalid_input(format!("scope name: {m}"))),
            KnowledgeScope::Project(p) => {
                let s = Scope::new(actor.tenant, *p);
                if !actor.can_access(&s) {
                    return Err(HarnessError::forbidden(
                        "no access to the document's project scope",
                    ));
                }
                let row: Option<(String,)> =
                    sqlx::query_as("SELECT id FROM projects WHERE id = ? AND tenant_id = ?")
                        .bind(p.to_string())
                        .bind(actor.tenant.to_string())
                        .fetch_optional(self.db.pool())
                        .await
                        .db()?;
                row.map(|_| ())
                    .ok_or_else(|| HarnessError::not_found("project not found in tenant"))
            }
            KnowledgeScope::Workflow(w) => {
                let row: Option<(String, String)> =
                    sqlx::query_as("SELECT tenant_id, project_id FROM workflow_instances WHERE id = ?")
                        .bind(w.to_string())
                        .fetch_optional(self.db.pool())
                        .await
                        .db()?;
                match row {
                    Some((t, p))
                        if t == actor.tenant.to_string()
                            && actor.can_access(&Scope::new(actor.tenant, p.parse()?)) =>
                    {
                        Ok(())
                    }
                    _ => Err(HarnessError::not_found("workflow not found in accessible scope")),
                }
            }
        }
    }

    /// Activate a document (inside a transaction): supersede the previous active version
    /// of the same (source, subject) and record conflicts with other sources.
    async fn activate_in(&self, tx: &mut WriteTx, doc_id: &str, trace: TraceContext) -> Result<()> {
        let row = sqlx::query("SELECT tenant_id, source_id, subject, content_hash, scope_level, mandatory FROM knowledge_documents WHERE id = ?")
            .bind(doc_id)
            .fetch_one(tx.conn())
            .await
            .db()?;
        let tenant: String = row.try_get("tenant_id").db()?;
        let source_id: String = row.try_get("source_id").db()?;
        let subject: String = row.try_get("subject").db()?;
        let hash: String = row.try_get("content_hash").db()?;
        let level: String = row.try_get("scope_level").db()?;
        let mandatory: i64 = row.try_get("mandatory").db()?;
        let now = ts(self.db.now());
        let prev: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM knowledge_documents WHERE source_id = ? AND subject = ? AND status = 'active' AND id != ?",
        )
        .bind(&source_id)
        .bind(&subject)
        .bind(doc_id)
        .fetch_optional(tx.conn())
        .await
        .db()?;
        if let Some((p,)) = &prev {
            sqlx::query("UPDATE knowledge_documents SET status = 'superseded', updated_at = ? WHERE id = ?")
                .bind(&now)
                .bind(p)
                .execute(tx.conn())
                .await
                .db()?;
        }
        sqlx::query("UPDATE knowledge_documents SET status = 'active', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(doc_id)
            .execute(tx.conn())
            .await
            .db()?;
        // Conflicts: same subject, other source, different content, still active.
        let others: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT id, scope_level, mandatory FROM knowledge_documents WHERE tenant_id = ? AND subject = ? AND status = 'active' \
             AND source_id != ? AND content_hash != ?",
        )
        .bind(&tenant)
        .bind(&subject)
        .bind(&source_id)
        .bind(&hash)
        .fetch_all(tx.conn())
        .await
        .db()?;
        for (other, other_level, other_mandatory) in others {
            let mine = (mandatory, KnowledgeScope::precedence(&level));
            let theirs = (other_mandatory, KnowledgeScope::precedence(&other_level));
            let resolution = if mine >= theirs {
                format!("{doc_id} prevails (mandatory={mandatory}, scope={level})")
            } else {
                format!("{other} prevails (mandatory={other_mandatory}, scope={other_level})")
            };
            sqlx::query(
                "INSERT INTO knowledge_conflicts (id, tenant_id, subject, document_a, document_b, detected_at, resolution) VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(KnowledgeDocumentId::new().to_string())
            .bind(&tenant)
            .bind(&subject)
            .bind(doc_id)
            .bind(&other)
            .bind(&now)
            .bind(&resolution)
            .execute(tx.conn())
            .await
            .db()?;
            emit(
                &self.db,
                tx,
                &KnowledgeConflictDetected {
                    subject: subject.clone(),
                    document_a: doc_id.into(),
                    document_b: other.clone(),
                },
                None,
                trace,
            )
            .await?;
        }
        emit(
            &self.db,
            tx,
            &KnowledgeActivated {
                document_id: doc_id.into(),
                superseded: prev.map(|p| p.0),
            },
            None,
            trace,
        )
        .await
    }

    /// Apply a decided approval for a knowledge document (called after `ApprovalManager::decide`).
    /// Activation requires the approval to be granted for exactly the stored content hash.
    pub async fn apply_approval(
        &self,
        approval_id: eaeh_core::ApprovalId,
        trace: TraceContext,
    ) -> Result<Option<DocumentMeta>> {
        let rec = self.approvals.get(approval_id).await?;
        if rec.subject_type != "knowledge_document" {
            return Ok(None);
        }
        let doc_id = rec.subject_ref.clone();
        let (status, hash): (String, String) = sqlx::query_as(
            "SELECT status, content_hash FROM knowledge_documents WHERE id = ? AND approval_id = ?",
        )
        .bind(&doc_id)
        .bind(approval_id.to_string())
        .fetch_optional(self.db.pool())
        .await
        .db()?
        .ok_or_else(|| HarnessError::not_found("document for approval not found"))?;
        if status != "pending_approval" {
            return Ok(Some(self.get_internal(&doc_id).await?));
        }
        let mut tx = self.db.write().await?;
        match rec.status {
            ApprovalStatus::Approved if rec.artifact_hash.as_deref() == Some(hash.as_str()) => {
                self.activate_in(&mut tx, &doc_id, trace).await?;
            }
            ApprovalStatus::Rejected => {
                sqlx::query(
                    "UPDATE knowledge_documents SET status = 'rejected', updated_at = ? WHERE id = ?",
                )
                .bind(ts(self.db.now()))
                .bind(&doc_id)
                .execute(tx.conn())
                .await
                .db()?;
            }
            _ => {
                tx.rollback().await?;
                return Ok(Some(self.get_internal(&doc_id).await?));
            }
        }
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    "knowledge",
                    "knowledge.approval_applied",
                    "knowledge_document",
                    doc_id.clone(),
                    AuditOutcome::Recorded,
                )
                .correlation(trace.correlation)
                .details(json!({"approval_id": approval_id.to_string(), "decision": rec.status})),
            )
            .await?;
        tx.commit().await?;
        Ok(Some(self.get_internal(&doc_id).await?))
    }

    pub async fn retire(&self, actor: &Principal, doc_id: &str) -> Result<DocumentMeta> {
        let doc = self.get(actor, doc_id).await?;
        if !matches!(
            doc.status,
            DocStatus::Active | DocStatus::PendingApproval | DocStatus::Superseded
        ) {
            return Err(HarnessError::conflict(format!(
                "cannot retire a {} document",
                doc.status.as_str()
            )));
        }
        let mut tx = self.db.write().await?;
        sqlx::query("UPDATE knowledge_documents SET status = 'retired', updated_at = ? WHERE id = ?")
            .bind(ts(self.db.now()))
            .bind(doc_id)
            .execute(tx.conn())
            .await
            .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "knowledge.retire",
                    "knowledge_document",
                    doc_id,
                    AuditOutcome::Succeeded,
                ),
            )
            .await?;
        tx.commit().await?;
        self.get_internal(doc_id).await
    }

    /// Delete: purge content, chunk text, index entries and vectors; keep a tombstone
    /// (metadata, hash) and the audit trail.
    pub async fn delete(&self, actor: &Principal, doc_id: &str) -> Result<DocumentMeta> {
        let doc = self.get(actor, doc_id).await?;
        if doc.status == DocStatus::Deleted {
            return Err(HarnessError::conflict("document already deleted"));
        }
        let mut tx = self.db.write().await?;
        let chunks: Vec<(i64, String, String, String)> =
            sqlx::query_as("SELECT rowid, id, text, heading FROM knowledge_chunks WHERE document_id = ?")
                .bind(doc_id)
                .fetch_all(tx.conn())
                .await
                .db()?;
        for (rowid, chunk_id, text, heading) in &chunks {
            sqlx::query("INSERT INTO knowledge_chunks_fts (knowledge_chunks_fts, rowid, text, heading) VALUES ('delete', ?, ?, ?)")
                .bind(rowid)
                .bind(text)
                .bind(heading)
                .execute(tx.conn())
                .await
                .db()?;
            sqlx::query("DELETE FROM knowledge_embeddings WHERE chunk_id = ?")
                .bind(chunk_id)
                .execute(tx.conn())
                .await
                .db()?;
        }
        sqlx::query("UPDATE knowledge_chunks SET text = '', heading = '' WHERE document_id = ?")
            .bind(doc_id)
            .execute(tx.conn())
            .await
            .db()?;
        sqlx::query(
            "UPDATE knowledge_documents SET content = NULL, status = 'deleted', updated_at = ? WHERE id = ?",
        )
        .bind(ts(self.db.now()))
        .bind(doc_id)
        .execute(tx.conn())
        .await
        .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "knowledge.delete",
                    "knowledge_document",
                    doc_id,
                    AuditOutcome::Succeeded,
                )
                .details(json!({"content_sha256": doc.content_hash, "purged_chunks": chunks.len()})),
            )
            .await?;
        tx.commit().await?;
        self.get_internal(doc_id).await
    }

    /// Metadata of a document (stewards/admins only).
    pub async fn get(&self, actor: &Principal, doc_id: &str) -> Result<DocumentMeta> {
        let doc = self.get_internal(doc_id).await?;
        let tenant: (String,) = sqlx::query_as("SELECT tenant_id FROM knowledge_documents WHERE id = ?")
            .bind(doc_id)
            .fetch_one(self.db.pool())
            .await
            .db()?;
        if tenant.0 != actor.tenant.to_string() {
            return Err(HarnessError::not_found("document not found"));
        }
        self.authz
            .authorize(actor, Action::KnowledgeManage, Target::Tenant(actor.tenant))?;
        Ok(doc)
    }

    /// The document awaiting approval `approval_id`, with its content, for the person who
    /// must review it. Authorized by `approval.read` on the approval's governance scope
    /// (approvers can see exactly what they are asked to approve; no knowledge.manage).
    pub async fn document_for_approval(
        &self,
        actor: &Principal,
        approval_id: &str,
    ) -> Result<(DocumentMeta, Option<String>)> {
        let row: Option<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT id, governance_project, content FROM knowledge_documents WHERE approval_id = ? AND tenant_id = ?",
        )
        .bind(approval_id)
        .bind(actor.tenant.to_string())
        .fetch_optional(self.db.pool())
        .await
        .db()?;
        let (id, gov, content) =
            row.ok_or_else(|| HarnessError::not_found("no document for this approval"))?;
        let scope = eaeh_core::Scope::new(actor.tenant, gov.parse()?);
        self.authz
            .authorize(actor, Action::ApprovalRead, Target::Scope(scope))?;
        Ok((self.get_internal(&id).await?, content))
    }

    pub async fn list(
        &self,
        actor: &Principal,
        status: Option<DocStatus>,
        limit: u32,
    ) -> Result<Vec<DocumentMeta>> {
        self.authz
            .authorize(actor, Action::KnowledgeManage, Target::Tenant(actor.tenant))?;
        let ids: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM knowledge_documents WHERE tenant_id = ? AND (? IS NULL OR status = ?) ORDER BY created_at DESC LIMIT ?",
        )
        .bind(actor.tenant.to_string())
        .bind(status.map(|s| s.as_str()))
        .bind(status.map(|s| s.as_str()))
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(self.db.pool())
        .await
        .db()?;
        let mut out = vec![];
        for (id,) in ids {
            out.push(self.get_internal(&id).await?);
        }
        Ok(out)
    }

    pub async fn conflicts(&self, actor: &Principal) -> Result<Vec<Conflict>> {
        self.authz
            .authorize(actor, Action::KnowledgeManage, Target::Tenant(actor.tenant))?;
        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, subject, document_a, document_b, detected_at, resolution FROM knowledge_conflicts WHERE tenant_id = ? ORDER BY detected_at",
        )
        .bind(actor.tenant.to_string())
        .fetch_all(self.db.pool())
        .await
        .db()?;
        Ok(rows
            .into_iter()
            .map(
                |(id, subject, document_a, document_b, detected_at, resolution)| Conflict {
                    id,
                    subject,
                    document_a,
                    document_b,
                    detected_at,
                    resolution,
                },
            )
            .collect())
    }

    pub(crate) async fn get_internal(&self, doc_id: &str) -> Result<DocumentMeta> {
        let row = sqlx::query(
            "SELECT d.*, s.name AS source_name, s.kind AS source_kind, (SELECT COUNT(*) FROM knowledge_chunks c WHERE c.document_id = d.id) AS n_chunks \
             FROM knowledge_documents d JOIN knowledge_sources s ON s.id = d.source_id WHERE d.id = ?",
        )
        .bind(doc_id)
        .fetch_optional(self.db.pool())
        .await
        .db()?
        .ok_or_else(|| HarnessError::not_found("document not found"))?;
        let format: String = row.try_get("format").db()?;
        Ok(DocumentMeta {
            id: row.try_get("id").db()?,
            source: row.try_get("source_name").db()?,
            source_kind: SourceKind::parse(&row.try_get::<String, _>("source_kind").db()?)?,
            subject: row.try_get("subject").db()?,
            version: row.try_get("version").db()?,
            title: row.try_get("title").db()?,
            format: serde_json::from_value(json!(format)).map_err(|_| HarnessError::integrity("format"))?,
            content_hash: row.try_get("content_hash").db()?,
            scope: KnowledgeScope::parse(
                &row.try_get::<String, _>("scope_level").db()?,
                &row.try_get::<String, _>("scope_ref").db()?,
            )?,
            classification: Classification::parse(&row.try_get::<String, _>("classification").db()?)?,
            tags: from_json(&row.try_get::<String, _>("tags").db()?)?,
            mandatory: row.try_get::<i64, _>("mandatory").db()? != 0,
            review_by: row.try_get("review_by").db()?,
            status: DocStatus::parse(&row.try_get::<String, _>("status").db()?)?,
            validation: from_json(&row.try_get::<String, _>("validation").db()?)?,
            approval_id: row.try_get("approval_id").db()?,
            chunks: row.try_get("n_chunks").db()?,
            created_by: row.try_get("created_by").db()?,
            created_at: row.try_get("created_at").db()?,
        })
    }

    // ------------------------------------------------------------------ retrieval contexts

    /// Retrieval context of a human principal reading within a project.
    pub async fn context_for_human(&self, actor: &Principal, project: ProjectId) -> Result<RetrievalContext> {
        let scope = Scope::new(actor.tenant, project);
        self.authz
            .authorize(actor, Action::KnowledgeRead, Target::Scope(scope))?;
        let teams: Vec<String> = {
            let row: Option<(String,)> = sqlx::query_as("SELECT teams FROM principals WHERE id = ?")
                .bind(actor.id.as_str())
                .fetch_optional(self.db.pool())
                .await
                .db()?;
            row.map(|(t,)| from_json(&t)).transpose()?.unwrap_or_default()
        };
        let privileged = actor.has_role(Role::Admin) || actor.has_role(Role::KnowledgeSteward);
        Ok(RetrievalContext {
            tenant: actor.tenant,
            project: Some(project),
            domain: self.project_domain(scope).await?,
            teams,
            workflow: None,
            max_classification: if privileged {
                Classification::Restricted
            } else {
                self.cfg.human_max_classification
            },
            principal_id: actor.id.to_string(),
        })
    }

    /// Retrieval context of an agent principal (project-bound). When the agent runs a
    /// workflow task, workflow-scoped knowledge of that instance is visible too.
    pub async fn context_for_agent(
        &self,
        agent: &Principal,
        scope: Scope,
        task: Option<TaskId>,
    ) -> Result<RetrievalContext> {
        self.authz
            .authorize(agent, Action::KnowledgeRead, Target::Scope(scope))?;
        let workflow = match task {
            Some(t) => {
                let row: Option<(Option<String>,)> =
                    sqlx::query_as("SELECT workflow_instance_id FROM tasks WHERE id = ? AND tenant_id = ? AND project_id = ?")
                        .bind(t.to_string())
                        .bind(scope.tenant.to_string())
                        .bind(scope.project.to_string())
                        .fetch_optional(self.db.pool())
                        .await
                        .db()?;
                row.and_then(|r| r.0).map(|w| w.parse()).transpose()?
            }
            None => None,
        };
        Ok(RetrievalContext {
            tenant: scope.tenant,
            project: Some(scope.project),
            domain: self.project_domain(scope).await?,
            teams: vec![],
            workflow,
            max_classification: self.cfg.agent_max_classification,
            principal_id: agent.id.to_string(),
        })
    }

    async fn project_domain(&self, scope: Scope) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT domain FROM projects WHERE id = ? AND tenant_id = ?")
                .bind(scope.project.to_string())
                .bind(scope.tenant.to_string())
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        Ok(row.and_then(|r| r.0))
    }
}
