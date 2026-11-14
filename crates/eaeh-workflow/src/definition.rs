//! Declarative workflow definitions and their static validation.

use eaeh_core::{AutonomyLevel, HarnessError, Result, RiskClass, Role, ToolId, WorkflowDefinitionId};
use eaeh_governance::BudgetLimits;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Succeeded,
    Failed,
    TimedOut,
    Approved,
    Rejected,
    /// Approver requested changes (Phase 3). Falls back to the `rejected` transition when
    /// a gate defines none.
    ChangesRequested,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Succeeded => "succeeded",
            Trigger::Failed => "failed",
            Trigger::TimedOut => "timed_out",
            Trigger::Approved => "approved",
            Trigger::Rejected => "rejected",
            Trigger::ChangesRequested => "changes_requested",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            backoff_ms: 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum StateKind {
    /// Work performed by a dynamically selected agent.
    AgentTask {
        task_kind: String,
        capabilities: BTreeSet<String>,
        max_autonomy: AutonomyLevel,
        /// Store the output as a versioned artifact with this name.
        #[serde(default)]
        artifact: Option<String>,
        /// Named inputs resolved by the engine from bindings (see [`resolve_bindings`])
        /// and passed to the agent as `input.inputs` (Phase 3).
        #[serde(default)]
        inputs: BTreeMap<String, Value>,
    },
    /// Deterministic tool step executed by the workflow engine's service identity.
    ToolStep {
        tool: ToolId,
        input: Value,
        #[serde(default)]
        artifact: Option<String>,
    },
    /// Hybrid orchestration: an (intelligent) planner proposes steps; the engine
    /// validates them against this declaration and policy, then executes them.
    Planned {
        allowed_task_kinds: Vec<String>,
        max_steps: u32,
        max_autonomy: AutonomyLevel,
        #[serde(default)]
        artifact: Option<String>,
    },
    /// Mandatory human approval of the artifact produced by `artifact_from`.
    ApprovalGate {
        required_role: Role,
        artifact_from: String,
        /// Maximum `request_changes` decisions honoured for this gate per instance; a
        /// further request follows the `rejected` transition (Phase 3, ADR-0020).
        #[serde(default = "default_max_revisions")]
        max_revisions: u32,
    },
    Terminal {
        outcome: Outcome,
    },
}

fn default_max_revisions() -> u32 {
    3
}

/// Reference to a platform quality gate run on a work state's output before it is stored
/// (fail closed). `context` may contain bindings (Phase 3, ADR-0021).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityGateRef {
    pub gate: String,
    #[serde(default)]
    pub context: BTreeMap<String, Value>,
}

/// Resolve engine bindings inside a JSON value (deterministic, no expressions):
/// * `{"$output": "<state>"}` (optionally with `"pointer": "/json/pointer"`) – that state's output;
/// * `{"$input": "/json/pointer"}` – part of the workflow input (`""` = all);
/// * `{"$artifact": "<state>"}` – `{id, name, version, hash}` of that state's artifact.
///
/// Anything else is copied, recursing into arrays and objects.
pub fn resolve_bindings(
    v: &Value,
    input: &Value,
    outputs: &BTreeMap<String, Value>,
    artifacts: &dyn Fn(&str) -> Option<Value>,
) -> Result<Value> {
    match v {
        Value::Object(m) => {
            if let Some(state) = m.get("$output").and_then(Value::as_str) {
                let out = outputs.get(state).ok_or_else(|| {
                    HarnessError::integrity(format!("binding $output '{state}': state has no output yet"))
                })?;
                return match m.get("pointer").and_then(Value::as_str) {
                    None => Ok(out.clone()),
                    Some(p) => out.pointer(p).cloned().ok_or_else(|| {
                        HarnessError::integrity(format!("binding $output '{state}': pointer '{p}' not found"))
                    }),
                };
            }
            if let Some(p) = m.get("$input").and_then(Value::as_str) {
                return input
                    .pointer(p)
                    .cloned()
                    .ok_or_else(|| HarnessError::invalid_input(format!("workflow input has no '{p}'")));
            }
            if let Some(state) = m.get("$artifact").and_then(Value::as_str) {
                return artifacts(state).ok_or_else(|| {
                    HarnessError::integrity(format!("binding $artifact '{state}': state has no artifact"))
                });
            }
            let mut o = serde_json::Map::new();
            for (k, x) in m {
                o.insert(k.clone(), resolve_bindings(x, input, outputs, artifacts)?);
            }
            Ok(Value::Object(o))
        }
        Value::Array(a) => Ok(Value::Array(
            a.iter()
                .map(|x| resolve_bindings(x, input, outputs, artifacts))
                .collect::<Result<_>>()?,
        )),
        other => Ok(other.clone()),
    }
}

/// States referenced by `$output` / `$artifact` bindings in a value.
pub fn binding_states(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for key in ["$output", "$artifact"] {
                if let Some(s) = m.get(key).and_then(Value::as_str) {
                    out.insert(s.to_string());
                }
            }
            m.values().for_each(|x| binding_states(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| binding_states(x, out)),
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateDef {
    pub name: String,
    #[serde(flatten)]
    pub kind: StateKind,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub retry: RetryPolicy,
    /// Approval gates that must have been approved (in this run) before this state may
    /// execute. Enforced at runtime independently of the transition graph.
    #[serde(default)]
    pub requires_approval_of: Vec<String>,
    #[serde(default)]
    pub transitions: BTreeMap<Trigger, String>,
    /// Quality gate run on the output before it is stored (work states, Phase 3).
    #[serde(default)]
    pub quality_gate: Option<QualityGateRef>,
    /// The output is `{"files": {path: text}, ...}`: every file is stored as artifact
    /// `<artifact>/<path>` and a manifest binding all file hashes is stored as `<artifact>`
    /// (the artifact an approval gate binds to). Phase 3.
    #[serde(default)]
    pub bundle: bool,
}

impl StateDef {
    /// Bindings used by this state (inputs, tool input, gate context).
    pub fn bindings(&self) -> Vec<&Value> {
        let mut v: Vec<&Value> = vec![];
        match &self.kind {
            StateKind::AgentTask { inputs, .. } => v.extend(inputs.values()),
            StateKind::ToolStep { input, .. } => v.push(input),
            _ => {}
        }
        if let Some(g) = &self.quality_gate {
            v.extend(g.context.values());
        }
        v
    }

    pub fn artifact_name(&self) -> Option<&str> {
        match &self.kind {
            StateKind::AgentTask { artifact, .. }
            | StateKind::ToolStep { artifact, .. }
            | StateKind::Planned { artifact, .. } => artifact.as_deref(),
            _ => None,
        }
    }

    pub fn is_work(&self) -> bool {
        matches!(
            self.kind,
            StateKind::AgentTask { .. } | StateKind::ToolStep { .. } | StateKind::Planned { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: WorkflowDefinitionId,
    pub version: semver::Version,
    pub description: String,
    pub initial: String,
    pub states: Vec<StateDef>,
    #[serde(default = "BudgetLimits::platform_default")]
    pub budget: BudgetLimits,
}

/// A step proposed by a planner (untrusted until validated).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanStep {
    pub task_kind: String,
    pub capabilities: BTreeSet<String>,
    pub input: Value,
    pub risk: RiskClass,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanProposal {
    pub steps: Vec<PlanStep>,
    #[serde(default)]
    pub rationale: String,
}

impl WorkflowDefinition {
    pub fn state(&self, name: &str) -> Result<&StateDef> {
        self.states
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| HarnessError::integrity(format!("state '{name}' not in definition {}", self.id)))
    }

    pub fn reference(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    pub fn validate(&self) -> Result<()> {
        let err = |m: String| {
            Err(HarnessError::invalid_input(format!(
                "workflow {}: {m}",
                self.reference()
            )))
        };
        let names: BTreeSet<&str> = self.states.iter().map(|s| s.name.as_str()).collect();
        if names.len() != self.states.len() {
            return err("duplicate state names".into());
        }
        if !names.contains(self.initial.as_str()) {
            return err(format!("initial state '{}' not defined", self.initial));
        }
        let gates: BTreeSet<&str> = self
            .states
            .iter()
            .filter(|s| matches!(s.kind, StateKind::ApprovalGate { .. }))
            .map(|s| s.name.as_str())
            .collect();
        for s in &self.states {
            for (t, target) in &s.transitions {
                if !names.contains(target.as_str()) {
                    return err(format!(
                        "state '{}' transition {t:?} targets unknown state '{target}'",
                        s.name
                    ));
                }
            }
            for g in &s.requires_approval_of {
                if !gates.contains(g.as_str()) {
                    return err(format!(
                        "state '{}' requires approval of '{g}', which is not an approval gate",
                        s.name
                    ));
                }
            }
            if s.retry.max_attempts == 0 || s.retry.max_attempts > 10 {
                return err(format!("state '{}' retry.max_attempts must be 1..=10", s.name));
            }
            match &s.kind {
                StateKind::Terminal { .. } => {
                    if !s.transitions.is_empty() {
                        return err(format!("terminal state '{}' has transitions", s.name));
                    }
                }
                StateKind::ApprovalGate {
                    artifact_from,
                    max_revisions,
                    ..
                } => {
                    if *max_revisions > 20 {
                        return err(format!("gate '{}' max_revisions must be 0..=20", s.name));
                    }
                    if !s.transitions.contains_key(&Trigger::Approved)
                        || !s.transitions.contains_key(&Trigger::Rejected)
                    {
                        return err(format!(
                            "gate '{}' needs approved and rejected transitions",
                            s.name
                        ));
                    }
                    let src = self.states.iter().find(|x| &x.name == artifact_from);
                    if src.and_then(|x| x.artifact_name()).is_none() {
                        return err(format!(
                            "gate '{}' reviews '{artifact_from}', which produces no artifact",
                            s.name
                        ));
                    }
                }
                StateKind::Planned {
                    allowed_task_kinds,
                    max_steps,
                    ..
                } => {
                    if allowed_task_kinds.is_empty() || *max_steps == 0 || *max_steps > 50 {
                        return err(format!(
                            "planned state '{}' needs allowed kinds and 1..=50 max_steps",
                            s.name
                        ));
                    }
                    if !s.transitions.contains_key(&Trigger::Succeeded) {
                        return err(format!("state '{}' has no succeeded transition", s.name));
                    }
                }
                _ => {
                    if !s.transitions.contains_key(&Trigger::Succeeded) {
                        return err(format!("state '{}' has no succeeded transition", s.name));
                    }
                }
            }
        }
        // Bindings may only reference work states; gates/bundles only on work states.
        let work: BTreeSet<&str> = self
            .states
            .iter()
            .filter(|s| s.is_work())
            .map(|s| s.name.as_str())
            .collect();
        for s in &self.states {
            let mut refs = BTreeSet::new();
            for b in s.bindings() {
                binding_states(b, &mut refs);
            }
            for r in refs {
                if !work.contains(r.as_str()) {
                    return err(format!(
                        "state '{}' binds to '{r}', which is not a work state",
                        s.name
                    ));
                }
            }
            if (s.quality_gate.is_some() || s.bundle) && !s.is_work() {
                return err(format!(
                    "state '{}': quality_gate/bundle are only allowed on work states",
                    s.name
                ));
            }
            if s.bundle && s.artifact_name().is_none() {
                return err(format!("state '{}': bundle requires an artifact name", s.name));
            }
        }
        // A terminal state must be reachable from the initial state.
        let mut seen = BTreeSet::new();
        let mut q = VecDeque::from([self.initial.as_str()]);
        let mut terminal_reachable = false;
        while let Some(n) = q.pop_front() {
            if !seen.insert(n) {
                continue;
            }
            let s = self.state(n)?;
            if matches!(s.kind, StateKind::Terminal { .. }) {
                terminal_reachable = true;
            }
            q.extend(s.transitions.values().map(String::as_str));
        }
        if !terminal_reachable {
            return err("no terminal state reachable from the initial state".into());
        }
        Ok(())
    }
}
