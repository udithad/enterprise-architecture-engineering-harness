//! Role- and scope-based authorization (RBAC + tenant/project ABAC).
//!
//! Authorization is a pure function. Callers (API handlers, tool gateway, workflow
//! engine, approval manager) invoke it at their trust boundary and audit denials.

use eaeh_core::{HarnessError, Principal, PrincipalKind, Result, Role, Scope, TenantId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    WorkflowStart,
    WorkflowRead,
    WorkflowCancel,
    WorkflowDefine,
    ApprovalRead,
    ApprovalDecide,
    AgentRegister,
    AgentRead,
    AgentDelegate,
    ToolRead,
    ToolInvoke,
    PluginManage,
    PolicyManage,
    PromptManage,
    PrincipalManage,
    TokenManageOwn,
    TokenManageAny,
    AuditRead,
    ArtifactRead,
    MetricsRead,
    KnowledgeManage,
    KnowledgeRead,
    SkillManage,
    SkillRead,
    /// Work on an agent task handed off to an external AI client: read its package, call
    /// the agent's allowed tools for it, submit its result (`external_ai` mode, ADR-0025).
    TaskExecuteExternal,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::WorkflowStart => "workflow.start",
            Action::WorkflowRead => "workflow.read",
            Action::WorkflowCancel => "workflow.cancel",
            Action::WorkflowDefine => "workflow.define",
            Action::ApprovalRead => "approval.read",
            Action::ApprovalDecide => "approval.decide",
            Action::AgentRegister => "agent.register",
            Action::AgentRead => "agent.read",
            Action::AgentDelegate => "agent.delegate",
            Action::ToolRead => "tool.read",
            Action::ToolInvoke => "tool.invoke",
            Action::PluginManage => "plugin.manage",
            Action::PolicyManage => "policy.manage",
            Action::PromptManage => "prompt.manage",
            Action::PrincipalManage => "principal.manage",
            Action::TokenManageOwn => "token.manage_own",
            Action::TokenManageAny => "token.manage_any",
            Action::AuditRead => "audit.read",
            Action::ArtifactRead => "artifact.read",
            Action::MetricsRead => "metrics.read",
            Action::KnowledgeManage => "knowledge.manage",
            Action::KnowledgeRead => "knowledge.read",
            Action::SkillManage => "skill.manage",
            Action::SkillRead => "skill.read",
            Action::TaskExecuteExternal => "task.execute_external",
        }
    }
}

/// What the action targets, for boundary checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Platform-wide registries (agent/tool/plugin/policy/prompt definitions).
    Platform,
    Tenant(TenantId),
    Scope(Scope),
}

pub trait Authorizer: Send + Sync + std::fmt::Debug {
    fn authorize(&self, principal: &Principal, action: Action, target: Target) -> Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RbacAuthorizer;

impl RbacAuthorizer {
    pub fn role_permits(role: Role, action: Action) -> bool {
        use Action::*;
        match role {
            Role::Admin => matches!(
                action,
                AgentRegister
                    | AgentRead
                    | ToolRead
                    | PluginManage
                    | PolicyManage
                    | PromptManage
                    | WorkflowDefine
                    | PrincipalManage
                    | TokenManageAny
                    | TokenManageOwn
                    | MetricsRead
                    | WorkflowRead
                    | ApprovalRead
                    | ArtifactRead
                    | AuditRead
                    | KnowledgeManage
                    | KnowledgeRead
                    | SkillManage
                    | SkillRead
            ),
            Role::Operator => matches!(
                action,
                TaskExecuteExternal
                    | WorkflowStart
                    | WorkflowRead
                    | WorkflowCancel
                    | ApprovalRead
                    | AgentRead
                    | ToolRead
                    | ToolInvoke
                    | ArtifactRead
                    | TokenManageOwn
                    | MetricsRead
                    | KnowledgeRead
                    | SkillRead
            ),
            Role::Approver => {
                matches!(
                    action,
                    ApprovalRead
                        | ApprovalDecide
                        | WorkflowRead
                        | ArtifactRead
                        | TokenManageOwn
                        | KnowledgeRead
                        | SkillRead
                )
            }
            Role::Viewer => {
                matches!(
                    action,
                    WorkflowRead
                        | ApprovalRead
                        | AgentRead
                        | ToolRead
                        | ArtifactRead
                        | TokenManageOwn
                        | KnowledgeRead
                        | SkillRead
                )
            }
            Role::Auditor => matches!(action, AuditRead | WorkflowRead | ApprovalRead | TokenManageOwn),
            Role::KnowledgeSteward => matches!(
                action,
                KnowledgeManage | KnowledgeRead | SkillManage | SkillRead | ApprovalRead | TokenManageOwn
            ),
        }
    }

    /// Fixed capability set of agent principals. Agents can never decide approvals,
    /// manage identities, policies, plugins or definitions.
    fn agent_permits(action: Action) -> bool {
        matches!(
            action,
            Action::ToolInvoke | Action::ArtifactRead | Action::AgentDelegate | Action::KnowledgeRead
        )
    }
}

impl Authorizer for RbacAuthorizer {
    fn authorize(&self, principal: &Principal, action: Action, target: Target) -> Result<()> {
        // 1. Boundary check.
        let in_boundary = match target {
            Target::Platform => !principal.is_agent(),
            Target::Tenant(t) => principal.tenant == t && !principal.is_agent(),
            Target::Scope(s) => principal.can_access(&s),
        };
        if !in_boundary {
            return Err(HarnessError::forbidden(format!(
                "principal '{}' may not access the target of {}",
                principal.id,
                action.as_str()
            )));
        }
        // 2. Capability check.
        let permitted = match &principal.kind {
            PrincipalKind::Agent { .. } => Self::agent_permits(action),
            PrincipalKind::Human | PrincipalKind::Service => {
                // Approval decisions are reserved for human principals.
                if action == Action::ApprovalDecide && !principal.is_human() {
                    false
                } else {
                    principal.roles().iter().any(|r| Self::role_permits(*r, action))
                }
            }
        };
        if permitted {
            Ok(())
        } else {
            let granting: Vec<&str> = Role::ALL
                .iter()
                .filter(|r| Self::role_permits(**r, action))
                .map(|r| r.as_str())
                .collect();
            Err(HarnessError::forbidden(format!(
                "principal '{}' ({}) lacks permission {}; roles that grant it: {}",
                principal.id,
                principal.kind_str(),
                action.as_str(),
                if granting.is_empty() {
                    "none (agents only)".to_string()
                } else {
                    granting.join(", ")
                }
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eaeh_core::{
        AgentDefinitionId, AgentInstanceId, AutonomyLevel, PrincipalId, ProjectAccess, ProjectId,
    };

    fn human(roles: &[Role], tenant: TenantId) -> Principal {
        Principal::human(
            PrincipalId::new("h").unwrap(),
            tenant,
            roles.iter().copied().collect(),
            ProjectAccess::All,
        )
    }

    #[test]
    fn only_human_approvers_can_decide() {
        let t = TenantId::new();
        let s = Scope::new(t, ProjectId::new());
        let a = RbacAuthorizer;
        assert!(
            a.authorize(
                &human(&[Role::Approver], t),
                Action::ApprovalDecide,
                Target::Scope(s)
            )
            .is_ok()
        );
        assert!(
            a.authorize(
                &human(&[Role::Admin, Role::Operator], t),
                Action::ApprovalDecide,
                Target::Scope(s)
            )
            .is_err()
        );
        let svc = Principal::service(
            PrincipalId::new("svc").unwrap(),
            t,
            [Role::Approver].into(),
            ProjectAccess::All,
        );
        assert!(
            a.authorize(&svc, Action::ApprovalDecide, Target::Scope(s))
                .is_err()
        );
        let agent = Principal::agent(
            AgentInstanceId::new(),
            AgentDefinitionId::new("x").unwrap(),
            "1.0.0".into(),
            AutonomyLevel::ExternalSideEffects,
            s,
        );
        assert!(
            a.authorize(&agent, Action::ApprovalDecide, Target::Scope(s))
                .is_err()
        );
        assert!(a.authorize(&agent, Action::ToolInvoke, Target::Scope(s)).is_ok());
        assert!(
            a.authorize(&agent, Action::AgentRegister, Target::Platform)
                .is_err()
        );
    }

    #[test]
    fn knowledge_steward_cannot_approve_and_agents_only_read_knowledge() {
        let t = TenantId::new();
        let s = Scope::new(t, ProjectId::new());
        let a = RbacAuthorizer;
        let steward = human(&[Role::KnowledgeSteward], t);
        assert!(
            a.authorize(&steward, Action::KnowledgeManage, Target::Tenant(t))
                .is_ok()
        );
        assert!(
            a.authorize(&steward, Action::SkillManage, Target::Platform)
                .is_ok()
        );
        assert!(
            a.authorize(&steward, Action::ApprovalDecide, Target::Scope(s))
                .is_err()
        );
        let viewer = human(&[Role::Viewer], t);
        assert!(
            a.authorize(&viewer, Action::KnowledgeRead, Target::Scope(s))
                .is_ok()
        );
        assert!(
            a.authorize(&viewer, Action::KnowledgeManage, Target::Tenant(t))
                .is_err()
        );
        let agent = Principal::agent(
            AgentInstanceId::new(),
            AgentDefinitionId::new("x").unwrap(),
            "1.0.0".into(),
            AutonomyLevel::ReadOnly,
            s,
        );
        assert!(
            a.authorize(&agent, Action::KnowledgeRead, Target::Scope(s))
                .is_ok()
        );
        assert!(
            a.authorize(&agent, Action::KnowledgeManage, Target::Scope(s))
                .is_err()
        );
        assert!(
            a.authorize(&agent, Action::SkillManage, Target::Platform)
                .is_err()
        );
    }

    #[test]
    fn cross_tenant_and_cross_project_denied() {
        let t = TenantId::new();
        let p1 = ProjectId::new();
        let p2 = ProjectId::new();
        let op = Principal::human(
            PrincipalId::new("op").unwrap(),
            t,
            [Role::Operator].into(),
            ProjectAccess::Only([p1].into()),
        );
        let a = RbacAuthorizer;
        assert!(
            a.authorize(&op, Action::WorkflowStart, Target::Scope(Scope::new(t, p1)))
                .is_ok()
        );
        assert!(
            a.authorize(&op, Action::WorkflowStart, Target::Scope(Scope::new(t, p2)))
                .is_err()
        );
        assert!(
            a.authorize(
                &op,
                Action::WorkflowStart,
                Target::Scope(Scope::new(TenantId::new(), p1))
            )
            .is_err()
        );
        let admin = human(&[Role::Admin], t);
        assert!(
            a.authorize(&admin, Action::PrincipalManage, Target::Tenant(TenantId::new()))
                .is_err()
        );
    }
}
