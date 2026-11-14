use crate::definition::AgentDefinition;
use eaeh_core::{AgentDefinitionId, AutonomyLevel, HarnessError, Principal, Result};
use eaeh_governance::authz::{Action, Authorizer, Target};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, from_json, sqlx, to_json, ts};
use eaeh_tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CapabilityQuery {
    pub required: BTreeSet<String>,
    pub max_autonomy: Option<AutonomyLevel>,
    /// Restrict to these definition ids (e.g. a delegation allowlist).
    pub ids: Option<Vec<AgentDefinitionId>>,
}

#[derive(Clone)]
pub struct AgentRegistry {
    db: Db,
    authz: Arc<dyn Authorizer>,
    tools: Arc<ToolRegistry>,
}

impl std::fmt::Debug for AgentRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRegistry").finish_non_exhaustive()
    }
}

impl AgentRegistry {
    pub fn new(db: Db, authz: Arc<dyn Authorizer>, tools: Arc<ToolRegistry>) -> Self {
        Self { db, authz, tools }
    }

    /// Register a definition version. Re-registering identical content is idempotent;
    /// different content under an existing version is a conflict.
    pub async fn register(&self, actor: &Principal, def: &AgentDefinition) -> Result<()> {
        self.authz
            .authorize(actor, Action::AgentRegister, Target::Platform)?;
        self.register_trusted(Some(actor), def).await
    }

    /// Registration path for platform configuration loaded at startup (no actor).
    pub async fn register_trusted(&self, actor: Option<&Principal>, def: &AgentDefinition) -> Result<()> {
        def.validate()?;
        for t in &def.allowed_tools {
            let d = self
                .tools
                .descriptor(t)
                .map_err(|e| e.context(format!("agent {}", def.reference())))?;
            if !def.autonomy.permits(d.risk) {
                return Err(HarnessError::invalid_input(format!(
                    "agent {} lists tool {t} ({}) above its autonomy level {:?}",
                    def.reference(),
                    d.risk,
                    def.autonomy
                )));
            }
        }
        let digest = def.digest()?;
        let existing: Option<(String,)> =
            sqlx::query_as("SELECT digest FROM agent_definitions WHERE id = ? AND version = ?")
                .bind(def.id.as_str())
                .bind(def.version.to_string())
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        match existing {
            Some((d,)) if d == digest => return Ok(()),
            Some(_) => {
                return Err(HarnessError::conflict(format!(
                    "agent {} already registered with different content",
                    def.reference()
                )));
            }
            None => {}
        }
        let mut tx = self.db.write().await?;
        sqlx::query("INSERT INTO agent_definitions (id, version, document, digest, status, created_at) VALUES (?, ?, ?, ?, 'active', ?)")
            .bind(def.id.as_str())
            .bind(def.version.to_string())
            .bind(to_json(def)?)
            .bind(&digest)
            .bind(ts(self.db.now()))
            .execute(tx.conn())
            .await
            .db()?;
        let entry = match actor {
            Some(a) => AuditEntry::new(
                a,
                "agent.register",
                "agent_definition",
                def.reference(),
                AuditOutcome::Succeeded,
            ),
            None => AuditEntry::system(
                "agent-loader",
                "agent.register",
                "agent_definition",
                def.reference(),
                AuditOutcome::Succeeded,
            ),
        };
        self.db
            .audit_in(
                &mut tx,
                entry.details(serde_json::json!({"digest": digest, "autonomy": def.autonomy})),
            )
            .await?;
        tx.commit().await
    }

    pub async fn set_status(
        &self,
        actor: &Principal,
        id: &AgentDefinitionId,
        version: &semver::Version,
        status: &str,
    ) -> Result<()> {
        self.authz
            .authorize(actor, Action::AgentRegister, Target::Platform)?;
        if !matches!(status, "active" | "deprecated" | "retired") {
            return Err(HarnessError::invalid_input(
                "status must be active, deprecated or retired",
            ));
        }
        let mut tx = self.db.write().await?;
        let r = sqlx::query(
            "UPDATE agent_definitions SET status = ? WHERE id = ? AND version = ? AND status != 'retired'",
        )
        .bind(status)
        .bind(id.as_str())
        .bind(version.to_string())
        .execute(tx.conn())
        .await
        .db()?;
        if r.rows_affected() == 0 {
            return Err(HarnessError::not_found("agent definition not found or retired"));
        }
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    format!("agent.{status}"),
                    "agent_definition",
                    format!("{id}@{version}"),
                    AuditOutcome::Succeeded,
                ),
            )
            .await?;
        tx.commit().await
    }

    pub async fn get(
        &self,
        id: &AgentDefinitionId,
        version: &semver::Version,
    ) -> Result<(AgentDefinition, String)> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT document, status FROM agent_definitions WHERE id = ? AND version = ?")
                .bind(id.as_str())
                .bind(version.to_string())
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        let (doc, status) =
            row.ok_or_else(|| HarnessError::not_found(format!("agent {id}@{version} not registered")))?;
        Ok((from_json(&doc)?, status))
    }

    /// All definitions with their status.
    pub async fn list(&self) -> Result<Vec<(AgentDefinition, String)>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT document, status FROM agent_definitions ORDER BY id, created_at")
                .fetch_all(self.db.pool())
                .await
                .db()?;
        rows.into_iter().map(|(d, s)| Ok((from_json(&d)?, s))).collect()
    }

    /// Highest *active* version of each definition.
    pub async fn active(&self) -> Result<Vec<AgentDefinition>> {
        let mut latest: BTreeMap<AgentDefinitionId, AgentDefinition> = BTreeMap::new();
        for (d, status) in self.list().await? {
            if status != "active" {
                continue;
            }
            match latest.get(&d.id) {
                Some(cur) if cur.version >= d.version => {}
                _ => {
                    latest.insert(d.id.clone(), d);
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub async fn latest(&self, id: &AgentDefinitionId) -> Result<AgentDefinition> {
        self.active()
            .await?
            .into_iter()
            .find(|d| &d.id == id)
            .ok_or_else(|| HarnessError::not_found(format!("no active version of agent {id}")))
    }

    /// Capability discovery. Prefers the smallest effective agent: fewest capabilities
    /// beyond those required, then lowest autonomy, then id.
    pub async fn discover(&self, q: &CapabilityQuery) -> Result<Vec<AgentDefinition>> {
        let mut c: Vec<AgentDefinition> = self
            .active()
            .await?
            .into_iter()
            .filter(|d| q.required.is_subset(&d.capabilities))
            .filter(|d| q.max_autonomy.is_none_or(|m| d.autonomy <= m))
            .filter(|d| q.ids.as_ref().is_none_or(|ids| ids.contains(&d.id)))
            .collect();
        c.sort_by(|a, b| {
            (a.capabilities.len() - q.required.len())
                .cmp(&(b.capabilities.len() - q.required.len()))
                .then(a.autonomy.cmp(&b.autonomy))
                .then(a.id.cmp(&b.id))
        });
        Ok(c)
    }
}
