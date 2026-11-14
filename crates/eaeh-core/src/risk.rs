use serde::{Deserialize, Serialize};
use std::fmt;

/// Side-effect / risk classification of operations (primarily tools). Ordered from least
/// to most consequential; comparisons (`<=`) are used by autonomy and policy checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    ReadOnly,
    WorkspaceWrite,
    BuildTest,
    Git,
    ExternalApi,
    CloudInfra,
    Deployment,
}

impl RiskClass {
    pub const ALL: [RiskClass; 7] = [
        RiskClass::ReadOnly,
        RiskClass::WorkspaceWrite,
        RiskClass::BuildTest,
        RiskClass::Git,
        RiskClass::ExternalApi,
        RiskClass::CloudInfra,
        RiskClass::Deployment,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            RiskClass::ReadOnly => "read_only",
            RiskClass::WorkspaceWrite => "workspace_write",
            RiskClass::BuildTest => "build_test",
            RiskClass::Git => "git",
            RiskClass::ExternalApi => "external_api",
            RiskClass::CloudInfra => "cloud_infra",
            RiskClass::Deployment => "deployment",
        }
    }
}

impl fmt::Display for RiskClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Agent autonomy levels (master instructions §10). Enforced deterministically by the
/// tool gateway through [`AutonomyLevel::max_risk`], never by prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    /// L0: read-only analysis.
    ReadOnly,
    /// L1: recommendations and proposed actions (results are returned, not applied).
    Recommend,
    /// L2: modifications within an isolated workspace.
    WorkspaceModify,
    /// L3: execution of authorized tools and workflows (build/test).
    ExecuteAuthorized,
    /// L4: authorized external side effects (still subject to policy and approvals).
    ExternalSideEffects,
}

impl AutonomyLevel {
    /// Highest tool risk class an agent at this level may ever invoke.
    pub fn max_risk(self) -> RiskClass {
        match self {
            AutonomyLevel::ReadOnly | AutonomyLevel::Recommend => RiskClass::ReadOnly,
            AutonomyLevel::WorkspaceModify => RiskClass::WorkspaceWrite,
            AutonomyLevel::ExecuteAuthorized => RiskClass::BuildTest,
            AutonomyLevel::ExternalSideEffects => RiskClass::Deployment,
        }
    }

    pub fn permits(self, risk: RiskClass) -> bool {
        risk <= self.max_risk()
    }

    pub fn level(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autonomy_maps_to_monotonic_risk_ceiling() {
        assert!(AutonomyLevel::ReadOnly.permits(RiskClass::ReadOnly));
        assert!(!AutonomyLevel::ReadOnly.permits(RiskClass::WorkspaceWrite));
        assert!(!AutonomyLevel::Recommend.permits(RiskClass::WorkspaceWrite));
        assert!(AutonomyLevel::WorkspaceModify.permits(RiskClass::WorkspaceWrite));
        assert!(!AutonomyLevel::WorkspaceModify.permits(RiskClass::BuildTest));
        assert!(AutonomyLevel::ExecuteAuthorized.permits(RiskClass::BuildTest));
        assert!(!AutonomyLevel::ExecuteAuthorized.permits(RiskClass::Git));
        assert!(AutonomyLevel::ExternalSideEffects.permits(RiskClass::Deployment));
        let levels = [
            AutonomyLevel::ReadOnly,
            AutonomyLevel::Recommend,
            AutonomyLevel::WorkspaceModify,
            AutonomyLevel::ExecuteAuthorized,
            AutonomyLevel::ExternalSideEffects,
        ];
        for w in levels.windows(2) {
            assert!(w[0].max_risk() <= w[1].max_risk());
        }
    }
}
