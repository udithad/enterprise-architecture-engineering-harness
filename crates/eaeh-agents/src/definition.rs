use eaeh_core::{AgentDefinitionId, AutonomyLevel, HarnessError, ModelId, PromptId, Result, ToolId};
use eaeh_governance::BudgetLimits;
use eaeh_validation::schema::CompiledSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPreferences {
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
    #[serde(default)]
    pub min_quality: u8,
    #[serde(default)]
    pub preferred: Vec<ModelId>,
    pub max_output_tokens: u32,
}

/// A deterministic validator run on the agent's final output inside the correction loop
/// (Phase 3, ADR-0021). `context` maps context keys to JSON pointers into the task input
/// (e.g. `{"requirements": "/inputs/requirements"}`); the platform adds `standards` and
/// `knowledge` itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputValidatorRef {
    pub validator: String,
    #[serde(default)]
    pub context: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DelegationPolicy {
    #[serde(default)]
    pub may_delegate_to: Vec<AgentDefinitionId>,
    #[serde(default)]
    pub max_children: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub id: AgentDefinitionId,
    pub version: semver::Version,
    pub purpose: String,
    #[serde(default)]
    pub responsibilities: Vec<String>,
    pub capabilities: BTreeSet<String>,
    /// Skill references (opaque until Skills Management in Phase 2).
    #[serde(default)]
    pub skills: Vec<String>,
    pub model: ModelPreferences,
    pub allowed_tools: Vec<ToolId>,
    #[serde(default)]
    pub required_context: Vec<String>,
    /// Approved prompt template providing the agent's instructions.
    pub instructions_prompt: PromptId,
    pub input_schema: Value,
    pub output_schema: Value,
    pub autonomy: AutonomyLevel,
    #[serde(default)]
    pub delegation: DelegationPolicy,
    /// Per-instance budget limits.
    pub constraints: BudgetLimits,
    #[serde(default)]
    pub evaluation_criteria: Vec<String>,
    /// Deterministic domain validators for the final output (Phase 3).
    #[serde(default)]
    pub output_validators: Vec<OutputValidatorRef>,
}

impl AgentDefinition {
    pub fn validate(&self) -> Result<()> {
        if self.purpose.trim().is_empty() {
            return Err(HarnessError::invalid_input("agent purpose required"));
        }
        if self.capabilities.is_empty() {
            return Err(HarnessError::invalid_input(
                "agent must declare at least one capability",
            ));
        }
        let unique: BTreeSet<&ToolId> = self.allowed_tools.iter().collect();
        if unique.len() != self.allowed_tools.len() {
            return Err(HarnessError::invalid_input("duplicate allowed tool"));
        }
        if self.model.max_output_tokens == 0 {
            return Err(HarnessError::invalid_input("model.max_output_tokens must be > 0"));
        }
        if self.delegation.may_delegate_to.contains(&self.id) {
            return Err(HarnessError::invalid_input("an agent may not delegate to itself"));
        }
        if !self.delegation.may_delegate_to.is_empty() && self.delegation.max_children == 0 {
            return Err(HarnessError::invalid_input(
                "delegation.max_children must be > 0 when delegation is allowed",
            ));
        }
        CompiledSchema::compile(&self.input_schema)?;
        CompiledSchema::compile(&self.output_schema)?;
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        Ok(eaeh_core::hash::canonical_json_hash(&serde_json::to_value(self)?))
    }

    pub fn reference(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
}
