//! Identities. Humans, services and agent instances are all principals; the authorizer
//! decides on the basis of kind, roles and scope. Agent principals are structurally
//! unable to hold roles (see [`Principal::agent`]), which makes agent self-approval
//! impossible rather than merely disallowed by a rule.

use crate::ids::{AgentDefinitionId, AgentInstanceId, PrincipalId, ProjectId, TenantId};
use crate::risk::AutonomyLevel;
use crate::scope::Scope;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Platform administration: principals, tokens, agent/tool/plugin/policy registration.
    Admin,
    /// Starts and operates workflows.
    Operator,
    /// May decide on human approval gates.
    Approver,
    /// Read-only access.
    Viewer,
    /// May read the audit trail.
    Auditor,
    /// Phase 2: curates enterprise knowledge sources/documents and registers skills.
    /// Cannot approve (standards and skills require an Approver; separation of duties).
    KnowledgeSteward,
}

impl Role {
    pub const ALL: [Role; 6] = [
        Role::Admin,
        Role::Operator,
        Role::Approver,
        Role::Viewer,
        Role::Auditor,
        Role::KnowledgeSteward,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Operator => "operator",
            Role::Approver => "approver",
            Role::Viewer => "viewer",
            Role::Auditor => "auditor",
            Role::KnowledgeSteward => "knowledge_steward",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "ids")]
pub enum ProjectAccess {
    /// All projects within the principal's tenant.
    All,
    Only(BTreeSet<ProjectId>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum PrincipalKind {
    Human,
    Service,
    Agent {
        instance: AgentInstanceId,
        definition: AgentDefinitionId,
        definition_version: String,
        autonomy: AutonomyLevel,
        /// The agent instance is bound to exactly one project.
        project: ProjectId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub tenant: TenantId,
    roles: BTreeSet<Role>,
    pub projects: ProjectAccess,
}

impl Principal {
    pub fn human(id: PrincipalId, tenant: TenantId, roles: BTreeSet<Role>, projects: ProjectAccess) -> Self {
        Self {
            id,
            kind: PrincipalKind::Human,
            tenant,
            roles,
            projects,
        }
    }

    pub fn service(
        id: PrincipalId,
        tenant: TenantId,
        roles: BTreeSet<Role>,
        projects: ProjectAccess,
    ) -> Self {
        Self {
            id,
            kind: PrincipalKind::Service,
            tenant,
            roles,
            projects,
        }
    }

    /// Agent principals never carry roles and are confined to a single project.
    pub fn agent(
        instance: AgentInstanceId,
        definition: AgentDefinitionId,
        definition_version: String,
        autonomy: AutonomyLevel,
        scope: Scope,
    ) -> Self {
        Self {
            id: PrincipalId::for_agent_instance(instance),
            kind: PrincipalKind::Agent {
                instance,
                definition,
                definition_version,
                autonomy,
                project: scope.project,
            },
            tenant: scope.tenant,
            roles: BTreeSet::new(),
            projects: ProjectAccess::Only([scope.project].into_iter().collect()),
        }
    }

    pub fn roles(&self) -> &BTreeSet<Role> {
        &self.roles
    }

    /// Agents never hold roles, even if a deserialized value claims otherwise.
    pub fn has_role(&self, role: Role) -> bool {
        !self.is_agent() && self.roles.contains(&role)
    }

    pub fn is_agent(&self) -> bool {
        matches!(self.kind, PrincipalKind::Agent { .. })
    }

    pub fn is_human(&self) -> bool {
        matches!(self.kind, PrincipalKind::Human)
    }

    pub fn autonomy(&self) -> Option<AutonomyLevel> {
        match &self.kind {
            PrincipalKind::Agent { autonomy, .. } => Some(*autonomy),
            _ => None,
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match self.kind {
            PrincipalKind::Human => "human",
            PrincipalKind::Service => "service",
            PrincipalKind::Agent { .. } => "agent",
        }
    }

    /// Whether this principal may act within `scope` (tenant and project boundary).
    pub fn can_access(&self, scope: &Scope) -> bool {
        if self.tenant != scope.tenant {
            return false;
        }
        match &self.projects {
            ProjectAccess::All => true,
            ProjectAccess::Only(set) => set.contains(&scope.project),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_principals_have_no_roles_and_single_project() {
        let scope = Scope::new(TenantId::new(), ProjectId::new());
        let p = Principal::agent(
            AgentInstanceId::new(),
            AgentDefinitionId::new("dev").unwrap(),
            "1.0.0".into(),
            AutonomyLevel::WorkspaceModify,
            scope,
        );
        assert!(p.roles().is_empty());
        assert!(!p.has_role(Role::Approver));
        assert!(p.can_access(&scope));
        assert!(!p.can_access(&Scope::new(scope.tenant, ProjectId::new())));
        assert!(!p.can_access(&Scope::new(TenantId::new(), scope.project)));
    }

    #[test]
    fn tenant_boundary_applies_even_with_all_projects() {
        let t = TenantId::new();
        let p = Principal::human(
            PrincipalId::new("alice").unwrap(),
            t,
            BTreeSet::new(),
            ProjectAccess::All,
        );
        assert!(p.can_access(&Scope::new(t, ProjectId::new())));
        assert!(!p.can_access(&Scope::new(TenantId::new(), ProjectId::new())));
    }
}
