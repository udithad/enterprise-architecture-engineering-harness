//! EAEH skills management (Phase 2, master §18).
//!
//! A skill is a reusable engineering capability — instructions, typed contracts, the
//! knowledge it needs and the tools it expects — independent of any one agent type.
//!
//! * Versions are immutable (identical re-registration is idempotent).
//! * Registration validates contracts, tools (exist, risk ≤ declared ceiling),
//!   dependencies (resolvable, acyclic) and platform compatibility, then requests a
//!   human approval bound to the definition digest. Only `approved` (or `deprecated`)
//!   versions are usable; the registrant can never approve (separation of duties).
//! * **A skill never grants permissions.** It binds to an agent only if every required
//!   tool is already in the agent's allow-list and its risk ceiling is within the agent's
//!   autonomy. Explicitly referenced incompatible skills fail the run; dynamically
//!   selected incompatible skills are skipped and reported.
//! * Selection is deterministic (explicit refs, then task-kind matches; highest usable
//!   version; dependencies first). Skill instructions enter context at `skill` trust —
//!   this crate is the only producer (fitness-enforced).

use eaeh_core::hash::canonical_json_hash;
use eaeh_core::{
    ApprovalId, AutonomyLevel, HarnessError, Principal, ProjectId, Result, RiskClass, Role, Scope, SkillId,
    ToolId, TraceContext,
};
use eaeh_events::catalog::{SkillRegistered, SkillStateChanged};
use eaeh_events::{DomainEvent, EventEnvelope, outbox};
use eaeh_governance::authz::{Action, Authorizer, Target};
use eaeh_governance::{ApprovalManager, ApprovalRequest, ApprovalStatus, ApprovalSubject};
use eaeh_intelligence::ContextItem;
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, WriteTx, from_json, sqlx, to_json, ts};
use eaeh_tools::ToolRegistry;
use eaeh_validation::schema::CompiledSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Skill API version; definitions declare a compatible requirement.
pub const SKILL_API_VERSION: semver::Version = semver::Version::new(1, 0, 0);
const MAX_INSTRUCTIONS: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRequirement {
    pub query: String,
    #[serde(default = "default_k")]
    pub k: usize,
}

fn default_k() -> usize {
    3
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDependency {
    pub id: SkillId,
    pub version: semver::VersionReq,
}

fn object_schema() -> Value {
    json!({"type": "object"})
}

fn api_req() -> semver::VersionReq {
    semver::VersionReq::parse("^1").unwrap_or(semver::VersionReq::STAR)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub id: SkillId,
    pub version: semver::Version,
    pub description: String,
    pub capabilities: BTreeSet<String>,
    /// Task kinds this skill is selected for dynamically.
    #[serde(default)]
    pub task_kinds: Vec<String>,
    /// Instruction text (plain text; no template placeholders).
    pub instructions: String,
    #[serde(default)]
    pub required_knowledge: Vec<KnowledgeRequirement>,
    #[serde(default)]
    pub required_tools: Vec<ToolId>,
    #[serde(default = "object_schema")]
    pub input_schema: Value,
    /// Additional output contract the agent's output must satisfy when the skill applies.
    #[serde(default = "object_schema")]
    pub output_schema: Value,
    /// Highest tool risk the skill expects; must be within the agent's autonomy.
    pub max_risk: RiskClass,
    #[serde(default)]
    pub dependencies: Vec<SkillDependency>,
    #[serde(default = "api_req")]
    pub eaeh_api: semver::VersionReq,
    #[serde(default)]
    pub evaluation_criteria: Vec<String>,
}

impl SkillDefinition {
    pub fn reference(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    pub fn digest(&self) -> Result<String> {
        Ok(canonical_json_hash(&serde_json::to_value(self)?))
    }

    /// Structural validation (no registry lookups).
    pub fn validate(&self) -> Result<()> {
        let err = |m: String| {
            Err(HarnessError::invalid_input(format!(
                "skill {}: {m}",
                self.reference()
            )))
        };
        if self.description.trim().is_empty() || self.description.len() > 2000 {
            return err("description must be 1..=2000 bytes".into());
        }
        if self.instructions.trim().is_empty() || self.instructions.len() > MAX_INSTRUCTIONS {
            return err(format!("instructions must be 1..={MAX_INSTRUCTIONS} bytes"));
        }
        if self.instructions.contains("{{") {
            return err("instructions may not contain template placeholders".into());
        }
        if self.capabilities.is_empty() {
            return err("at least one capability required".into());
        }
        for k in &self.task_kinds {
            eaeh_core::ids::validate_slug(k)
                .map_err(|m| HarnessError::invalid_input(format!("task kind '{k}': {m}")))?;
        }
        for r in &self.required_knowledge {
            if r.query.trim().is_empty() || r.query.len() > 500 || !(1..=10).contains(&r.k) {
                return err("required_knowledge entries need a 1..=500 byte query and k in 1..=10".into());
            }
        }
        if !self.eaeh_api.matches(&SKILL_API_VERSION) {
            return err(format!(
                "requires skill API {} (platform {SKILL_API_VERSION})",
                self.eaeh_api
            ));
        }
        if self.dependencies.iter().any(|d| d.id == self.id) {
            return err("a skill cannot depend on itself".into());
        }
        CompiledSchema::compile(&self.input_schema)?;
        CompiledSchema::compile(&self.output_schema)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillStatus {
    PendingApproval,
    Approved,
    Rejected,
    Deprecated,
    Retired,
}

impl SkillStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SkillStatus::PendingApproval => "pending_approval",
            SkillStatus::Approved => "approved",
            SkillStatus::Rejected => "rejected",
            SkillStatus::Deprecated => "deprecated",
            SkillStatus::Retired => "retired",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "pending_approval" => Self::PendingApproval,
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            "deprecated" => Self::Deprecated,
            "retired" => Self::Retired,
            _ => return Err(HarnessError::integrity("unknown skill status")),
        })
    }

    pub fn usable(self) -> bool {
        matches!(self, SkillStatus::Approved | SkillStatus::Deprecated)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillRecord {
    pub definition: SkillDefinition,
    pub digest: String,
    pub status: SkillStatus,
    pub approval_id: Option<String>,
    pub registered_by: String,
}

/// A parsed skill reference from an agent definition: `id` or `id@<semver req>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRef {
    pub id: SkillId,
    pub version: semver::VersionReq,
}

impl SkillRef {
    pub fn parse(s: &str) -> Result<Self> {
        let (id, req) = match s.split_once('@') {
            Some((i, r)) => (
                i,
                semver::VersionReq::parse(r)
                    .map_err(|e| HarnessError::invalid_input(format!("skill ref '{s}': {e}")))?,
            ),
            None => (s, semver::VersionReq::STAR),
        };
        Ok(Self {
            id: SkillId::new(id)?,
            version: req,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedSkill {
    pub reference: String,
    pub definition: SkillDefinition,
    /// `explicit`, `task_kind` or `dependency`.
    pub selected_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    pub skills: Vec<ResolvedSkill>,
    /// Dynamically matched skills that were not applied, with the reason.
    pub skipped: Vec<(String, String)>,
}

impl Resolution {
    pub fn references(&self) -> Vec<String> {
        self.skills.iter().map(|s| s.reference.clone()).collect()
    }

    /// Skill instructions at `skill` trust (the only producer of such items).
    pub fn instruction_items(&self) -> Vec<ContextItem> {
        self.skills
            .iter()
            .map(|s| {
                ContextItem::skill_instruction(
                    &format!("skill:{}", s.reference),
                    format!(
                        "APPROVED SKILL [{}]: {}\n{}",
                        s.reference, s.definition.description, s.definition.instructions
                    ),
                    &s.reference,
                )
            })
            .collect()
    }
}

/// What a skill binds to: the calling agent's permissions.
#[derive(Debug, Clone)]
pub struct Binding<'a> {
    pub explicit: &'a [String],
    pub allowed_tools: &'a [ToolId],
    pub autonomy: AutonomyLevel,
    pub task_kind: &'a str,
}

#[derive(Clone)]
pub struct SkillRegistry {
    db: Db,
    authz: Arc<dyn Authorizer>,
    approvals: ApprovalManager,
    tools: Arc<ToolRegistry>,
}

impl std::fmt::Debug for SkillRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillRegistry").finish_non_exhaustive()
    }
}

async fn emit<E: DomainEvent>(db: &Db, tx: &mut WriteTx, ev: &E, trace: TraceContext) -> Result<()> {
    let env = EventEnvelope::new(ev, "skills", None, trace, db.now())?;
    outbox::enqueue_in(tx, &env).await.map(|_| ())
}

impl SkillRegistry {
    pub fn new(
        db: Db,
        authz: Arc<dyn Authorizer>,
        approvals: ApprovalManager,
        tools: Arc<ToolRegistry>,
    ) -> Self {
        Self {
            db,
            authz,
            approvals,
            tools,
        }
    }

    /// Register a skill version and request its approval (scoped to `governance_project`).
    pub async fn register(
        &self,
        actor: &Principal,
        def: &SkillDefinition,
        governance_project: ProjectId,
        trace: TraceContext,
    ) -> Result<SkillRecord> {
        let gov = Scope::new(actor.tenant, governance_project);
        self.authz
            .authorize(actor, Action::SkillManage, Target::Scope(gov))?;
        def.validate()?;
        for t in &def.required_tools {
            let d = self
                .tools
                .descriptor(t)
                .map_err(|e| e.context(format!("skill {}", def.reference())))?;
            if d.risk > def.max_risk {
                return Err(HarnessError::invalid_input(format!(
                    "skill {} requires tool {t} ({}) above its declared max_risk {}",
                    def.reference(),
                    d.risk,
                    def.max_risk
                )));
            }
        }
        let all = self.all().await?;
        for dep in &def.dependencies {
            if !all.iter().any(|r| {
                r.definition.id == dep.id
                    && dep.version.matches(&r.definition.version)
                    && r.status != SkillStatus::Retired
            }) {
                return Err(HarnessError::invalid_input(format!(
                    "unresolved skill dependency {} {}",
                    dep.id, dep.version
                )));
            }
        }
        check_acyclic(def, &all)?;
        let digest = def.digest()?;
        if let Some(existing) = all
            .iter()
            .find(|r| r.definition.id == def.id && r.definition.version == def.version)
        {
            if existing.digest == digest {
                return Ok(existing.clone());
            }
            return Err(HarnessError::conflict(format!(
                "skill {} already registered with different content",
                def.reference()
            )));
        }
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO skills (id, version, document, digest, status, registered_by, created_at, updated_at) VALUES (?, ?, ?, ?, 'pending_approval', ?, ?, ?)",
        )
        .bind(def.id.as_str())
        .bind(def.version.to_string())
        .bind(to_json(def)?)
        .bind(&digest)
        .bind(actor.id.as_str())
        .bind(&now)
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        let aid = self
            .approvals
            .request_in(
                &mut tx,
                ApprovalRequest {
                    scope: gov,
                    subject: ApprovalSubject::Skill {
                        skill: def.id.to_string(),
                        version: def.version.to_string(),
                        digest: digest.clone(),
                    },
                    required_role: Role::Approver,
                    authors: vec![actor.id.clone()],
                    requested_by: actor.id.clone(),
                    trace,
                },
            )
            .await?;
        sqlx::query("UPDATE skills SET approval_id = ? WHERE id = ? AND version = ?")
            .bind(aid.to_string())
            .bind(def.id.as_str())
            .bind(def.version.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(actor, "skill.register", "skill", def.reference(), AuditOutcome::Succeeded)
                    .scope(gov)
                    .correlation(trace.correlation)
                    .details(json!({"digest": digest, "tools": def.required_tools, "max_risk": def.max_risk, "dependencies": def.dependencies})),
            )
            .await?;
        emit(
            &self.db,
            &mut tx,
            &SkillRegistered {
                skill: def.id.to_string(),
                version: def.version.to_string(),
                status: "pending_approval".into(),
            },
            trace,
        )
        .await?;
        tx.commit().await?;
        self.get(&def.id, &def.version).await
    }

    /// Apply a decided approval (approved ⇒ `approved` only when the digest matches).
    pub async fn apply_approval(
        &self,
        approval_id: ApprovalId,
        trace: TraceContext,
    ) -> Result<Option<SkillRecord>> {
        let rec = self.approvals.get(approval_id).await?;
        if rec.subject_type != "skill" {
            return Ok(None);
        }
        let (id, version) = rec
            .subject_ref
            .split_once('@')
            .ok_or_else(|| HarnessError::integrity("malformed skill approval subject"))?;
        let id = SkillId::new(id)?;
        let version =
            semver::Version::parse(version).map_err(|_| HarnessError::integrity("skill version"))?;
        let current = self.get(&id, &version).await?;
        if current.status != SkillStatus::PendingApproval
            || current.approval_id.as_deref() != Some(approval_id.to_string().as_str())
        {
            return Ok(Some(current));
        }
        let new_status = match rec.status {
            ApprovalStatus::Approved if rec.artifact_hash.as_deref() == Some(current.digest.as_str()) => {
                SkillStatus::Approved
            }
            ApprovalStatus::Rejected => SkillStatus::Rejected,
            _ => return Ok(Some(current)),
        };
        self.transition(None, &current, new_status, trace).await?;
        Ok(Some(self.get(&id, &version).await?))
    }

    pub async fn set_status(
        &self,
        actor: &Principal,
        id: &SkillId,
        version: &semver::Version,
        to: SkillStatus,
    ) -> Result<SkillRecord> {
        self.authz
            .authorize(actor, Action::SkillManage, Target::Tenant(actor.tenant))?;
        let current = self.get(id, version).await?;
        let ok = matches!(
            (current.status, to),
            (SkillStatus::Approved, SkillStatus::Deprecated)
                | (SkillStatus::Approved, SkillStatus::Retired)
                | (SkillStatus::Deprecated, SkillStatus::Retired)
                | (SkillStatus::Deprecated, SkillStatus::Approved)
        );
        if !ok {
            return Err(HarnessError::conflict(format!(
                "skill cannot transition {:?} -> {:?}",
                current.status, to
            )));
        }
        self.transition(Some(actor), &current, to, TraceContext::new_root())
            .await?;
        self.get(id, version).await
    }

    async fn transition(
        &self,
        actor: Option<&Principal>,
        current: &SkillRecord,
        to: SkillStatus,
        trace: TraceContext,
    ) -> Result<()> {
        let d = &current.definition;
        let mut tx = self.db.write().await?;
        let r = sqlx::query(
            "UPDATE skills SET status = ?, updated_at = ? WHERE id = ? AND version = ? AND status = ?",
        )
        .bind(to.as_str())
        .bind(ts(self.db.now()))
        .bind(d.id.as_str())
        .bind(d.version.to_string())
        .bind(current.status.as_str())
        .execute(tx.conn())
        .await
        .db()?;
        if r.rows_affected() != 1 {
            return Err(HarnessError::conflict("skill state changed concurrently"));
        }
        let entry = match actor {
            Some(a) => AuditEntry::new(
                a,
                format!("skill.{}", to.as_str()),
                "skill",
                d.reference(),
                AuditOutcome::Succeeded,
            ),
            None => AuditEntry::system(
                "skills",
                format!("skill.{}", to.as_str()),
                "skill",
                d.reference(),
                AuditOutcome::Succeeded,
            ),
        };
        self.db
            .audit_in(&mut tx, entry.correlation(trace.correlation))
            .await?;
        emit(
            &self.db,
            &mut tx,
            &SkillStateChanged {
                skill: d.id.to_string(),
                version: d.version.to_string(),
                status: to.as_str().into(),
            },
            trace,
        )
        .await?;
        tx.commit().await
    }

    pub async fn get(&self, id: &SkillId, version: &semver::Version) -> Result<SkillRecord> {
        let row: Option<(String, String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT document, digest, status, approval_id, registered_by FROM skills WHERE id = ? AND version = ?",
        )
        .bind(id.as_str())
        .bind(version.to_string())
        .fetch_optional(self.db.pool())
        .await
        .db()?;
        to_record(row.ok_or_else(|| HarnessError::not_found(format!("skill {id}@{version} not registered")))?)
    }

    pub async fn list(&self, actor: &Principal) -> Result<Vec<SkillRecord>> {
        self.authz
            .authorize(actor, Action::SkillRead, Target::Tenant(actor.tenant))?;
        self.all().await
    }

    async fn all(&self) -> Result<Vec<SkillRecord>> {
        let rows: Vec<(String, String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT document, digest, status, approval_id, registered_by FROM skills ORDER BY id, created_at",
        )
        .fetch_all(self.db.pool())
        .await
        .db()?;
        rows.into_iter().map(to_record).collect()
    }

    /// Resolve the skills that apply to an agent run (see crate docs for the rules).
    pub async fn resolve(&self, b: &Binding<'_>) -> Result<Resolution> {
        let all = self.all().await?;
        let best = |id: &SkillId, req: &semver::VersionReq| -> Option<SkillRecord> {
            all.iter()
                .filter(|r| &r.definition.id == id && req.matches(&r.definition.version) && r.status.usable())
                .max_by(|a, b| {
                    (a.status == SkillStatus::Approved, &a.definition.version)
                        .cmp(&(b.status == SkillStatus::Approved, &b.definition.version))
                })
                .cloned()
        };
        let compatible = |d: &SkillDefinition| -> std::result::Result<(), String> {
            if let Some(t) = d.required_tools.iter().find(|t| !b.allowed_tools.contains(t)) {
                return Err(format!("requires tool {t} which the agent is not allowed to use"));
            }
            if d.max_risk > b.autonomy.max_risk() {
                return Err(format!(
                    "max_risk {} exceeds the agent's autonomy ceiling {}",
                    d.max_risk,
                    b.autonomy.max_risk()
                ));
            }
            Ok(())
        };
        let mut chosen: BTreeMap<SkillId, ResolvedSkill> = BTreeMap::new();
        let mut order: Vec<SkillId> = vec![];
        let mut skipped = vec![];
        let mut queue: Vec<(SkillRecord, String)> = vec![];
        for s in b.explicit {
            let r = SkillRef::parse(s)?;
            let rec = best(&r.id, &r.version).ok_or_else(|| {
                HarnessError::not_found(format!("no approved version of skill '{s}' is available"))
            })?;
            compatible(&rec.definition).map_err(|m| {
                HarnessError::forbidden(format!(
                    "skill {} cannot be bound: {m}",
                    rec.definition.reference()
                ))
            })?;
            queue.push((rec, "explicit".into()));
        }
        let mut dynamic: Vec<&SkillRecord> = all
            .iter()
            .filter(|r| {
                r.status == SkillStatus::Approved && r.definition.task_kinds.iter().any(|k| k == b.task_kind)
            })
            .collect();
        dynamic.sort_by(|a, b| {
            a.definition
                .id
                .cmp(&b.definition.id)
                .then(b.definition.version.cmp(&a.definition.version))
        });
        let mut seen_dyn = BTreeSet::new();
        for r in dynamic {
            if !seen_dyn.insert(r.definition.id.clone())
                || queue.iter().any(|(q, _)| q.definition.id == r.definition.id)
            {
                continue;
            }
            match compatible(&r.definition) {
                Ok(()) => queue.push((r.clone(), "task_kind".into())),
                Err(m) => skipped.push((r.definition.reference(), m)),
            }
        }
        // Depth-first: dependencies are placed before dependants.
        fn place(
            rec: SkillRecord,
            by: String,
            best: &dyn Fn(&SkillId, &semver::VersionReq) -> Option<SkillRecord>,
            compatible: &dyn Fn(&SkillDefinition) -> std::result::Result<(), String>,
            chosen: &mut BTreeMap<SkillId, ResolvedSkill>,
            order: &mut Vec<SkillId>,
            depth: usize,
        ) -> Result<()> {
            if chosen.contains_key(&rec.definition.id) {
                return Ok(());
            }
            if depth > 16 {
                return Err(HarnessError::invalid_input("skill dependency chain too deep"));
            }
            for dep in &rec.definition.dependencies {
                let d = best(&dep.id, &dep.version).ok_or_else(|| {
                    HarnessError::not_found(format!(
                        "dependency {} {} of {} has no approved version",
                        dep.id,
                        dep.version,
                        rec.definition.reference()
                    ))
                })?;
                compatible(&d.definition).map_err(|m| {
                    HarnessError::forbidden(format!(
                        "dependency {} cannot be bound: {m}",
                        d.definition.reference()
                    ))
                })?;
                place(d, "dependency".into(), best, compatible, chosen, order, depth + 1)?;
            }
            order.push(rec.definition.id.clone());
            chosen.insert(
                rec.definition.id.clone(),
                ResolvedSkill {
                    reference: rec.definition.reference(),
                    definition: rec.definition,
                    selected_by: by,
                },
            );
            Ok(())
        }
        for (rec, by) in queue {
            place(rec, by, &best, &compatible, &mut chosen, &mut order, 0)?;
        }
        Ok(Resolution {
            skills: order.into_iter().filter_map(|id| chosen.remove(&id)).collect(),
            skipped,
        })
    }
}

fn check_acyclic(new: &SkillDefinition, all: &[SkillRecord]) -> Result<()> {
    // Graph over ids using the highest registered version matching each requirement.
    let deps_of = |id: &SkillId| -> Vec<SkillId> {
        if id == &new.id {
            return new.dependencies.iter().map(|d| d.id.clone()).collect();
        }
        all.iter()
            .filter(|r| &r.definition.id == id)
            .max_by(|a, b| a.definition.version.cmp(&b.definition.version))
            .map(|r| r.definition.dependencies.iter().map(|d| d.id.clone()).collect())
            .unwrap_or_default()
    };
    fn visit(
        n: &SkillId,
        deps_of: &dyn Fn(&SkillId) -> Vec<SkillId>,
        stack: &mut Vec<SkillId>,
    ) -> Result<()> {
        if stack.contains(n) {
            return Err(HarnessError::invalid_input(format!(
                "skill dependency cycle: {} -> {n}",
                stack
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            )));
        }
        stack.push(n.clone());
        for d in deps_of(n) {
            visit(&d, deps_of, stack)?;
        }
        stack.pop();
        Ok(())
    }
    visit(&new.id, &deps_of, &mut vec![])
}

fn to_record(row: (String, String, String, Option<String>, String)) -> Result<SkillRecord> {
    Ok(SkillRecord {
        definition: from_json(&row.0)?,
        digest: row.1,
        status: SkillStatus::parse(&row.2)?,
        approval_id: row.3,
        registered_by: row.4,
    })
}
