//! Plugin manifests, integrity verification, lifecycle and registry (master §9, D-03).
//!
//! **Trust model (clarification 1).** Phase 1 executes only *trusted in-process Rust
//! extensions compiled into the binary*. There is no dynamic loading of libraries.
//! Manifest digests and ed25519 signatures establish *provenance and integrity of the
//! declaration*; they do **not** isolate plugin code at runtime. An enabled in-process
//! extension runs with the privileges of the harness process. Its only sanctioned access
//! paths are the registries it is granted capabilities for, and every tool it contributes
//! is still invoked through the governed tool gateway.
//!
//! WASM and external-process manifests can be registered and validated, but enabling
//! them fails with `NotSupported` because their execution hosts are deferred.

use eaeh_core::hash::{canonical_json, from_hex, sha256, to_hex};
use eaeh_core::{HarnessError, PluginId, Principal, Result, TraceContext};
use eaeh_events::catalog::PluginStateChanged;
use eaeh_events::{EventEnvelope, outbox};
use eaeh_governance::authz::{Action, Authorizer, Target};
use eaeh_governance::policy::{Effect, PolicyEngine, PolicyRequest};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{Db, DbResultExt, from_json, sqlx, to_json, ts};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const PLUGIN_API_VERSION: semver::Version = semver::Version::new(1, 0, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    InProcess,
    Wasm,
    ExternalProcess,
}

impl PluginKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PluginKind::InProcess => "in_process",
            PluginKind::Wasm => "wasm",
            PluginKind::ExternalProcess => "external_process",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    RegisterHooks,
    RegisterTools,
    RegisterMiddleware,
    RegisterInterceptors,
    RegisterModelProviders,
    RegisterValidators,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDependency {
    pub id: PluginId,
    pub version: semver::VersionReq,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimitsSpec {
    pub timeout_ms: u64,
    pub memory_mb: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Integrity {
    /// SHA-256 (hex) of the canonical manifest JSON with `integrity` removed.
    pub digest: String,
    /// ed25519 signature (hex) over the raw 32-byte digest.
    pub signature: Option<String>,
    pub key_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: PluginId,
    pub version: semver::Version,
    pub kind: PluginKind,
    pub description: String,
    /// Compatible plugin API versions (e.g. `^1`).
    pub eaeh_api: semver::VersionReq,
    #[serde(default)]
    pub dependencies: Vec<PluginDependency>,
    pub capabilities: BTreeSet<PluginCapability>,
    #[serde(default)]
    pub config_schema: Option<serde_json::Value>,
    pub resource_limits: ResourceLimitsSpec,
    #[serde(default)]
    pub integrity: Option<Integrity>,
}

impl PluginManifest {
    pub fn compute_digest(&self) -> Result<[u8; 32]> {
        let mut v = serde_json::to_value(self)?;
        if let Some(o) = v.as_object_mut() {
            o.remove("integrity");
        }
        Ok(sha256(canonical_json(&v).as_bytes()))
    }

    pub fn validate(&self) -> Result<()> {
        if self.description.trim().is_empty() {
            return Err(HarnessError::invalid_input("plugin description required"));
        }
        if !self.eaeh_api.matches(&PLUGIN_API_VERSION) {
            return Err(HarnessError::invalid_input(format!(
                "plugin {} requires API {} (platform {})",
                self.id, self.eaeh_api, PLUGIN_API_VERSION
            )));
        }
        if self.resource_limits.timeout_ms == 0 || self.resource_limits.timeout_ms > 600_000 {
            return Err(HarnessError::invalid_input(
                "resource_limits.timeout_ms must be 1..=600000",
            ));
        }
        if self.dependencies.iter().any(|d| d.id == self.id) {
            return Err(HarnessError::invalid_input("plugin cannot depend on itself"));
        }
        Ok(())
    }
}

/// Integrity / trust configuration.
#[derive(Debug, Clone, Default)]
pub struct TrustPolicy {
    pub require_signature: bool,
    /// key id → ed25519 public key bytes.
    pub trusted_keys: BTreeMap<String, [u8; 32]>,
    /// Upper bound of capabilities that may be granted to any plugin.
    pub grantable: BTreeSet<PluginCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedManifest {
    pub digest: String,
    pub signed_by: Option<String>,
}

/// Validate a manifest and its integrity metadata against the trust policy.
pub fn verify_manifest(m: &PluginManifest, trust: &TrustPolicy) -> Result<VerifiedManifest> {
    m.validate()?;
    let digest = m.compute_digest()?;
    let digest_hex = to_hex(&digest);
    let Some(integrity) = &m.integrity else {
        if trust.require_signature {
            return Err(HarnessError::integrity(
                "unsigned plugin manifest rejected (signature required)",
            ));
        }
        return Ok(VerifiedManifest {
            digest: digest_hex,
            signed_by: None,
        });
    };
    if integrity.digest != digest_hex {
        return Err(HarnessError::integrity(
            "manifest digest mismatch (manifest modified after signing)",
        ));
    }
    match (&integrity.signature, &integrity.key_id) {
        (Some(sig_hex), Some(key_id)) => {
            let key_bytes = trust
                .trusted_keys
                .get(key_id)
                .ok_or_else(|| HarnessError::integrity(format!("signing key '{key_id}' is not trusted")))?;
            let key = VerifyingKey::from_bytes(key_bytes)
                .map_err(|_| HarnessError::integrity("trusted key is not a valid ed25519 key"))?;
            let sig_bytes = from_hex(sig_hex)?;
            let sig = Signature::from_slice(&sig_bytes)
                .map_err(|_| HarnessError::integrity("malformed signature"))?;
            key.verify_strict(&digest, &sig)
                .map_err(|_| HarnessError::integrity("invalid manifest signature"))?;
            Ok(VerifiedManifest {
                digest: digest_hex,
                signed_by: Some(key_id.clone()),
            })
        }
        (None, _) if !trust.require_signature => Ok(VerifiedManifest {
            digest: digest_hex,
            signed_by: None,
        }),
        _ => Err(HarnessError::integrity(
            "plugin manifest signature and key_id required",
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    Validated,
    Enabled,
    Disabled,
    Failed,
    Retired,
}

impl PluginState {
    fn as_str(self) -> &'static str {
        match self {
            PluginState::Validated => "validated",
            PluginState::Enabled => "enabled",
            PluginState::Disabled => "disabled",
            PluginState::Failed => "failed",
            PluginState::Retired => "retired",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "validated" => Self::Validated,
            "enabled" => Self::Enabled,
            "disabled" => Self::Disabled,
            "failed" => Self::Failed,
            "retired" => Self::Retired,
            _ => return Err(HarnessError::integrity("unknown plugin state")),
        })
    }

    /// Allowed lifecycle transitions.
    pub fn can_transition(self, to: PluginState) -> bool {
        use PluginState::*;
        matches!(
            (self, to),
            (Validated, Enabled)
                | (Validated, Retired)
                | (Enabled, Disabled)
                | (Enabled, Failed)
                | (Disabled, Enabled)
                | (Disabled, Retired)
                | (Failed, Disabled)
                | (Failed, Retired)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginRecord {
    pub manifest: PluginManifest,
    pub digest: String,
    pub signed_by: Option<String>,
    pub state: PluginState,
    pub granted: BTreeSet<PluginCapability>,
}

#[derive(Clone)]
pub struct PluginRegistry {
    db: Db,
    authz: Arc<dyn Authorizer>,
    policy: Arc<dyn PolicyEngine>,
    trust: TrustPolicy,
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRegistry")
            .field("require_signature", &self.trust.require_signature)
            .finish()
    }
}

impl PluginRegistry {
    pub fn new(
        db: Db,
        authz: Arc<dyn Authorizer>,
        policy: Arc<dyn PolicyEngine>,
        trust: TrustPolicy,
    ) -> Self {
        Self {
            db,
            authz,
            policy,
            trust,
        }
    }

    /// Register (validate + persist) a manifest. Versions are immutable.
    pub async fn register(&self, actor: &Principal, m: PluginManifest) -> Result<PluginRecord> {
        self.authz
            .authorize(actor, Action::PluginManage, Target::Platform)?;
        self.register_inner(Some(actor), m).await
    }

    /// Register the manifest of an extension compiled into this binary. Idempotent for an
    /// identical manifest; a changed manifest under the same version is rejected. The
    /// plugin stays `validated` until an administrator enables it.
    pub async fn register_compiled(&self, m: PluginManifest) -> Result<PluginRecord> {
        match self.get(&m.id, &m.version).await {
            Ok(rec) => {
                let digest = to_hex(&m.compute_digest()?);
                if rec.digest != digest {
                    return Err(HarnessError::integrity(format!(
                        "compiled plugin {}@{} differs from its registered manifest",
                        m.id, m.version
                    )));
                }
                Ok(rec)
            }
            Err(e) if e.kind == eaeh_core::ErrorKind::NotFound => self.register_inner(None, m).await,
            Err(e) => Err(e),
        }
    }

    async fn register_inner(&self, actor: Option<&Principal>, m: PluginManifest) -> Result<PluginRecord> {
        let pid = format!("{}@{}", m.id, m.version);
        let entry = |outcome| match actor {
            Some(a) => AuditEntry::new(a, "plugin.register", "plugin", pid.clone(), outcome),
            None => AuditEntry::system("plugin-loader", "plugin.register", "plugin", pid.clone(), outcome),
        };
        let verified = match verify_manifest(&m, &self.trust) {
            Ok(v) => v,
            Err(e) => {
                self.db
                    .audit(entry(AuditOutcome::Denied).details(json!({"reason": e.to_string()})))
                    .await?;
                return Err(e);
            }
        };
        for dep in &m.dependencies {
            let ok = self.list().await?.into_iter().any(|r| {
                r.manifest.id == dep.id
                    && dep.version.matches(&r.manifest.version)
                    && r.state != PluginState::Retired
            });
            if !ok {
                return Err(HarnessError::invalid_input(format!(
                    "unresolved dependency {} {}",
                    dep.id, dep.version
                )));
            }
        }
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO plugins (id, version, kind, manifest, digest, signed_by, state, granted, registered_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'validated', '[]', ?, ?)",
        )
        .bind(m.id.as_str())
        .bind(m.version.to_string())
        .bind(m.kind.as_str())
        .bind(to_json(&m)?)
        .bind(&verified.digest)
        .bind(&verified.signed_by)
        .bind(&now)
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        self.db
            .audit_in(
                &mut tx,
                entry(AuditOutcome::Succeeded)
                    .details(json!({"digest": verified.digest, "signed_by": verified.signed_by, "kind": m.kind.as_str()})),
            )
            .await?;
        tx.commit().await?;
        self.get(&m.id, &m.version).await
    }

    /// Enable a plugin; returns the capabilities actually granted.
    pub async fn enable(
        &self,
        actor: &Principal,
        id: &PluginId,
        version: &semver::Version,
    ) -> Result<PluginRecord> {
        self.authz
            .authorize(actor, Action::PluginManage, Target::Platform)?;
        let rec = self.get(id, version).await?;
        let decision = self.policy.evaluate(&PolicyRequest {
            action: "plugin.enable".into(),
            principal_id: actor.id.to_string(),
            principal_kind: actor.kind_str().into(),
            autonomy: None,
            risk: None,
            resource_type: "plugin".into(),
            resource_id: id.to_string(),
            scope: None,
            attributes: [("plugin_kind".to_string(), rec.manifest.kind.as_str().to_string())].into(),
        });
        if decision.effect != Effect::Permit {
            self.db
                .audit(
                    AuditEntry::new(
                        actor,
                        "plugin.enable",
                        "plugin",
                        format!("{id}@{version}"),
                        AuditOutcome::Denied,
                    )
                    .details(json!({"policy": decision})),
                )
                .await?;
            return Err(HarnessError::policy_denied(format!(
                "plugin enable denied: {}",
                decision.reason
            )));
        }
        if rec.manifest.kind != PluginKind::InProcess {
            return Err(HarnessError::not_supported(format!(
                "{} plugin execution host is not available in Phase 1",
                rec.manifest.kind.as_str()
            )));
        }
        let granted: BTreeSet<PluginCapability> = rec
            .manifest
            .capabilities
            .intersection(&self.trust.grantable)
            .copied()
            .collect();
        self.transition(actor, &rec, PluginState::Enabled, Some(&granted))
            .await
    }

    pub async fn disable(
        &self,
        actor: &Principal,
        id: &PluginId,
        version: &semver::Version,
    ) -> Result<PluginRecord> {
        self.authz
            .authorize(actor, Action::PluginManage, Target::Platform)?;
        let rec = self.get(id, version).await?;
        self.transition(actor, &rec, PluginState::Disabled, Some(&BTreeSet::new()))
            .await
    }

    pub async fn retire(
        &self,
        actor: &Principal,
        id: &PluginId,
        version: &semver::Version,
    ) -> Result<PluginRecord> {
        self.authz
            .authorize(actor, Action::PluginManage, Target::Platform)?;
        let rec = self.get(id, version).await?;
        self.transition(actor, &rec, PluginState::Retired, Some(&BTreeSet::new()))
            .await
    }

    /// Mark an enabled plugin as failed (health check / registration failure).
    pub async fn mark_failed(&self, id: &PluginId, version: &semver::Version, reason: &str) -> Result<()> {
        let rec = self.get(id, version).await?;
        if !rec.state.can_transition(PluginState::Failed) {
            return Ok(());
        }
        let mut tx = self.db.write().await?;
        sqlx::query("UPDATE plugins SET state = 'failed', granted = '[]', updated_at = ? WHERE id = ? AND version = ?")
            .bind(ts(self.db.now()))
            .bind(id.as_str())
            .bind(version.to_string())
            .execute(tx.conn())
            .await
            .db()?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::system(
                    "plugin-registry",
                    "plugin.failed",
                    "plugin",
                    format!("{id}@{version}"),
                    AuditOutcome::Failed,
                )
                .details(json!({"reason": reason})),
            )
            .await?;
        tx.commit().await
    }

    async fn transition(
        &self,
        actor: &Principal,
        rec: &PluginRecord,
        to: PluginState,
        granted: Option<&BTreeSet<PluginCapability>>,
    ) -> Result<PluginRecord> {
        if !rec.state.can_transition(to) {
            return Err(HarnessError::conflict(format!(
                "plugin cannot transition {:?} -> {:?}",
                rec.state, to
            )));
        }
        let mut tx = self.db.write().await?;
        let r = sqlx::query(
            "UPDATE plugins SET state = ?, granted = COALESCE(?, granted), updated_at = ? WHERE id = ? AND version = ? AND state = ?",
        )
        .bind(to.as_str())
        .bind(granted.map(to_json).transpose()?)
        .bind(ts(self.db.now()))
        .bind(rec.manifest.id.as_str())
        .bind(rec.manifest.version.to_string())
        .bind(rec.state.as_str())
        .execute(tx.conn())
        .await
        .db()?;
        if r.rows_affected() != 1 {
            return Err(HarnessError::conflict("plugin state changed concurrently"));
        }
        let pid = format!("{}@{}", rec.manifest.id, rec.manifest.version);
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    format!("plugin.{}", to.as_str()),
                    "plugin",
                    pid,
                    AuditOutcome::Succeeded,
                )
                .details(json!({"granted": granted})),
            )
            .await?;
        let ev = EventEnvelope::new(
            &PluginStateChanged {
                plugin_id: rec.manifest.id.to_string(),
                version: rec.manifest.version.to_string(),
                state: to.as_str().into(),
            },
            "plugin-registry",
            None,
            TraceContext::new_root(),
            self.db.now(),
        )?;
        outbox::enqueue_in(&mut tx, &ev).await?;
        tx.commit().await?;
        self.get(&rec.manifest.id, &rec.manifest.version).await
    }

    pub async fn get(&self, id: &PluginId, version: &semver::Version) -> Result<PluginRecord> {
        let row: Option<(String, String, Option<String>, String, String)> = sqlx::query_as(
            "SELECT manifest, digest, signed_by, state, granted FROM plugins WHERE id = ? AND version = ?",
        )
        .bind(id.as_str())
        .bind(version.to_string())
        .fetch_optional(self.db.pool())
        .await
        .db()?;
        let row =
            row.ok_or_else(|| HarnessError::not_found(format!("plugin {id}@{version} not registered")))?;
        to_record(row)
    }

    pub async fn list(&self) -> Result<Vec<PluginRecord>> {
        let rows: Vec<(String, String, Option<String>, String, String)> = sqlx::query_as(
            "SELECT manifest, digest, signed_by, state, granted FROM plugins ORDER BY id, version",
        )
        .fetch_all(self.db.pool())
        .await
        .db()?;
        rows.into_iter().map(to_record).collect()
    }
}

fn to_record(row: (String, String, Option<String>, String, String)) -> Result<PluginRecord> {
    Ok(PluginRecord {
        manifest: from_json(&row.0)?,
        digest: row.1,
        signed_by: row.2,
        state: PluginState::parse(&row.3)?,
        granted: from_json(&row.4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eaeh_core::{ErrorKind, PrincipalId, ProjectAccess, Role, TenantId};
    use eaeh_governance::authz::RbacAuthorizer;
    use eaeh_governance::policy::RuleEngine;
    use ed25519_dalek::{Signer, SigningKey};

    fn manifest(kind: PluginKind) -> PluginManifest {
        PluginManifest {
            id: PluginId::new("acme.audit-hooks").unwrap(),
            version: semver::Version::new(1, 2, 0),
            kind,
            description: "Adds audit hooks".into(),
            eaeh_api: semver::VersionReq::parse("^1").unwrap(),
            dependencies: vec![],
            capabilities: [PluginCapability::RegisterHooks, PluginCapability::RegisterTools].into(),
            config_schema: None,
            resource_limits: ResourceLimitsSpec {
                timeout_ms: 1000,
                memory_mb: None,
            },
            integrity: None,
        }
    }

    fn sign(m: &mut PluginManifest, key: &SigningKey, key_id: &str) {
        let digest = m.compute_digest().unwrap();
        let sig = key.sign(&digest);
        m.integrity = Some(Integrity {
            digest: to_hex(&digest),
            signature: Some(to_hex(&sig.to_bytes())),
            key_id: Some(key_id.into()),
        });
    }

    fn key() -> SigningKey {
        SigningKey::from_bytes(&eaeh_core::hash::random_bytes::<32>().unwrap())
    }

    fn trust(k: &SigningKey) -> TrustPolicy {
        TrustPolicy {
            require_signature: true,
            trusted_keys: [("release".to_string(), k.verifying_key().to_bytes())].into(),
            grantable: [PluginCapability::RegisterHooks].into(),
        }
    }

    #[test]
    fn signature_verification() {
        let k = key();
        let t = trust(&k);
        let mut m = manifest(PluginKind::InProcess);
        assert_eq!(
            verify_manifest(&m, &t).unwrap_err().kind,
            ErrorKind::Integrity,
            "unsigned rejected"
        );
        sign(&mut m, &k, "release");
        assert_eq!(
            verify_manifest(&m, &t).unwrap().signed_by.as_deref(),
            Some("release")
        );

        let mut tampered = m.clone();
        tampered
            .capabilities
            .insert(PluginCapability::RegisterModelProviders);
        assert!(
            verify_manifest(&tampered, &t)
                .unwrap_err()
                .message
                .contains("digest mismatch")
        );

        let mut other = manifest(PluginKind::InProcess);
        sign(&mut other, &key(), "release");
        assert!(
            verify_manifest(&other, &t)
                .unwrap_err()
                .message
                .contains("invalid manifest signature")
        );

        let mut unknown = manifest(PluginKind::InProcess);
        sign(&mut unknown, &k, "someone-else");
        assert!(
            verify_manifest(&unknown, &t)
                .unwrap_err()
                .message
                .contains("not trusted")
        );
    }

    #[tokio::test]
    async fn lifecycle_grants_and_phase1_kind_restriction() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("p.db")).await.unwrap();
        let k = key();
        let reg = PluginRegistry::new(
            db,
            Arc::new(RbacAuthorizer),
            Arc::new(RuleEngine::with_baseline().unwrap()),
            trust(&k),
        );
        let admin = Principal::human(
            PrincipalId::new("admin").unwrap(),
            TenantId::new(),
            [Role::Admin].into(),
            ProjectAccess::All,
        );
        let op = Principal::human(
            PrincipalId::new("op").unwrap(),
            admin.tenant,
            [Role::Operator].into(),
            ProjectAccess::All,
        );

        let mut m = manifest(PluginKind::InProcess);
        sign(&mut m, &k, "release");
        assert_eq!(
            reg.register(&op, m.clone()).await.unwrap_err().kind,
            ErrorKind::Forbidden
        );
        let r = reg.register(&admin, m.clone()).await.unwrap();
        assert_eq!(r.state, PluginState::Validated);
        assert!(
            reg.register(&admin, m.clone()).await.is_err(),
            "versions are immutable"
        );

        let r = reg.enable(&admin, &m.id, &m.version).await.unwrap();
        assert_eq!(r.state, PluginState::Enabled);
        assert_eq!(
            r.granted,
            [PluginCapability::RegisterHooks].into(),
            "only grantable capabilities"
        );
        let r = reg.disable(&admin, &m.id, &m.version).await.unwrap();
        assert!(r.granted.is_empty());
        reg.retire(&admin, &m.id, &m.version).await.unwrap();
        assert_eq!(
            reg.enable(&admin, &m.id, &m.version).await.unwrap_err().kind,
            ErrorKind::Conflict
        );

        let mut w = manifest(PluginKind::Wasm);
        w.id = PluginId::new("acme.wasm").unwrap();
        sign(&mut w, &k, "release");
        reg.register(&admin, w.clone()).await.unwrap();
        assert_eq!(
            reg.enable(&admin, &w.id, &w.version).await.unwrap_err().kind,
            ErrorKind::PolicyDenied
        );
    }
}
