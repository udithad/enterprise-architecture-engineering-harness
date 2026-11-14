//! Prompts as versioned platform assets (master §15).
//!
//! Templates use `{{name}}` placeholders. Every placeholder must be declared and every
//! declared variable used. Values are [`TrustedText`]; `Untrusted<_>` data cannot be
//! passed to `render` and must instead enter the model request through the context
//! assembler's fenced data section.

use eaeh_core::{HarnessError, Principal, PromptId, Result};
use eaeh_governance::authz::{Action, Authorizer, Target};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, from_json, sqlx, to_json, ts};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    System,
    Agent,
    Task,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableSpec {
    pub name: String,
    #[serde(default = "yes")]
    pub required: bool,
    #[serde(default = "default_max")]
    pub max_len: usize,
}

fn yes() -> bool {
    true
}
fn default_max() -> usize {
    8192
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub id: PromptId,
    pub version: semver::Version,
    pub kind: PromptKind,
    pub description: String,
    pub body: String,
    #[serde(default)]
    pub variables: Vec<VariableSpec>,
}

/// Text originating from a trusted source (platform configuration, authenticated task
/// input). Distinct from `Untrusted<String>` so the two cannot be confused by type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedText(String);

impl TrustedText {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Output of [`PromptTemplate::render`]; fields are private so instruction text can only
/// originate from a registered template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderedPrompt {
    id: PromptId,
    version: semver::Version,
    kind: PromptKind,
    text: String,
}

impl RenderedPrompt {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn reference(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
    pub fn kind(&self) -> PromptKind {
        self.kind
    }
}

fn placeholders(body: &str) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| HarnessError::invalid_input("unterminated placeholder"))?;
        let name = after[..end].trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(HarnessError::invalid_input(format!(
                "invalid placeholder '{name}'"
            )));
        }
        out.insert(name.to_string());
        rest = &after[end + 2..];
    }
    Ok(out)
}

impl PromptTemplate {
    pub fn validate(&self) -> Result<()> {
        if self.body.trim().is_empty() || self.body.len() > 64 * 1024 {
            return Err(HarnessError::invalid_input("prompt body must be 1..=65536 bytes"));
        }
        let used = placeholders(&self.body)?;
        let declared: BTreeSet<String> = self.variables.iter().map(|v| v.name.clone()).collect();
        if declared.len() != self.variables.len() {
            return Err(HarnessError::invalid_input("duplicate variable declaration"));
        }
        if let Some(u) = used.difference(&declared).next() {
            return Err(HarnessError::invalid_input(format!(
                "placeholder '{u}' is not declared"
            )));
        }
        if let Some(d) = declared.difference(&used).next() {
            return Err(HarnessError::invalid_input(format!(
                "declared variable '{d}' is unused"
            )));
        }
        Ok(())
    }

    pub fn render(&self, vars: &BTreeMap<String, TrustedText>) -> Result<RenderedPrompt> {
        self.validate()?;
        let mut text = self.body.clone();
        for v in &self.variables {
            let value = match vars.get(&v.name) {
                Some(t) => t.as_str(),
                None if v.required => {
                    return Err(HarnessError::invalid_input(format!(
                        "missing prompt variable '{}'",
                        v.name
                    )));
                }
                None => "",
            };
            if value.len() > v.max_len {
                return Err(HarnessError::invalid_input(format!(
                    "variable '{}' exceeds {} bytes",
                    v.name, v.max_len
                )));
            }
            // Values cannot introduce new placeholders (single-pass substitution).
            text = text.replace(&format!("{{{{{}}}}}", v.name), &value.replace("{{", "{ {"));
        }
        if let Some(extra) = vars
            .keys()
            .find(|k| !self.variables.iter().any(|v| &v.name == *k))
        {
            return Err(HarnessError::invalid_input(format!(
                "unknown prompt variable '{extra}'"
            )));
        }
        Ok(RenderedPrompt {
            id: self.id.clone(),
            version: self.version.clone(),
            kind: self.kind,
            text,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptStatus {
    Draft,
    Approved,
    Retired,
}

impl PromptStatus {
    fn as_str(self) -> &'static str {
        match self {
            PromptStatus::Draft => "draft",
            PromptStatus::Approved => "approved",
            PromptStatus::Retired => "retired",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptRegistry {
    db: Db,
    authz: Arc<dyn Authorizer>,
}

impl PromptRegistry {
    pub fn new(db: Db, authz: Arc<dyn Authorizer>) -> Self {
        Self { db, authz }
    }

    /// Register a new version as `draft`. Versions are immutable.
    pub async fn register(&self, actor: &Principal, t: &PromptTemplate) -> Result<()> {
        self.authz
            .authorize(actor, Action::PromptManage, Target::Platform)?;
        self.insert(Some(actor), t, PromptStatus::Draft).await
    }

    /// Load platform prompt files (`*.toml`) shipped with the deployment. Files are
    /// configuration under operator control, so they are registered as `approved` the
    /// first time they are seen; an identical re-load is a no-op and a changed file with
    /// the same version is rejected.
    pub async fn load_platform_dir(&self, dir: &Path) -> Result<Vec<String>> {
        let mut loaded = vec![];
        if !dir.exists() {
            return Ok(loaded);
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        {
            let t: PromptTemplate = toml::from_str(&std::fs::read_to_string(&p)?)
                .map_err(|e| HarnessError::invalid_input(format!("{}: {e}", p.display())))?;
            match self.get(&t.id, &t.version).await {
                Ok((existing, _)) if existing == t => {}
                Ok(_) => {
                    return Err(HarnessError::conflict(format!(
                        "{}: prompt {}@{} changed without a version bump",
                        p.display(),
                        t.id,
                        t.version
                    )));
                }
                Err(e) if e.kind == eaeh_core::ErrorKind::NotFound => {
                    self.insert(None, &t, PromptStatus::Draft).await?;
                    self.set_active(None, &t.id, &t.version).await?;
                }
                Err(e) => return Err(e),
            }
            loaded.push(format!("{}@{}", t.id, t.version));
        }
        Ok(loaded)
    }

    async fn insert(
        &self,
        actor: Option<&Principal>,
        t: &PromptTemplate,
        status: PromptStatus,
    ) -> Result<()> {
        t.validate()?;
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        sqlx::query("INSERT INTO prompt_templates (id, version, document, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(t.id.as_str())
            .bind(t.version.to_string())
            .bind(to_json(t)?)
            .bind(status.as_str())
            .bind(&now)
            .bind(&now)
            .execute(tx.conn())
            .await
            .db()?;
        let entry = match actor {
            Some(a) => AuditEntry::new(
                a,
                "prompt.register",
                "prompt",
                format!("{}@{}", t.id, t.version),
                AuditOutcome::Succeeded,
            ),
            None => AuditEntry::system(
                "prompt-loader",
                "prompt.register",
                "prompt",
                format!("{}@{}", t.id, t.version),
                AuditOutcome::Succeeded,
            ),
        };
        self.db
            .audit_in(&mut tx, entry.details(serde_json::json!({"kind": t.kind})))
            .await?;
        tx.commit().await
    }

    /// Approve a version, making it the single active version (other approved versions
    /// are retired). Approving an older version is a rollback.
    pub async fn approve(&self, actor: &Principal, id: &PromptId, version: &semver::Version) -> Result<()> {
        self.authz
            .authorize(actor, Action::PromptManage, Target::Platform)?;
        self.set_active(Some(actor), id, version).await
    }

    async fn set_active(
        &self,
        actor: Option<&Principal>,
        id: &PromptId,
        version: &semver::Version,
    ) -> Result<()> {
        self.get(id, version).await?;
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        sqlx::query("UPDATE prompt_templates SET status = 'retired', updated_at = ? WHERE id = ? AND status = 'approved'")
            .bind(&now)
            .bind(id.as_str())
            .execute(tx.conn())
            .await
            .db()?;
        sqlx::query(
            "UPDATE prompt_templates SET status = 'approved', updated_at = ? WHERE id = ? AND version = ?",
        )
        .bind(&now)
        .bind(id.as_str())
        .bind(version.to_string())
        .execute(tx.conn())
        .await
        .db()?;
        let rid = format!("{id}@{version}");
        let entry = match actor {
            Some(a) => AuditEntry::new(a, "prompt.approve", "prompt", rid, AuditOutcome::Succeeded),
            None => AuditEntry::system(
                "prompt-loader",
                "prompt.approve",
                "prompt",
                rid,
                AuditOutcome::Succeeded,
            ),
        };
        self.db.audit_in(&mut tx, entry).await?;
        tx.commit().await
    }

    pub async fn get(
        &self,
        id: &PromptId,
        version: &semver::Version,
    ) -> Result<(PromptTemplate, PromptStatus)> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT document, status FROM prompt_templates WHERE id = ? AND version = ?")
                .bind(id.as_str())
                .bind(version.to_string())
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        let (doc, status) =
            row.ok_or_else(|| HarnessError::not_found(format!("prompt {id}@{version} not found")))?;
        let status = match status.as_str() {
            "draft" => PromptStatus::Draft,
            "approved" => PromptStatus::Approved,
            _ => PromptStatus::Retired,
        };
        Ok((from_json(&doc)?, status))
    }

    /// The single approved version of a prompt.
    pub async fn active(&self, id: &PromptId) -> Result<PromptTemplate> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT document FROM prompt_templates WHERE id = ? AND status = 'approved'")
                .bind(id.as_str())
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        from_json(
            &row.ok_or_else(|| HarnessError::not_found(format!("no approved version of prompt {id}")))?
                .0,
        )
    }

    pub async fn history(&self, id: &PromptId) -> Result<Vec<(String, String, String)>> {
        sqlx::query_as(
            "SELECT version, status, updated_at FROM prompt_templates WHERE id = ? ORDER BY created_at",
        )
        .bind(id.as_str())
        .fetch_all(self.db.pool())
        .await
        .db()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eaeh_core::{PrincipalId, ProjectAccess, Role, TenantId};
    use eaeh_governance::authz::RbacAuthorizer;

    fn tpl(version: &str, body: &str) -> PromptTemplate {
        PromptTemplate {
            id: PromptId::new("agent.reviewer").unwrap(),
            version: semver::Version::parse(version).unwrap(),
            kind: PromptKind::Agent,
            description: "d".into(),
            body: body.into(),
            variables: vec![VariableSpec {
                name: "goal".into(),
                required: true,
                max_len: 100,
            }],
        }
    }

    #[test]
    fn validation_and_rendering() {
        let t = tpl("1.0.0", "Review for {{goal}}.");
        let r = t
            .render(&[("goal".to_string(), TrustedText::new("security {{goal}}"))].into())
            .unwrap();
        assert_eq!(r.text(), "Review for security { {goal}}.");
        assert_eq!(r.reference(), "agent.reviewer@1.0.0");
        assert!(t.render(&BTreeMap::new()).is_err());
        assert!(
            t.render(
                &[
                    ("other".to_string(), TrustedText::new("x")),
                    ("goal".to_string(), TrustedText::new("x"))
                ]
                .into()
            )
            .is_err()
        );
        assert!(tpl("1.0.0", "No vars").validate().is_err(), "declared but unused");
        assert!(tpl("1.0.0", "{{goal}} {{undeclared}}").validate().is_err());
    }

    #[tokio::test]
    async fn approval_rollback_and_immutability() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("p.db")).await.unwrap();
        let reg = PromptRegistry::new(db, Arc::new(RbacAuthorizer));
        let admin = Principal::human(
            PrincipalId::new("a").unwrap(),
            TenantId::new(),
            [Role::Admin].into(),
            ProjectAccess::All,
        );
        let op = Principal::human(
            PrincipalId::new("o").unwrap(),
            admin.tenant,
            [Role::Operator].into(),
            ProjectAccess::All,
        );
        let v1 = tpl("1.0.0", "v1 {{goal}}");
        let v2 = tpl("2.0.0", "v2 {{goal}}");
        assert!(reg.register(&op, &v1).await.is_err());
        reg.register(&admin, &v1).await.unwrap();
        reg.register(&admin, &v2).await.unwrap();
        assert!(
            reg.register(&admin, &tpl("1.0.0", "changed {{goal}}"))
                .await
                .is_err(),
            "immutable version"
        );
        assert!(reg.active(&v1.id).await.is_err(), "drafts are not active");
        reg.approve(&admin, &v1.id, &v1.version).await.unwrap();
        reg.approve(&admin, &v2.id, &v2.version).await.unwrap();
        assert_eq!(reg.active(&v1.id).await.unwrap().version, v2.version);
        reg.approve(&admin, &v1.id, &v1.version).await.unwrap(); // rollback
        assert_eq!(reg.active(&v1.id).await.unwrap().version, v1.version);
        let h = reg.history(&v1.id).await.unwrap();
        assert_eq!(h.iter().filter(|(_, s, _)| s == "approved").count(), 1);
    }
}
