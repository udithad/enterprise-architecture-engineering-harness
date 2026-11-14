use crate::ids::{ProjectId, TenantId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Tenant + project boundary. Every scoped entity (workflow, task, workspace, artifact,
/// approval, context item) carries a `Scope`; cross-scope access is denied by the
/// authorizer and by store queries (which always filter on both columns).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scope {
    pub tenant: TenantId,
    pub project: ProjectId,
}

impl Scope {
    pub fn new(tenant: TenantId, project: ProjectId) -> Self {
        Self { tenant, project }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.tenant, self.project)
    }
}
