//! Deterministic policy engine (ADR-0006, decision D-02).
//!
//! * Default deny: if no rule matches, the effect is `Deny`.
//! * Conflict resolution: `Deny` overrides `RequireApproval`, which overrides `Allow`.
//! * Inheritance: enterprise-level policies apply to every request; project-level policies
//!   apply only to requests in that project. Because deny/approval always dominate allow,
//!   a project policy can never weaken an enterprise rule.
//! * Every decision lists the matched rules (`policy@version#rule`) for decision records.

use eaeh_core::{AutonomyLevel, HarnessError, PolicyId, ProjectId, Result, RiskClass, Role, Scope};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "level")]
pub enum PolicyLevel {
    Enterprise,
    Project { project: ProjectId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "effect")]
pub enum RuleEffect {
    Allow,
    Deny,
    RequireApproval { role: Role },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
    ActionIn(Vec<String>),
    RiskAtLeast(RiskClass),
    RiskAtMost(RiskClass),
    PrincipalKindIn(Vec<String>),
    AutonomyAtLeast(AutonomyLevel),
    ResourceIdIn(Vec<String>),
    AttributeEquals { key: String, value: String },
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub description: String,
    #[serde(flatten)]
    pub effect: RuleEffect,
    pub when: Condition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDocument {
    pub id: PolicyId,
    pub version: semver::Version,
    pub description: String,
    #[serde(flatten)]
    pub level: PolicyLevel,
    pub rules: Vec<Rule>,
}

impl PolicyDocument {
    pub fn validate(&self) -> Result<()> {
        if self.rules.is_empty() {
            return Err(HarnessError::invalid_input(format!(
                "policy {} has no rules",
                self.id
            )));
        }
        let mut seen = std::collections::BTreeSet::new();
        for r in &self.rules {
            if r.id.is_empty() || !seen.insert(&r.id) {
                return Err(HarnessError::invalid_input(format!(
                    "policy {}: empty or duplicate rule id '{}'",
                    self.id, r.id
                )));
            }
        }
        Ok(())
    }

    fn applies_to(&self, req: &PolicyRequest) -> bool {
        match &self.level {
            PolicyLevel::Enterprise => true,
            PolicyLevel::Project { project } => req.scope.map(|s| s.project == *project).unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRequest {
    pub action: String,
    pub principal_id: String,
    pub principal_kind: String,
    pub autonomy: Option<AutonomyLevel>,
    pub risk: Option<RiskClass>,
    pub resource_type: String,
    pub resource_id: String,
    pub scope: Option<Scope>,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "effect")]
pub enum Effect {
    Permit,
    Deny,
    RequireApproval { role: Role },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub effect: Effect,
    pub matched_rules: Vec<String>,
    pub reason: String,
}

impl PolicyDecision {
    pub fn effect_str(&self) -> &'static str {
        match self.effect {
            Effect::Permit => "permit",
            Effect::Deny => "deny",
            Effect::RequireApproval { .. } => "require_approval",
        }
    }
}

pub trait PolicyEngine: Send + Sync + std::fmt::Debug {
    /// Pure, deterministic evaluation. No I/O.
    fn evaluate(&self, req: &PolicyRequest) -> PolicyDecision;
}

fn matches(c: &Condition, r: &PolicyRequest) -> bool {
    match c {
        Condition::All(cs) => cs.iter().all(|c| matches(c, r)),
        Condition::Any(cs) => cs.iter().any(|c| matches(c, r)),
        Condition::Not(c) => !matches(c, r),
        Condition::ActionIn(a) => a.iter().any(|x| x == &r.action),
        // Missing risk never satisfies a risk condition (fail closed for Allow rules;
        // Deny rules should be written with explicit conditions).
        Condition::RiskAtLeast(min) => r.risk.map(|x| x >= *min).unwrap_or(false),
        Condition::RiskAtMost(max) => r.risk.map(|x| x <= *max).unwrap_or(false),
        Condition::PrincipalKindIn(k) => k.iter().any(|x| x == &r.principal_kind),
        Condition::AutonomyAtLeast(min) => r.autonomy.map(|a| a >= *min).unwrap_or(false),
        Condition::ResourceIdIn(ids) => ids.iter().any(|x| x == &r.resource_id),
        Condition::AttributeEquals { key, value } => r.attributes.get(key) == Some(value),
        Condition::Always => true,
    }
}

/// In-memory rule engine over a set of active policy documents.
#[derive(Debug, Default)]
pub struct RuleEngine {
    docs: RwLock<Vec<PolicyDocument>>,
}

impl RuleEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_baseline() -> Result<Self> {
        let e = Self::new();
        e.load(baseline_policy()?)?;
        Ok(e)
    }

    /// Load (or replace by id) a policy document after validation.
    pub fn load(&self, doc: PolicyDocument) -> Result<()> {
        doc.validate()?;
        let mut docs = self
            .docs
            .write()
            .map_err(|_| HarnessError::internal("policy lock poisoned"))?;
        docs.retain(|d| d.id != doc.id);
        docs.push(doc);
        docs.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(())
    }

    pub fn documents(&self) -> Vec<PolicyDocument> {
        self.docs.read().map(|d| d.clone()).unwrap_or_default()
    }
}

impl PolicyEngine for RuleEngine {
    fn evaluate(&self, req: &PolicyRequest) -> PolicyDecision {
        let Ok(docs) = self.docs.read() else {
            return PolicyDecision {
                effect: Effect::Deny,
                matched_rules: vec![],
                reason: "policy store unavailable".into(),
            };
        };
        let mut deny = vec![];
        let mut approval: Vec<(String, Role)> = vec![];
        let mut allow = vec![];
        for d in docs.iter().filter(|d| d.applies_to(req)) {
            for rule in &d.rules {
                if matches(&rule.when, req) {
                    let tag = format!("{}@{}#{}", d.id, d.version, rule.id);
                    match &rule.effect {
                        RuleEffect::Deny => deny.push(tag),
                        RuleEffect::RequireApproval { role } => approval.push((tag, *role)),
                        RuleEffect::Allow => allow.push(tag),
                    }
                }
            }
        }
        if !deny.is_empty() {
            return PolicyDecision {
                effect: Effect::Deny,
                reason: "denied by rule".into(),
                matched_rules: deny,
            };
        }
        if let Some((_, role)) = approval.first() {
            let role = *role;
            return PolicyDecision {
                effect: Effect::RequireApproval { role },
                reason: "human approval required".into(),
                matched_rules: approval.into_iter().map(|(t, _)| t).collect(),
            };
        }
        if !allow.is_empty() {
            return PolicyDecision {
                effect: Effect::Permit,
                reason: "allowed by rule".into(),
                matched_rules: allow,
            };
        }
        PolicyDecision {
            effect: Effect::Deny,
            reason: "no matching allow rule (default deny)".into(),
            matched_rules: vec![],
        }
    }
}

/// Persist a policy document (immutable per version) and audit the change.
pub async fn save_policy(
    db: &eaeh_store::Db,
    actor: &eaeh_core::Principal,
    doc: &PolicyDocument,
) -> Result<()> {
    use eaeh_store::DbResultExt;
    doc.validate()?;
    let mut tx = db.write().await?;
    let level = match &doc.level {
        PolicyLevel::Enterprise => "enterprise".to_string(),
        PolicyLevel::Project { project } => format!("project:{project}"),
    };
    eaeh_store::sqlx::query("UPDATE policies SET status = 'retired' WHERE id = ? AND status = 'active'")
        .bind(doc.id.as_str())
        .execute(tx.conn())
        .await
        .db()?;
    eaeh_store::sqlx::query(
        "INSERT INTO policies (id, version, level, document, status, created_at) VALUES (?, ?, ?, ?, 'active', ?)",
    )
    .bind(doc.id.as_str())
    .bind(doc.version.to_string())
    .bind(level)
    .bind(eaeh_store::to_json(doc)?)
    .bind(eaeh_store::ts(db.now()))
    .execute(tx.conn())
    .await
    .db()?;
    db.audit_in(
        &mut tx,
        eaeh_store::audit::AuditEntry::new(
            actor,
            "policy.save",
            "policy",
            format!("{}@{}", doc.id, doc.version),
            eaeh_store::audit::AuditOutcome::Succeeded,
        )
        .details(serde_json::to_value(doc)?),
    )
    .await?;
    tx.commit().await
}

/// Load all active persisted policy documents.
pub async fn load_active_policies(db: &eaeh_store::Db) -> Result<Vec<PolicyDocument>> {
    use eaeh_store::DbResultExt;
    let rows: Vec<(String,)> =
        eaeh_store::sqlx::query_as("SELECT document FROM policies WHERE status = 'active' ORDER BY id")
            .fetch_all(db.pool())
            .await
            .db()?;
    rows.iter().map(|(d,)| eaeh_store::from_json(d)).collect()
}

/// Structural platform baseline. This is **not** an enterprise standard (none are invented,
/// see R-07): it only encodes the safety defaults approved in the Phase 1 assessment.
pub fn baseline_policy() -> Result<PolicyDocument> {
    let json = serde_json::json!({
        "id": "eaeh.baseline",
        "version": "1.0.0",
        "description": "Platform safety baseline: low-risk tools allowed, infrastructure/deployment require approval, everything else denied by default.",
        "level": "enterprise",
        "rules": [
            {
                "id": "tool-low-risk-allow",
                "description": "Read-only, workspace-write and build/test tools are permitted (autonomy is enforced separately by the gateway).",
                "effect": "allow",
                "when": {"all": [{"action_in": ["tool.invoke"]}, {"risk_at_most": "build_test"}]}
            },
            {
                "id": "tool-infra-deploy-approval",
                "description": "Cloud infrastructure and deployment tools always require a human approver.",
                "effect": "require_approval", "role": "approver",
                "when": {"all": [{"action_in": ["tool.invoke"]}, {"risk_at_least": "cloud_infra"}]}
            },
            {
                "id": "plan-step-allow",
                "description": "Planner-proposed steps up to build/test risk may be scheduled.",
                "effect": "allow",
                "when": {"all": [{"action_in": ["workflow.plan_step"]}, {"risk_at_most": "build_test"}]}
            },
            {
                "id": "model-invoke-allow",
                "description": "Model calls are permitted unless a more specific rule denies them (Phase 2 egress governance).",
                "effect": "allow",
                "when": {"action_in": ["model.invoke"]}
            },
            {
                "id": "model-restricted-egress-deny",
                "description": "Content classified 'restricted' is never sent to an external model provider (P2-D5).",
                "effect": "deny",
                "when": {"all": [{"action_in": ["model.invoke"]}, {"attribute_equals": {"key": "egress", "value": "external"}}, {"attribute_equals": {"key": "classification", "value": "restricted"}}]}
            },
            {
                "id": "plugin-in-process-allow",
                "description": "Only trusted in-process extensions may be enabled in Phase 1 (D-03).",
                "effect": "allow",
                "when": {"all": [{"action_in": ["plugin.enable"]}, {"attribute_equals": {"key": "plugin_kind", "value": "in_process"}}]}
            },
            {
                "id": "plugin-other-kinds-deny",
                "description": "WASM and external-process plugin execution hosts are deferred (D-03).",
                "effect": "deny",
                "when": {"all": [{"action_in": ["plugin.enable"]}, {"not": {"attribute_equals": {"key": "plugin_kind", "value": "in_process"}}}]}
            }
        ]
    });
    serde_json::from_value(json).map_err(|e| HarnessError::internal(format!("baseline policy invalid: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eaeh_core::TenantId;

    fn req(action: &str, risk: Option<RiskClass>) -> PolicyRequest {
        PolicyRequest {
            action: action.into(),
            principal_id: "agent:x".into(),
            principal_kind: "agent".into(),
            autonomy: Some(AutonomyLevel::ExternalSideEffects),
            risk,
            resource_type: "tool".into(),
            resource_id: "t".into(),
            scope: None,
            attributes: BTreeMap::new(),
        }
    }

    #[test]
    fn baseline_decisions() {
        let e = RuleEngine::with_baseline().unwrap();
        assert_eq!(
            e.evaluate(&req("tool.invoke", Some(RiskClass::ReadOnly))).effect,
            Effect::Permit
        );
        assert_eq!(
            e.evaluate(&req("tool.invoke", Some(RiskClass::BuildTest))).effect,
            Effect::Permit
        );
        assert_eq!(
            e.evaluate(&req("tool.invoke", Some(RiskClass::ExternalApi)))
                .effect,
            Effect::Deny
        );
        assert_eq!(
            e.evaluate(&req("tool.invoke", Some(RiskClass::Git))).effect,
            Effect::Deny
        );
        assert_eq!(
            e.evaluate(&req("tool.invoke", Some(RiskClass::Deployment)))
                .effect,
            Effect::RequireApproval { role: Role::Approver }
        );
        assert_eq!(e.evaluate(&req("tool.invoke", None)).effect, Effect::Deny);
        assert_eq!(
            e.evaluate(&req("unknown.action", Some(RiskClass::ReadOnly)))
                .effect,
            Effect::Deny
        );
    }

    #[test]
    fn project_policy_cannot_override_enterprise_deny() {
        let e = RuleEngine::with_baseline().unwrap();
        let project = ProjectId::new();
        let mut enterprise_deny = baseline_policy().unwrap();
        enterprise_deny.id = PolicyId::new("ent.deny-fs-write").unwrap();
        enterprise_deny.rules = vec![Rule {
            id: "deny-x".into(),
            description: "".into(),
            effect: RuleEffect::Deny,
            when: Condition::ResourceIdIn(vec!["t".into()]),
        }];
        e.load(enterprise_deny).unwrap();
        let project_allow = PolicyDocument {
            id: PolicyId::new("proj.allow-all").unwrap(),
            version: semver::Version::new(1, 0, 0),
            description: "".into(),
            level: PolicyLevel::Project { project },
            rules: vec![Rule {
                id: "a".into(),
                description: "".into(),
                effect: RuleEffect::Allow,
                when: Condition::Always,
            }],
        };
        e.load(project_allow).unwrap();
        let mut r = req("tool.invoke", Some(RiskClass::ReadOnly));
        r.scope = Some(Scope::new(TenantId::new(), project));
        let d = e.evaluate(&r);
        assert_eq!(d.effect, Effect::Deny);
        assert!(d.matched_rules.iter().any(|m| m.starts_with("ent.deny-fs-write")));

        // Project-level allow applies only in its project (ExternalApi denied elsewhere).
        let mut other = req("tool.invoke", Some(RiskClass::ExternalApi));
        other.scope = Some(Scope::new(TenantId::new(), ProjectId::new()));
        assert_eq!(e.evaluate(&other).effect, Effect::Deny);
        let mut inproj = req("tool.invoke", Some(RiskClass::ExternalApi));
        inproj.resource_id = "other-tool".into();
        inproj.scope = Some(Scope::new(TenantId::new(), project));
        assert_eq!(e.evaluate(&inproj).effect, Effect::Permit);
    }

    #[test]
    fn restricted_content_never_egresses_to_external_models() {
        let e = RuleEngine::with_baseline().unwrap();
        let mut r = req("model.invoke", None);
        for (egress, class, effect) in [
            ("external", "internal", Effect::Permit),
            ("external", "confidential", Effect::Permit),
            ("external", "restricted", Effect::Deny),
            ("local", "restricted", Effect::Permit),
        ] {
            r.attributes = [
                ("egress".to_string(), egress.to_string()),
                ("classification".to_string(), class.to_string()),
            ]
            .into();
            assert_eq!(e.evaluate(&r).effect, effect, "{egress}/{class}");
        }
    }

    #[test]
    fn evaluation_is_deterministic() {
        let e = RuleEngine::with_baseline().unwrap();
        let r = req("tool.invoke", Some(RiskClass::CloudInfra));
        let first = e.evaluate(&r);
        for _ in 0..50 {
            assert_eq!(e.evaluate(&r), first);
        }
    }

    #[test]
    fn invalid_documents_rejected() {
        let mut d = baseline_policy().unwrap();
        d.rules.push(d.rules[0].clone());
        assert!(RuleEngine::new().load(d).is_err());
    }
}
