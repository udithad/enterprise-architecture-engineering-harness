//! Typed identifiers.
//!
//! Instance identifiers are UUIDv7 newtypes (time-ordered, unguessable enough for
//! correlation; never used as secrets). Definition identifiers (agents, tools, plugins,
//! policies, prompts, workflows) are validated slugs so they are stable and human readable.

use crate::error::{HarnessError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! uuid_id {
    ($($(#[$m:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$m])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub Uuid);

            impl $name {
                pub fn new() -> Self { Self(Uuid::now_v7()) }
                pub fn as_uuid(&self) -> Uuid { self.0 }
            }

            impl Default for $name {
                fn default() -> Self { Self::new() }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
            }

            impl FromStr for $name {
                type Err = HarnessError;
                fn from_str(s: &str) -> Result<Self> {
                    Uuid::parse_str(s)
                        .map(Self)
                        .map_err(|e| HarnessError::invalid_input(format!("invalid {}: {e}", stringify!($name))))
                }
            }
        )*
    };
}

uuid_id! {
    TenantId,
    ProjectId,
    WorkflowInstanceId,
    TaskId,
    AgentInstanceId,
    ApprovalId,
    ArtifactId,
    WorkspaceId,
    EventId,
    /// Groups every operation belonging to one logical request / workflow run.
    CorrelationId,
    AuditId,
    TokenId,
    InvocationId,
    KnowledgeSourceId,
    KnowledgeDocumentId,
}

macro_rules! slug_id {
    ($($(#[$m:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$m])*
            #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
            #[serde(try_from = "String", into = "String")]
            pub struct $name(String);

            impl $name {
                pub fn new(value: impl Into<String>) -> Result<Self> {
                    let value = value.into();
                    validate_slug(&value).map_err(|m| {
                        HarnessError::invalid_input(format!("invalid {} '{value}': {m}", stringify!($name)))
                    })?;
                    Ok(Self(value))
                }
                pub fn as_str(&self) -> &str { &self.0 }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
            }

            impl TryFrom<String> for $name {
                type Error = HarnessError;
                fn try_from(v: String) -> Result<Self> { Self::new(v) }
            }

            impl From<$name> for String {
                fn from(v: $name) -> String { v.0 }
            }

            impl FromStr for $name {
                type Err = HarnessError;
                fn from_str(s: &str) -> Result<Self> { Self::new(s) }
            }
        )*
    };
}

slug_id! {
    AgentDefinitionId,
    ToolId,
    PluginId,
    PolicyId,
    PromptId,
    WorkflowDefinitionId,
    ProviderId,
    ModelId,
    /// Stable identifier of a principal (human, service or agent instance).
    PrincipalId,
    SkillId,
}

impl PrincipalId {
    /// Principal id of an agent instance: `agent:<uuid>` (always a valid slug).
    pub fn for_agent_instance(instance: AgentInstanceId) -> Self {
        Self(format!("agent:{instance}"))
    }
}

/// Slug rule: 1..=128 chars, lowercase ASCII alphanumerics plus `.`, `-`, `_`, `:`;
/// must start with an alphanumeric. Excludes path separators and whitespace by design.
pub fn validate_slug(value: &str) -> std::result::Result<(), &'static str> {
    if value.is_empty() || value.len() > 128 {
        return Err("length must be 1..=128");
    }
    let mut chars = value.chars();
    let first = chars.next().unwrap_or(' ');
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err("must start with a lowercase letter or digit");
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_' | ':'))
    {
        return Err("allowed characters: a-z 0-9 . - _ :");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_ids_roundtrip_and_are_time_ordered() {
        let a = TaskId::new();
        let b = TaskId::new();
        assert!(a <= b);
        let parsed: TaskId = a.to_string().parse().unwrap();
        assert_eq!(a, parsed);
        assert!("nope".parse::<TaskId>().is_err());
    }

    #[test]
    fn slugs_reject_traversal_and_uppercase() {
        assert!(ToolId::new("fs.read").is_ok());
        assert!(ToolId::new("../etc").is_err());
        assert!(ToolId::new("Fs").is_err());
        assert!(ToolId::new("a/b").is_err());
        assert!(ToolId::new("").is_err());
        assert!(serde_json::from_str::<ToolId>("\"BAD ID\"").is_err());
        assert_eq!(
            serde_json::to_string(&ToolId::new("x").unwrap()).unwrap(),
            "\"x\""
        );
    }
}
