//! Identity, credential provisioning and API tokens (clarification 4).
//!
//! * There are **no default credentials**. The first administrator is created by an
//!   explicit, one-time [`IdentityService::bootstrap`] that succeeds only while no
//!   principal exists; it returns a freshly generated token exactly once.
//! * Tokens are 256-bit random values (`eaeh_<64 hex>`). Only their SHA-256 hash is stored.
//! * Tokens expire (default 30 days, maximum 365), can be revoked, and can be rotated
//!   (new token issued, old token revoked and linked via `replaced_by`) atomically.
//! * Authentication failures are indistinguishable to the caller (unknown / revoked /
//!   expired / disabled principal all return `Unauthenticated`).

use crate::authz::{Action, Authorizer, Target};
use chrono::{DateTime, Duration, Utc};
use eaeh_core::hash::{random_bytes, sha256_hex, to_hex};
use eaeh_core::secret::TOKEN_PREFIX;
use eaeh_core::{
    HarnessError, Principal, PrincipalId, PrincipalKind, ProjectAccess, ProjectId, Result, Role,
    SecretString, TenantId, TokenId,
};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, WriteTx, from_json, parse_ts, sqlx, to_json, ts};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::collections::BTreeSet;
use std::sync::Arc;

pub const DEFAULT_TOKEN_TTL_DAYS: i64 = 30;
pub const MAX_TOKEN_TTL_DAYS: i64 = 365;

#[derive(Debug)]
pub struct IssuedToken {
    pub id: TokenId,
    /// Shown to the caller once; never persisted or logged.
    pub token: SecretString,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenInfo {
    pub id: String,
    pub principal_id: String,
    pub label: String,
    pub created_at: String,
    pub expires_at: String,
    pub revoked_at: Option<String>,
    pub replaced_by: Option<String>,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewPrincipal {
    pub id: PrincipalId,
    pub display_name: String,
    pub kind: PrincipalKind,
    pub roles: BTreeSet<Role>,
    pub projects: ProjectAccess,
}

#[derive(Debug, Clone)]
pub struct IdentityService {
    db: Db,
    authz: Arc<dyn Authorizer>,
}

fn new_token_value() -> Result<String> {
    Ok(format!("{TOKEN_PREFIX}{}", to_hex(&random_bytes::<32>()?)))
}

fn hash_token(token: &str) -> String {
    sha256_hex(token.as_bytes())
}

impl IdentityService {
    pub fn new(db: Db, authz: Arc<dyn Authorizer>) -> Self {
        Self { db, authz }
    }

    pub async fn principal_count(&self) -> Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM principals")
            .fetch_one(self.db.pool())
            .await
            .db()?;
        Ok(n)
    }

    /// One-time bootstrap of the first tenant, project and administrator.
    /// Fails with `Conflict` once any principal exists.
    pub async fn bootstrap(
        &self,
        tenant_name: &str,
        project_name: &str,
        admin_id: PrincipalId,
        display_name: &str,
        ttl_days: i64,
    ) -> Result<(Principal, ProjectId, IssuedToken)> {
        validate_name(tenant_name)?;
        validate_name(project_name)?;
        let mut tx = self.db.write().await?;
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM principals")
            .fetch_one(tx.conn())
            .await
            .db()?;
        if n > 0 {
            return Err(HarnessError::conflict(
                "bootstrap refused: principals already exist",
            ));
        }
        let now = ts(self.db.now());
        let tenant = TenantId::new();
        let project = ProjectId::new();
        sqlx::query("INSERT INTO tenants (id, name, created_at) VALUES (?, ?, ?)")
            .bind(tenant.to_string())
            .bind(tenant_name)
            .bind(&now)
            .execute(tx.conn())
            .await
            .db()?;
        insert_project(&mut tx, tenant, project, project_name, &now).await?;
        let principal = Principal::human(
            admin_id,
            tenant,
            [Role::Admin].into_iter().collect(),
            ProjectAccess::All,
        );
        insert_principal(&mut tx, &principal, display_name, None, &now).await?;
        let token = self
            .insert_token(&mut tx, &principal.id, "bootstrap", ttl_days)
            .await?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    &principal,
                    "identity.bootstrap",
                    "principal",
                    principal.id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .details(
                    json!({"tenant": tenant_name, "project": project_name, "token_id": token.id.to_string()}),
                ),
            )
            .await?;
        tx.commit().await?;
        Ok((principal, project, token))
    }

    pub async fn create_project(&self, actor: &Principal, tenant: TenantId, name: &str) -> Result<ProjectId> {
        self.authz
            .authorize(actor, Action::PrincipalManage, Target::Tenant(tenant))?;
        validate_name(name)?;
        let mut tx = self.db.write().await?;
        let id = ProjectId::new();
        insert_project(&mut tx, tenant, id, name, &ts(self.db.now())).await?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "project.create",
                    "project",
                    id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .details(json!({"name": name})),
            )
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn create_principal(&self, actor: &Principal, spec: NewPrincipal) -> Result<Principal> {
        self.authz
            .authorize(actor, Action::PrincipalManage, Target::Tenant(actor.tenant))?;
        let principal = match spec.kind {
            PrincipalKind::Human => Principal::human(spec.id, actor.tenant, spec.roles, spec.projects),
            PrincipalKind::Service => Principal::service(spec.id, actor.tenant, spec.roles, spec.projects),
            PrincipalKind::Agent { .. } => {
                return Err(HarnessError::invalid_input(
                    "agent principals are created by the agent runtime only",
                ));
            }
        };
        if let ProjectAccess::Only(ps) = &principal.projects {
            for p in ps {
                self.ensure_project(actor.tenant, *p).await?;
            }
        }
        let mut tx = self.db.write().await?;
        insert_principal(
            &mut tx,
            &principal,
            &spec.display_name,
            Some(&actor.id),
            &ts(self.db.now()),
        )
        .await?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "principal.create",
                    "principal",
                    principal.id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .details(json!({"roles": principal.roles(), "kind": principal.kind_str()})),
            )
            .await?;
        tx.commit().await?;
        Ok(principal)
    }

    pub async fn ensure_project(&self, tenant: TenantId, project: ProjectId) -> Result<()> {
        let row: Option<(String,)> = sqlx::query_as("SELECT id FROM projects WHERE id = ? AND tenant_id = ?")
            .bind(project.to_string())
            .bind(tenant.to_string())
            .fetch_optional(self.db.pool())
            .await
            .db()?;
        row.map(|_| ())
            .ok_or_else(|| HarnessError::not_found(format!("project {project} not found in tenant")))
    }

    pub async fn projects(&self, tenant: TenantId) -> Result<Vec<(ProjectId, String)>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, name FROM projects WHERE tenant_id = ? ORDER BY name")
                .bind(tenant.to_string())
                .fetch_all(self.db.pool())
                .await
                .db()?;
        rows.into_iter().map(|(id, n)| Ok((id.parse()?, n))).collect()
    }

    /// Assign a project to a knowledge domain (ABAC attribute used for domain-scoped knowledge).
    pub async fn set_project_domain(
        &self,
        actor: &Principal,
        project: ProjectId,
        domain: Option<&str>,
    ) -> Result<()> {
        self.authz
            .authorize(actor, Action::PrincipalManage, Target::Tenant(actor.tenant))?;
        if let Some(d) = domain {
            eaeh_core::ids::validate_slug(d)
                .map_err(|m| HarnessError::invalid_input(format!("domain: {m}")))?;
        }
        let mut tx = self.db.write().await?;
        let r = sqlx::query("UPDATE projects SET domain = ? WHERE id = ? AND tenant_id = ?")
            .bind(domain)
            .bind(project.to_string())
            .bind(actor.tenant.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        if r.rows_affected() == 0 {
            return Err(HarnessError::not_found("project not found in tenant"));
        }
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "project.set_domain",
                    "project",
                    project.to_string(),
                    AuditOutcome::Succeeded,
                )
                .details(json!({"domain": domain})),
            )
            .await?;
        tx.commit().await
    }

    pub async fn project_domain(&self, tenant: TenantId, project: ProjectId) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT domain FROM projects WHERE id = ? AND tenant_id = ?")
                .bind(project.to_string())
                .bind(tenant.to_string())
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        Ok(row.and_then(|r| r.0))
    }

    /// Grant an existing (human or service) principal access to one more project of the
    /// same tenant (developer-experience bootstrap). Idempotent; roles are unchanged.
    /// Returns `true` when access was added.
    pub async fn grant_project_access(
        &self,
        actor: &Principal,
        principal: &PrincipalId,
        project: ProjectId,
    ) -> Result<bool> {
        self.authz
            .authorize(actor, Action::PrincipalManage, Target::Tenant(actor.tenant))?;
        self.ensure_project(actor.tenant, project).await?;
        let mut tx = self.db.write().await?;
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT tenant_id, projects FROM principals WHERE id = ?")
                .bind(principal.as_str())
                .fetch_optional(tx.conn())
                .await
                .db()?;
        let Some((tenant, projects)) = row else {
            return Err(HarnessError::not_found("principal not found"));
        };
        if tenant != actor.tenant.to_string() {
            // Cross-tenant: indistinguishable from not found.
            return Err(HarnessError::not_found("principal not found"));
        }
        let access: ProjectAccess = from_json(&projects)?;
        let updated = match access {
            ProjectAccess::All => return Ok(false),
            ProjectAccess::Only(mut set) => {
                if !set.insert(project) {
                    return Ok(false);
                }
                ProjectAccess::Only(set)
            }
        };
        sqlx::query("UPDATE principals SET projects = ? WHERE id = ?")
            .bind(to_json(&updated)?)
            .bind(principal.as_str())
            .execute(tx.conn())
            .await
            .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "principal.grant_project",
                    "principal",
                    principal.to_string(),
                    AuditOutcome::Succeeded,
                )
                .scope(eaeh_core::Scope::new(actor.tenant, project))
                .details(json!({"project": project})),
            )
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Assign team memberships (ABAC attribute used for team-scoped knowledge).
    pub async fn set_principal_teams(
        &self,
        actor: &Principal,
        id: &PrincipalId,
        teams: &[String],
    ) -> Result<()> {
        self.authz
            .authorize(actor, Action::PrincipalManage, Target::Tenant(actor.tenant))?;
        for t in teams {
            eaeh_core::ids::validate_slug(t)
                .map_err(|m| HarnessError::invalid_input(format!("team '{t}': {m}")))?;
        }
        let mut tx = self.db.write().await?;
        let r = sqlx::query("UPDATE principals SET teams = ? WHERE id = ? AND tenant_id = ?")
            .bind(to_json(&teams)?)
            .bind(id.as_str())
            .bind(actor.tenant.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        if r.rows_affected() == 0 {
            return Err(HarnessError::not_found("principal not found"));
        }
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "principal.set_teams",
                    "principal",
                    id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .details(json!({"teams": teams})),
            )
            .await?;
        tx.commit().await
    }

    pub async fn principal_teams(&self, id: &PrincipalId) -> Result<Vec<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT teams FROM principals WHERE id = ?")
            .bind(id.as_str())
            .fetch_optional(self.db.pool())
            .await
            .db()?;
        row.map(|(t,)| from_json(&t))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    pub async fn set_principal_status(
        &self,
        actor: &Principal,
        id: &PrincipalId,
        active: bool,
    ) -> Result<()> {
        self.authz
            .authorize(actor, Action::PrincipalManage, Target::Tenant(actor.tenant))?;
        let mut tx = self.db.write().await?;
        let r = sqlx::query("UPDATE principals SET status = ? WHERE id = ? AND tenant_id = ?")
            .bind(if active { "active" } else { "disabled" })
            .bind(id.as_str())
            .bind(actor.tenant.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        if r.rows_affected() == 0 {
            return Err(HarnessError::not_found("principal not found"));
        }
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    if active {
                        "principal.enable"
                    } else {
                        "principal.disable"
                    },
                    "principal",
                    id.to_string(),
                    AuditOutcome::Succeeded,
                ),
            )
            .await?;
        tx.commit().await
    }

    pub async fn get_principal(&self, id: &PrincipalId) -> Result<(Principal, bool)> {
        let row = sqlx::query("SELECT * FROM principals WHERE id = ?")
            .bind(id.as_str())
            .fetch_optional(self.db.pool())
            .await
            .db()?
            .ok_or_else(|| HarnessError::not_found("principal not found"))?;
        row_to_principal(&row)
    }

    /// Resolve a bearer token to an active principal.
    pub async fn authenticate(&self, token: &str) -> Result<Principal> {
        self.authenticate_detailed(token).await.map(|(p, _, _)| p)
    }

    /// Authenticate and also return the token id and its expiry (RFC 3339), so a local
    /// client can store credential metadata and diagnose expiry without guessing.
    pub async fn authenticate_detailed(&self, token: &str) -> Result<(Principal, String, String)> {
        let unauth = || HarnessError::unauthenticated("invalid or expired credentials");
        if !token.starts_with(TOKEN_PREFIX) || token.len() != TOKEN_PREFIX.len() + 64 {
            return Err(unauth());
        }
        let hash = hash_token(token);
        let row = sqlx::query(
            "SELECT t.id AS token_id, t.expires_at, t.revoked_at, p.* FROM api_tokens t \
             JOIN principals p ON p.id = t.principal_id WHERE t.token_hash = ?",
        )
        .bind(&hash)
        .fetch_optional(self.db.pool())
        .await
        .db()?
        .ok_or_else(unauth)?;
        let revoked: Option<String> = row.try_get("revoked_at").db()?;
        let expires_raw: String = row.try_get("expires_at").db()?;
        let expires = parse_ts(&expires_raw)?;
        let (principal, active) = row_to_principal(&row)?;
        if revoked.is_some() || expires <= self.db.now() || !active {
            return Err(unauth());
        }
        let token_id: String = row.try_get("token_id").db()?;
        // Usage tracking (best effort: failures do not fail authentication).
        if let Ok(mut tx) = self.db.write().await {
            let ok = sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ?")
                .bind(ts(self.db.now()))
                .bind(&token_id)
                .execute(tx.conn())
                .await
                .is_ok();
            if ok {
                let _ = tx.commit().await;
            }
        }
        Ok((principal, token_id, expires_raw))
    }

    pub async fn issue_token(
        &self,
        actor: &Principal,
        for_principal: &PrincipalId,
        label: &str,
        ttl_days: i64,
    ) -> Result<IssuedToken> {
        self.authorize_token_admin(actor, for_principal).await?;
        let mut tx = self.db.write().await?;
        let t = self.insert_token(&mut tx, for_principal, label, ttl_days).await?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(actor, "token.issue", "api_token", t.id.to_string(), AuditOutcome::Succeeded)
                    .details(json!({"principal_id": for_principal.as_str(), "label": label, "expires_at": ts(t.expires_at)})),
            )
            .await?;
        tx.commit().await?;
        Ok(t)
    }

    pub async fn revoke_token(&self, actor: &Principal, token_id: TokenId) -> Result<()> {
        let owner = self.token_owner(token_id).await?;
        self.authorize_token_admin(actor, &owner).await?;
        let mut tx = self.db.write().await?;
        let r = sqlx::query("UPDATE api_tokens SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL")
            .bind(ts(self.db.now()))
            .bind(token_id.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        if r.rows_affected() == 0 {
            return Err(HarnessError::conflict("token already revoked"));
        }
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "token.revoke",
                    "api_token",
                    token_id.to_string(),
                    AuditOutcome::Succeeded,
                ),
            )
            .await?;
        tx.commit().await
    }

    /// Issue a replacement token and revoke the old one in a single transaction.
    pub async fn rotate_token(
        &self,
        actor: &Principal,
        token_id: TokenId,
        ttl_days: i64,
    ) -> Result<IssuedToken> {
        let owner = self.token_owner(token_id).await?;
        self.authorize_token_admin(actor, &owner).await?;
        let mut tx = self.db.write().await?;
        let label: Option<(String,)> =
            sqlx::query_as("SELECT label FROM api_tokens WHERE id = ? AND revoked_at IS NULL")
                .bind(token_id.to_string())
                .fetch_optional(tx.conn())
                .await
                .db()?;
        let (label,) = label.ok_or_else(|| HarnessError::conflict("token is revoked or unknown"))?;
        let new = self.insert_token(&mut tx, &owner, &label, ttl_days).await?;
        sqlx::query("UPDATE api_tokens SET revoked_at = ?, replaced_by = ? WHERE id = ?")
            .bind(ts(self.db.now()))
            .bind(new.id.to_string())
            .bind(token_id.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "token.rotate",
                    "api_token",
                    token_id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .details(json!({"replaced_by": new.id.to_string()})),
            )
            .await?;
        tx.commit().await?;
        Ok(new)
    }

    pub async fn list_tokens(&self, actor: &Principal, principal: &PrincipalId) -> Result<Vec<TokenInfo>> {
        self.authorize_token_admin(actor, principal).await?;
        let rows = sqlx::query(
            "SELECT id, principal_id, label, created_at, expires_at, revoked_at, replaced_by, last_used_at \
             FROM api_tokens WHERE principal_id = ? ORDER BY created_at",
        )
        .bind(principal.as_str())
        .fetch_all(self.db.pool())
        .await
        .db()?;
        rows.iter()
            .map(|r| {
                Ok(TokenInfo {
                    id: r.try_get("id").db()?,
                    principal_id: r.try_get("principal_id").db()?,
                    label: r.try_get("label").db()?,
                    created_at: r.try_get("created_at").db()?,
                    expires_at: r.try_get("expires_at").db()?,
                    revoked_at: r.try_get("revoked_at").db()?,
                    replaced_by: r.try_get("replaced_by").db()?,
                    last_used_at: r.try_get("last_used_at").db()?,
                })
            })
            .collect()
    }

    async fn token_owner(&self, token_id: TokenId) -> Result<PrincipalId> {
        let row: Option<(String,)> = sqlx::query_as("SELECT principal_id FROM api_tokens WHERE id = ?")
            .bind(token_id.to_string())
            .fetch_optional(self.db.pool())
            .await
            .db()?;
        PrincipalId::new(row.ok_or_else(|| HarnessError::not_found("token not found"))?.0)
    }

    async fn authorize_token_admin(&self, actor: &Principal, target: &PrincipalId) -> Result<()> {
        let (target_p, _) = self.get_principal(target).await?;
        if target_p.tenant != actor.tenant {
            return Err(HarnessError::forbidden("cross-tenant token management denied"));
        }
        if &actor.id == target {
            self.authz
                .authorize(actor, Action::TokenManageOwn, Target::Tenant(actor.tenant))
        } else {
            self.authz
                .authorize(actor, Action::TokenManageAny, Target::Tenant(actor.tenant))
        }
    }

    async fn insert_token(
        &self,
        tx: &mut WriteTx,
        principal: &PrincipalId,
        label: &str,
        ttl_days: i64,
    ) -> Result<IssuedToken> {
        if !(1..=MAX_TOKEN_TTL_DAYS).contains(&ttl_days) {
            return Err(HarnessError::invalid_input(format!(
                "token ttl must be 1..={MAX_TOKEN_TTL_DAYS} days"
            )));
        }
        if label.len() > 100 {
            return Err(HarnessError::invalid_input("token label too long"));
        }
        let value = new_token_value()?;
        let id = TokenId::new();
        let now = self.db.now();
        let expires_at = now + Duration::days(ttl_days);
        sqlx::query(
            "INSERT INTO api_tokens (id, principal_id, token_hash, label, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(principal.as_str())
        .bind(hash_token(&value))
        .bind(label)
        .bind(ts(now))
        .bind(ts(expires_at))
        .execute(tx.conn())
        .await
        .db()?;
        let token = SecretString::new(value);
        self.db.redactor().register(&token);
        Ok(IssuedToken {
            id,
            token,
            expires_at,
        })
    }
}

fn validate_name(n: &str) -> Result<()> {
    if n.trim().is_empty() || n.len() > 100 || n.chars().any(|c| c.is_control()) {
        return Err(HarnessError::invalid_input(
            "name must be 1..=100 printable characters",
        ));
    }
    Ok(())
}

async fn insert_project(
    tx: &mut WriteTx,
    tenant: TenantId,
    id: ProjectId,
    name: &str,
    now: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO projects (id, tenant_id, name, created_at) VALUES (?, ?, ?, ?)")
        .bind(id.to_string())
        .bind(tenant.to_string())
        .bind(name)
        .bind(now)
        .execute(tx.conn())
        .await
        .db()
        .map(|_| ())
}

async fn insert_principal(
    tx: &mut WriteTx,
    p: &Principal,
    display: &str,
    created_by: Option<&PrincipalId>,
    now: &str,
) -> Result<()> {
    validate_name(display)?;
    sqlx::query(
        "INSERT INTO principals (id, kind, tenant_id, display_name, roles, projects, status, created_at, created_by) \
         VALUES (?, ?, ?, ?, ?, ?, 'active', ?, ?)",
    )
    .bind(p.id.as_str())
    .bind(p.kind_str())
    .bind(p.tenant.to_string())
    .bind(display)
    .bind(to_json(p.roles())?)
    .bind(to_json(&p.projects)?)
    .bind(now)
    .bind(created_by.map(|c| c.to_string()))
    .execute(tx.conn())
    .await
    .db()
    .map(|_| ())
}

fn row_to_principal(row: &sqlx::sqlite::SqliteRow) -> Result<(Principal, bool)> {
    let id = PrincipalId::new(row.try_get::<String, _>("id").db()?)?;
    let tenant: TenantId = row.try_get::<String, _>("tenant_id").db()?.parse()?;
    let roles: BTreeSet<Role> = from_json(&row.try_get::<String, _>("roles").db()?)?;
    let projects: ProjectAccess = from_json(&row.try_get::<String, _>("projects").db()?)?;
    let kind: String = row.try_get("kind").db()?;
    let status: String = row.try_get("status").db()?;
    let p = match kind.as_str() {
        "human" => Principal::human(id, tenant, roles, projects),
        "service" => Principal::service(id, tenant, roles, projects),
        _ => return Err(HarnessError::integrity("unknown principal kind")),
    };
    Ok((p, status == "active"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authz::RbacAuthorizer;

    async fn svc() -> (tempfile::TempDir, IdentityService) {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("id.db")).await.unwrap();
        (d, IdentityService::new(db, Arc::new(RbacAuthorizer)))
    }

    #[tokio::test]
    async fn bootstrap_is_one_time_and_token_authenticates() {
        let (_d, s) = svc().await;
        let (admin, _p, tok) = s
            .bootstrap("acme", "core", PrincipalId::new("admin").unwrap(), "Admin", 30)
            .await
            .unwrap();
        assert!(tok.token.expose().starts_with("eaeh_"));
        let p = s.authenticate(tok.token.expose()).await.unwrap();
        assert_eq!(p.id, admin.id);
        assert!(p.has_role(Role::Admin));
        let again = s
            .bootstrap("x", "y", PrincipalId::new("admin2").unwrap(), "A", 30)
            .await;
        assert_eq!(again.unwrap_err().kind, eaeh_core::ErrorKind::Conflict);
        // Plaintext token is not stored.
        let (cnt,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_tokens WHERE token_hash = ?")
            .bind(tok.token.expose())
            .fetch_one(s.db.pool())
            .await
            .unwrap();
        assert_eq!(cnt, 0);
    }

    #[tokio::test]
    async fn revoke_rotate_and_reject_invalid() {
        let (_d, s) = svc().await;
        let (admin, _p, tok) = s
            .bootstrap("acme", "core", PrincipalId::new("admin").unwrap(), "Admin", 30)
            .await
            .unwrap();
        let rotated = s.rotate_token(&admin, tok.id, 10).await.unwrap();
        assert!(
            s.authenticate(tok.token.expose()).await.is_err(),
            "old token revoked by rotation"
        );
        assert!(s.authenticate(rotated.token.expose()).await.is_ok());
        s.revoke_token(&admin, rotated.id).await.unwrap();
        assert_eq!(
            s.authenticate(rotated.token.expose()).await.unwrap_err().kind,
            eaeh_core::ErrorKind::Unauthenticated
        );
        assert!(s.authenticate("eaeh_short").await.is_err());
        assert!(s.authenticate(&format!("eaeh_{}", "0".repeat(64))).await.is_err());
        let list = s.list_tokens(&admin, &admin.id).await.unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|t| t.revoked_at.is_some()));
    }

    #[tokio::test]
    async fn non_admin_cannot_manage_others_or_create_principals() {
        let (_d, s) = svc().await;
        let (admin, project, _tok) = s
            .bootstrap("acme", "core", PrincipalId::new("admin").unwrap(), "Admin", 30)
            .await
            .unwrap();
        let op = s
            .create_principal(
                &admin,
                NewPrincipal {
                    id: PrincipalId::new("op").unwrap(),
                    display_name: "Op".into(),
                    kind: PrincipalKind::Human,
                    roles: [Role::Operator].into(),
                    projects: ProjectAccess::Only([project].into()),
                },
            )
            .await
            .unwrap();
        assert!(s.issue_token(&op, &op.id, "mine", 5).await.is_ok());
        assert_eq!(
            s.issue_token(&op, &admin.id, "steal", 5).await.unwrap_err().kind,
            eaeh_core::ErrorKind::Forbidden
        );
        let r = s
            .create_principal(
                &op,
                NewPrincipal {
                    id: PrincipalId::new("evil").unwrap(),
                    display_name: "E".into(),
                    kind: PrincipalKind::Human,
                    roles: [Role::Admin].into(),
                    projects: ProjectAccess::All,
                },
            )
            .await;
        assert_eq!(r.unwrap_err().kind, eaeh_core::ErrorKind::Forbidden);
        // Disabled principal's token no longer authenticates.
        let t = s.issue_token(&admin, &op.id, "x", 5).await.unwrap();
        s.set_principal_status(&admin, &op.id, false).await.unwrap();
        assert!(s.authenticate(t.token.expose()).await.is_err());
    }

    #[tokio::test]
    async fn expired_tokens_rejected() {
        let d = tempfile::tempdir().unwrap();
        let clock = eaeh_core::clock::ManualClock::new(Utc::now());
        let db = Db::open_with(
            &d.path().join("x.db"),
            Arc::new(clock.clone()),
            Default::default(),
        )
        .await
        .unwrap();
        let s = IdentityService::new(db, Arc::new(RbacAuthorizer));
        let (_a, _p, tok) = s
            .bootstrap("acme", "core", PrincipalId::new("admin").unwrap(), "Admin", 1)
            .await
            .unwrap();
        assert!(s.authenticate(tok.token.expose()).await.is_ok());
        clock.advance(Duration::days(2));
        assert!(s.authenticate(tok.token.expose()).await.is_err());
    }

    #[tokio::test]
    async fn grant_project_access_is_admin_only_tenant_bound_and_detailed_auth_reports_expiry() {
        let (_d, s) = svc().await;
        let (admin, p1, tok) = s
            .bootstrap("acme", "core", PrincipalId::new("admin").unwrap(), "Admin", 30)
            .await
            .unwrap();
        let (_, token_id, expires) = s.authenticate_detailed(tok.token.expose()).await.unwrap();
        assert_eq!(token_id, tok.id.to_string());
        assert!(expires.starts_with(&tok.expires_at.format("%Y-%m-%d").to_string()));
        let p2 = s.create_project(&admin, admin.tenant, "second").await.unwrap();
        let dev = s
            .create_principal(
                &admin,
                NewPrincipal {
                    id: PrincipalId::new("dev").unwrap(),
                    display_name: "Dev".into(),
                    kind: PrincipalKind::Human,
                    roles: [Role::Operator].into(),
                    projects: ProjectAccess::Only([p1].into()),
                },
            )
            .await
            .unwrap();
        // Operators cannot grant (not even to themselves).
        assert_eq!(
            s.grant_project_access(&dev, &dev.id, p2).await.unwrap_err().kind,
            eaeh_core::ErrorKind::Forbidden
        );
        assert!(s.grant_project_access(&admin, &dev.id, p2).await.unwrap());
        let (d2, _) = s.get_principal(&dev.id).await.unwrap();
        assert!(d2.can_access(&eaeh_core::Scope::new(admin.tenant, p2)));
        assert_eq!(d2.roles(), dev.roles(), "roles are never changed by a grant");
        // An administrator of another tenant sees neither the principal nor the project.
        let foreign = Principal::human(
            PrincipalId::new("eve").unwrap(),
            TenantId::new(),
            [Role::Admin].into(),
            ProjectAccess::All,
        );
        let e = s.grant_project_access(&foreign, &dev.id, p1).await.unwrap_err();
        assert_eq!(e.kind, eaeh_core::ErrorKind::NotFound);
    }
}
