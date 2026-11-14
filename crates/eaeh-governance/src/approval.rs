//! Human approval management.
//!
//! Approval decisions are enforced *here*, inside the trusted component, independently of
//! the API layer, prompts or hooks:
//! * only `Human` principals holding the required role may decide (`Authorizer`);
//! * the decider must be inside the approval's tenant/project scope;
//! * separation of duties: no author of the subject (including agent principals that
//!   produced the artifact) may decide;
//! * the decision binds to an exact artifact hash; a changed artifact cannot be approved
//!   under an old approval;
//! * an approval is decided at most once; re-requesting supersedes pending approvals for
//!   the same subject while keeping full history.
//!
//! Approver, role, timestamp, decision, comments, artifact version, workflow and audit
//! event are recorded atomically (master instructions §3.5).

use crate::authz::{Action, Authorizer, Target};
use eaeh_core::{
    ApprovalId, ArtifactId, HarnessError, Principal, PrincipalId, Result, Role, Scope, TraceContext,
    WorkflowInstanceId,
};
use eaeh_events::catalog::{ApprovalChangesRequested, ApprovalGranted, ApprovalRejected, ApprovalRequested};
use eaeh_events::{EventEnvelope, outbox};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, WriteTx, from_json, sqlx, to_json, ts};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ApprovalSubject {
    WorkflowGate {
        instance: WorkflowInstanceId,
        gate: String,
        artifact: Option<ArtifactId>,
        artifact_hash: Option<String>,
    },
    ToolInvocation {
        tool: String,
        /// SHA-256 over tool id, version and canonical input.
        fingerprint: String,
    },
    /// Phase 2: activation of a governed knowledge document (e.g. an enterprise
    /// standard), bound to its content hash.
    KnowledgeDocument { document: String, content_hash: String },
    /// Phase 2: approval of a skill version, bound to its definition digest.
    Skill {
        skill: String,
        version: String,
        digest: String,
    },
}

impl ApprovalSubject {
    pub fn subject_type(&self) -> &'static str {
        match self {
            ApprovalSubject::WorkflowGate { .. } => "workflow_gate",
            ApprovalSubject::ToolInvocation { .. } => "tool_invocation",
            ApprovalSubject::KnowledgeDocument { .. } => "knowledge_document",
            ApprovalSubject::Skill { .. } => "skill",
        }
    }

    pub fn subject_ref(&self) -> String {
        match self {
            ApprovalSubject::WorkflowGate { instance, gate, .. } => format!("{instance}:{gate}"),
            ApprovalSubject::ToolInvocation { tool, fingerprint } => format!("{tool}:{fingerprint}"),
            ApprovalSubject::KnowledgeDocument { document, .. } => document.clone(),
            ApprovalSubject::Skill { skill, version, .. } => format!("{skill}@{version}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub scope: Scope,
    pub subject: ApprovalSubject,
    pub required_role: Role,
    /// Principals who produced the subject; they may never decide it.
    pub authors: Vec<PrincipalId>,
    pub requested_by: PrincipalId,
    pub trace: TraceContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    /// Final rejection.
    Reject,
    /// Send the work back for revision (workflow gates only, Phase 3, ADR-0020).
    RequestChanges,
}

impl ApprovalDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalDecision::Approve => "approve",
            ApprovalDecision::Reject => "reject",
            ApprovalDecision::RequestChanges => "request_changes",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "approve" => Self::Approve,
            "reject" => Self::Reject,
            "request_changes" => Self::RequestChanges,
            _ => {
                return Err(HarnessError::invalid_input(
                    "decision must be approve, reject or request_changes",
                ));
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    ChangesRequested,
    Superseded,
}

impl ApprovalStatus {
    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "pending" => Self::Pending,
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            "changes_requested" => Self::ChangesRequested,
            "superseded" => Self::Superseded,
            _ => return Err(HarnessError::integrity("unknown approval status")),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub approver_id: String,
    pub approver_role: String,
    pub decision: String,
    pub comments: String,
    pub artifact_hash: Option<String>,
    pub decided_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub id: ApprovalId,
    pub scope: Scope,
    pub subject_type: String,
    pub subject_ref: String,
    pub workflow_instance_id: Option<String>,
    pub gate: Option<String>,
    pub artifact_id: Option<String>,
    pub artifact_hash: Option<String>,
    pub authors: Vec<PrincipalId>,
    pub required_role: Role,
    pub requested_by: String,
    pub status: ApprovalStatus,
    pub created_at: String,
    pub decided_at: Option<String>,
    pub decision: Option<DecisionRecord>,
}

#[derive(Debug, Clone)]
pub struct ApprovalManager {
    db: Db,
    authz: Arc<dyn Authorizer>,
}

impl ApprovalManager {
    pub fn new(db: Db, authz: Arc<dyn Authorizer>) -> Self {
        Self { db, authz }
    }

    /// Create an approval request inside the caller's transaction. Pending approvals for
    /// the same subject are superseded (history retained).
    pub async fn request_in(&self, tx: &mut WriteTx, req: ApprovalRequest) -> Result<ApprovalId> {
        let id = ApprovalId::new();
        let now = ts(self.db.now());
        let subject_ref = req.subject.subject_ref();
        sqlx::query("UPDATE approvals SET status = 'superseded' WHERE subject_type = ? AND subject_ref = ? AND status = 'pending'")
            .bind(req.subject.subject_type())
            .bind(&subject_ref)
            .execute(tx.conn())
            .await
            .db()?;
        let (wf, gate, art, hash) = match &req.subject {
            ApprovalSubject::WorkflowGate {
                instance,
                gate,
                artifact,
                artifact_hash,
            } => (
                Some(instance.to_string()),
                Some(gate.clone()),
                artifact.map(|a| a.to_string()),
                artifact_hash.clone(),
            ),
            ApprovalSubject::ToolInvocation { .. } => (None, None, None, None),
            // The approver must confirm the exact content hash / digest being approved.
            ApprovalSubject::KnowledgeDocument { content_hash, .. } => {
                (None, None, None, Some(content_hash.clone()))
            }
            ApprovalSubject::Skill { digest, .. } => (None, None, None, Some(digest.clone())),
        };
        let role = serde_json::to_value(req.required_role)?
            .as_str()
            .unwrap_or("approver")
            .to_string();
        sqlx::query(
            "INSERT INTO approvals (id, tenant_id, project_id, subject_type, subject_ref, workflow_instance_id, gate, \
             artifact_id, artifact_hash, authors, required_role, requested_by, status, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
        )
        .bind(id.to_string())
        .bind(req.scope.tenant.to_string())
        .bind(req.scope.project.to_string())
        .bind(req.subject.subject_type())
        .bind(&subject_ref)
        .bind(wf)
        .bind(gate)
        .bind(art)
        .bind(hash)
        .bind(to_json(&req.authors)?)
        .bind(&role)
        .bind(req.requested_by.as_str())
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        self.db
            .audit_in(
                tx,
                AuditEntry::system(
                    "approval-manager",
                    "approval.request",
                    "approval",
                    id.to_string(),
                    AuditOutcome::Requested,
                )
                .scope(req.scope)
                .correlation(req.trace.correlation)
                .details(json!({"subject": req.subject, "required_role": role, "authors": req.authors})),
            )
            .await?;
        let ev = EventEnvelope::new(
            &ApprovalRequested {
                approval_id: id.to_string(),
                subject_type: req.subject.subject_type().into(),
                subject_ref,
                required_role: role,
            },
            "approval-manager",
            Some(req.scope),
            req.trace,
            self.db.now(),
        )?;
        outbox::enqueue_in(tx, &ev).await?;
        Ok(id)
    }

    pub async fn request(&self, req: ApprovalRequest) -> Result<ApprovalId> {
        let mut tx = self.db.write().await?;
        let id = self.request_in(&mut tx, req).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Decide an approval. All checks are deterministic and performed before any write.
    pub async fn decide(
        &self,
        approver: &Principal,
        id: ApprovalId,
        decision: ApprovalDecision,
        comments: &str,
        confirmed_artifact_hash: Option<&str>,
        trace: TraceContext,
    ) -> Result<ApprovalRecord> {
        let mut tx = self.db.write().await?;
        let rec = self.load_in(&mut tx, id).await?;
        let check = self.check_decision(approver, &rec, confirmed_artifact_hash);
        if let Err(e) = check {
            self.db
                .audit_in(
                    &mut tx,
                    AuditEntry::new(
                        approver,
                        "approval.decide",
                        "approval",
                        id.to_string(),
                        AuditOutcome::Denied,
                    )
                    .scope(rec.scope)
                    .correlation(trace.correlation)
                    .details(json!({"reason": e.to_string(), "decision": decision})),
                )
                .await?;
            tx.commit().await?;
            return Err(e);
        }
        if comments.len() > 10_000 {
            return Err(HarnessError::invalid_input("comments too long"));
        }
        if decision == ApprovalDecision::RequestChanges {
            if rec.subject_type != "workflow_gate" {
                return Err(HarnessError::invalid_input(
                    "request_changes is only available for workflow approval gates",
                ));
            }
            if comments.trim().is_empty() {
                return Err(HarnessError::invalid_input(
                    "request_changes requires comments describing the changes",
                ));
            }
        }
        let now = ts(self.db.now());
        let decision_str = decision.as_str();
        let role_str = serde_json::to_value(rec.required_role)?
            .as_str()
            .unwrap_or("approver")
            .to_string();
        sqlx::query(
            "INSERT INTO approval_decisions (id, approval_id, approver_id, approver_role, decision, comments, artifact_hash, decided_at, correlation_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(ApprovalId::new().to_string())
        .bind(id.to_string())
        .bind(approver.id.as_str())
        .bind(&role_str)
        .bind(decision_str)
        .bind(comments)
        .bind(&rec.artifact_hash)
        .bind(&now)
        .bind(trace.correlation.to_string())
        .execute(tx.conn())
        .await
        .db()?;
        let new_status = match decision {
            ApprovalDecision::Approve => "approved",
            ApprovalDecision::Reject => "rejected",
            ApprovalDecision::RequestChanges => "changes_requested",
        };
        let upd = sqlx::query(
            "UPDATE approvals SET status = ?, decided_at = ? WHERE id = ? AND status = 'pending'",
        )
        .bind(new_status)
        .bind(&now)
        .bind(id.to_string())
        .execute(tx.conn())
        .await
        .db()?;
        if upd.rows_affected() != 1 {
            return Err(HarnessError::conflict("approval is no longer pending"));
        }
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    approver,
                    "approval.decide",
                    "approval",
                    id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .scope(rec.scope)
                .correlation(trace.correlation)
                .details(json!({
                    "decision": decision_str,
                    "role": role_str,
                    "comments": comments,
                    "artifact_id": rec.artifact_id,
                    "artifact_hash": rec.artifact_hash,
                    "workflow_instance_id": rec.workflow_instance_id,
                    "gate": rec.gate,
                })),
            )
            .await?;
        let ev = match decision {
            ApprovalDecision::Approve => EventEnvelope::new(
                &ApprovalGranted {
                    approval_id: id.to_string(),
                    approver_id: approver.id.to_string(),
                },
                "approval-manager",
                Some(rec.scope),
                trace,
                self.db.now(),
            )?,
            ApprovalDecision::Reject => EventEnvelope::new(
                &ApprovalRejected {
                    approval_id: id.to_string(),
                    approver_id: approver.id.to_string(),
                },
                "approval-manager",
                Some(rec.scope),
                trace,
                self.db.now(),
            )?,
            ApprovalDecision::RequestChanges => EventEnvelope::new(
                &ApprovalChangesRequested {
                    approval_id: id.to_string(),
                    approver_id: approver.id.to_string(),
                },
                "approval-manager",
                Some(rec.scope),
                trace,
                self.db.now(),
            )?,
        };
        outbox::enqueue_in(&mut tx, &ev).await?;
        let out = self.load_in(&mut tx, id).await?;
        tx.commit().await?;
        Ok(out)
    }

    fn check_decision(
        &self,
        approver: &Principal,
        rec: &ApprovalRecord,
        confirmed: Option<&str>,
    ) -> Result<()> {
        self.authz
            .authorize(approver, Action::ApprovalDecide, Target::Scope(rec.scope))?;
        if !approver.is_human() {
            return Err(HarnessError::forbidden(
                "only human principals may decide approvals",
            ));
        }
        if !approver.has_role(rec.required_role) {
            return Err(HarnessError::forbidden(format!(
                "approval requires role {:?}",
                rec.required_role
            )));
        }
        if rec.authors.contains(&approver.id) {
            return Err(HarnessError::forbidden(
                "separation of duties: authors may not approve their own work",
            ));
        }
        if rec.status != ApprovalStatus::Pending {
            return Err(HarnessError::conflict(format!(
                "approval is {:?}, not pending",
                rec.status
            )));
        }
        if let Some(expected) = &rec.artifact_hash {
            match confirmed {
                None => {
                    return Err(HarnessError::invalid_input(
                        "artifact_hash confirmation required for this approval",
                    ));
                }
                Some(c) if c != expected => {
                    return Err(HarnessError::conflict(
                        "artifact hash mismatch: the artifact changed since the approval was requested",
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub async fn get(&self, id: ApprovalId) -> Result<ApprovalRecord> {
        let row = sqlx::query("SELECT * FROM approvals WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.db.pool())
            .await
            .db()?
            .ok_or_else(|| HarnessError::not_found("approval not found"))?;
        let mut rec = row_to_record(&row)?;
        rec.decision = self.decision_of(id).await?;
        Ok(rec)
    }

    async fn load_in(&self, tx: &mut WriteTx, id: ApprovalId) -> Result<ApprovalRecord> {
        let row = sqlx::query("SELECT * FROM approvals WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(tx.conn())
            .await
            .db()?
            .ok_or_else(|| HarnessError::not_found("approval not found"))?;
        row_to_record(&row)
    }

    async fn decision_of(&self, id: ApprovalId) -> Result<Option<DecisionRecord>> {
        let row: Option<(String, String, String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT approver_id, approver_role, decision, comments, artifact_hash, decided_at FROM approval_decisions WHERE approval_id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(self.db.pool())
        .await
        .db()?;
        Ok(row.map(|(a, r, d, c, h, t)| DecisionRecord {
            approver_id: a,
            approver_role: r,
            decision: d,
            comments: c,
            artifact_hash: h,
            decided_at: t,
        }))
    }

    /// List approvals visible within a scope, newest first, keyset-paginated by id.
    pub async fn list(
        &self,
        scope: Scope,
        status: Option<ApprovalStatus>,
        before: Option<ApprovalId>,
        limit: u32,
    ) -> Result<Vec<ApprovalRecord>> {
        let status = status.map(|s| {
            serde_json::to_value(s)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
        });
        let rows = sqlx::query(
            "SELECT * FROM approvals WHERE tenant_id = ? AND project_id = ? AND (? IS NULL OR status = ?) \
             AND (? IS NULL OR id < ?) ORDER BY id DESC LIMIT ?",
        )
        .bind(scope.tenant.to_string())
        .bind(scope.project.to_string())
        .bind(status.clone().flatten())
        .bind(status.flatten())
        .bind(before.map(|b| b.to_string()))
        .bind(before.map(|b| b.to_string()))
        .bind(i64::from(limit.min(500)))
        .fetch_all(self.db.pool())
        .await
        .db()?;
        rows.iter().map(row_to_record).collect()
    }

    /// Full history for a subject (all requests, oldest first).
    pub async fn history(&self, subject: &ApprovalSubject) -> Result<Vec<ApprovalRecord>> {
        let rows =
            sqlx::query("SELECT * FROM approvals WHERE subject_type = ? AND subject_ref = ? ORDER BY id")
                .bind(subject.subject_type())
                .bind(subject.subject_ref())
                .fetch_all(self.db.pool())
                .await
                .db()?;
        let mut out = vec![];
        for r in &rows {
            let mut rec = row_to_record(r)?;
            rec.decision = self.decision_of(rec.id).await?;
            out.push(rec);
        }
        Ok(out)
    }

    /// The approved approval for a tool-invocation fingerprint, if any (used by the gateway).
    pub async fn approved_for(
        &self,
        id: ApprovalId,
        subject: &ApprovalSubject,
        scope: Scope,
    ) -> Result<bool> {
        let rec = self.get(id).await?;
        Ok(rec.status == ApprovalStatus::Approved
            && rec.subject_type == subject.subject_type()
            && rec.subject_ref == subject.subject_ref()
            && rec.scope == scope)
    }
}

fn row_to_record(row: &sqlx::sqlite::SqliteRow) -> Result<ApprovalRecord> {
    let role: String = row.try_get("required_role").db()?;
    Ok(ApprovalRecord {
        id: row.try_get::<String, _>("id").db()?.parse()?,
        scope: Scope::new(
            row.try_get::<String, _>("tenant_id").db()?.parse()?,
            row.try_get::<String, _>("project_id").db()?.parse()?,
        ),
        subject_type: row.try_get("subject_type").db()?,
        subject_ref: row.try_get("subject_ref").db()?,
        workflow_instance_id: row.try_get("workflow_instance_id").db()?,
        gate: row.try_get("gate").db()?,
        artifact_id: row.try_get("artifact_id").db()?,
        artifact_hash: row.try_get("artifact_hash").db()?,
        authors: from_json(&row.try_get::<String, _>("authors").db()?)?,
        required_role: serde_json::from_value(serde_json::Value::String(role))
            .map_err(|_| HarnessError::integrity("unknown role"))?,
        requested_by: row.try_get("requested_by").db()?,
        status: ApprovalStatus::parse(&row.try_get::<String, _>("status").db()?)?,
        created_at: row.try_get("created_at").db()?,
        decided_at: row.try_get("decided_at").db()?,
        decision: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authz::RbacAuthorizer;
    use eaeh_core::{
        AgentDefinitionId, AgentInstanceId, AutonomyLevel, ErrorKind, ProjectAccess, ProjectId, TenantId,
    };

    struct Fx {
        _d: tempfile::TempDir,
        m: ApprovalManager,
        scope: Scope,
    }

    async fn fx() -> Fx {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("a.db")).await.unwrap();
        Fx {
            _d: d,
            m: ApprovalManager::new(db, Arc::new(RbacAuthorizer)),
            scope: Scope::new(TenantId::new(), ProjectId::new()),
        }
    }

    fn human(id: &str, roles: &[Role], scope: Scope) -> Principal {
        Principal::human(
            PrincipalId::new(id).unwrap(),
            scope.tenant,
            roles.iter().copied().collect(),
            ProjectAccess::All,
        )
    }

    fn gate_req(scope: Scope, authors: Vec<PrincipalId>) -> ApprovalRequest {
        ApprovalRequest {
            scope,
            subject: ApprovalSubject::WorkflowGate {
                instance: WorkflowInstanceId::new(),
                gate: "design_review".into(),
                artifact: Some(ArtifactId::new()),
                artifact_hash: Some("abc".into()),
            },
            required_role: Role::Approver,
            authors,
            requested_by: PrincipalId::new("system:workflow-engine").unwrap(),
            trace: TraceContext::new_root(),
        }
    }

    #[tokio::test]
    async fn approver_can_approve_with_matching_hash() {
        let f = fx().await;
        let id = f.m.request(gate_req(f.scope, vec![])).await.unwrap();
        let bob = human("bob", &[Role::Approver], f.scope);
        let r =
            f.m.decide(
                &bob,
                id,
                ApprovalDecision::Approve,
                "lgtm",
                Some("abc"),
                TraceContext::new_root(),
            )
            .await
            .unwrap();
        assert_eq!(r.status, ApprovalStatus::Approved);
        let full = f.m.get(id).await.unwrap();
        let d = full.decision.unwrap();
        assert_eq!(d.approver_id, "bob");
        assert_eq!(d.comments, "lgtm");
        // Cannot be decided twice.
        let again =
            f.m.decide(
                &bob,
                id,
                ApprovalDecision::Reject,
                "",
                Some("abc"),
                TraceContext::new_root(),
            )
            .await;
        assert_eq!(again.unwrap_err().kind, ErrorKind::Conflict);
    }

    #[tokio::test]
    async fn self_approval_and_non_humans_rejected() {
        let f = fx().await;
        let agent = Principal::agent(
            AgentInstanceId::new(),
            AgentDefinitionId::new("dev").unwrap(),
            "1.0.0".into(),
            AutonomyLevel::ExternalSideEffects,
            f.scope,
        );
        let author = human("alice", &[Role::Approver], f.scope);
        let id =
            f.m.request(gate_req(f.scope, vec![agent.id.clone(), author.id.clone()]))
                .await
                .unwrap();
        let t = TraceContext::new_root();
        assert_eq!(
            f.m.decide(&agent, id, ApprovalDecision::Approve, "", Some("abc"), t)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Forbidden
        );
        assert_eq!(
            f.m.decide(&author, id, ApprovalDecision::Approve, "", Some("abc"), t)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Forbidden
        );
        let admin = human("root", &[Role::Admin], f.scope);
        assert_eq!(
            f.m.decide(&admin, id, ApprovalDecision::Approve, "", Some("abc"), t)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Forbidden
        );
        let outsider = Principal::human(
            PrincipalId::new("eve").unwrap(),
            TenantId::new(),
            [Role::Approver].into(),
            ProjectAccess::All,
        );
        assert_eq!(
            f.m.decide(&outsider, id, ApprovalDecision::Approve, "", Some("abc"), t)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Forbidden
        );
        assert_eq!(f.m.get(id).await.unwrap().status, ApprovalStatus::Pending);
    }

    #[tokio::test]
    async fn artifact_hash_must_match() {
        let f = fx().await;
        let id = f.m.request(gate_req(f.scope, vec![])).await.unwrap();
        let bob = human("bob", &[Role::Approver], f.scope);
        let t = TraceContext::new_root();
        assert_eq!(
            f.m.decide(&bob, id, ApprovalDecision::Approve, "", None, t)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::InvalidInput
        );
        assert_eq!(
            f.m.decide(&bob, id, ApprovalDecision::Approve, "", Some("zzz"), t)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
    }

    #[tokio::test]
    async fn rerequest_supersedes_and_keeps_history() {
        let f = fx().await;
        let req = gate_req(f.scope, vec![]);
        let first = f.m.request(req.clone()).await.unwrap();
        let bob = human("bob", &[Role::Approver], f.scope);
        f.m.decide(
            &bob,
            first,
            ApprovalDecision::Reject,
            "needs work",
            Some("abc"),
            TraceContext::new_root(),
        )
        .await
        .unwrap();
        let second = f.m.request(req.clone()).await.unwrap();
        let third = f.m.request(req.clone()).await.unwrap();
        let h = f.m.history(&req.subject).await.unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].status, ApprovalStatus::Rejected);
        assert_eq!(h[0].decision.as_ref().unwrap().comments, "needs work");
        assert_eq!(h[1].id, second);
        assert_eq!(h[1].status, ApprovalStatus::Superseded);
        assert_eq!(h[2].id, third);
        assert_eq!(h[2].status, ApprovalStatus::Pending);
        let pending =
            f.m.list(f.scope, Some(ApprovalStatus::Pending), None, 10)
                .await
                .unwrap();
        assert_eq!(pending.len(), 1);
    }

    #[tokio::test]
    async fn request_changes_is_limited_to_workflow_gates_and_needs_comments() {
        let f = fx().await;
        let bob = human("bob", &[Role::Approver], f.scope);
        let t = TraceContext::new_root();
        // Knowledge standards are approved or rejected – never "revised" through the gate.
        let k =
            f.m.request(ApprovalRequest {
                scope: f.scope,
                subject: ApprovalSubject::KnowledgeDocument {
                    document: "doc-1".into(),
                    content_hash: "abc".into(),
                },
                required_role: Role::Approver,
                authors: vec![],
                requested_by: PrincipalId::new("steward").unwrap(),
                trace: t,
            })
            .await
            .unwrap();
        let e =
            f.m.decide(
                &bob,
                k,
                ApprovalDecision::RequestChanges,
                "please fix",
                Some("abc"),
                t,
            )
            .await
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::InvalidInput);
        assert_eq!(f.m.get(k).await.unwrap().status, ApprovalStatus::Pending);

        let g = f.m.request(gate_req(f.scope, vec![])).await.unwrap();
        let e =
            f.m.decide(&bob, g, ApprovalDecision::RequestChanges, "  ", Some("abc"), t)
                .await
                .unwrap_err();
        assert_eq!(e.kind, ErrorKind::InvalidInput);
        let r =
            f.m.decide(
                &bob,
                g,
                ApprovalDecision::RequestChanges,
                "add sequence diagrams",
                Some("abc"),
                t,
            )
            .await
            .unwrap();
        assert_eq!(r.status, ApprovalStatus::ChangesRequested);
        assert_eq!(
            f.m.get(g).await.unwrap().decision.unwrap().decision,
            "request_changes"
        );
        assert_eq!(
            ApprovalDecision::parse("request_changes").unwrap(),
            ApprovalDecision::RequestChanges
        );
        assert!(ApprovalDecision::parse("maybe").is_err());
    }
}
