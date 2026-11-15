//! Typed core event catalogue (schema version 1). Payloads carry identifiers and
//! metadata only — never secrets or full content.

use crate::DomainEvent;
use serde::{Deserialize, Serialize};

macro_rules! event {
    ($name:ident, $ty:literal { $($(#[$fm:meta])* pub $f:ident : $t:ty),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name { $($(#[$fm])* pub $f: $t),* }
        impl DomainEvent for $name {
            const TYPE: &'static str = $ty;
            const VERSION: u16 = 1;
        }
    };
}

event!(AgentCreated, "agent.created" {
    pub instance_id: String,
    pub definition_id: String,
    pub definition_version: String,
    pub parent_instance_id: Option<String>,
    pub delegation_depth: u32,
});
event!(AgentExecutionStarted, "agent.execution_started" { pub instance_id: String, pub task_id: Option<String> });
event!(AgentExecutionCompleted, "agent.execution_completed" {
    pub instance_id: String,
    pub task_id: Option<String>,
    pub succeeded: bool,
    pub steps: u32,
});
event!(AgentTerminated, "agent.terminated" { pub instance_id: String, pub final_state: String });
event!(AgentDelegated, "agent.delegated" {
    pub from_instance_id: String,
    pub to_instance_id: String,
    pub task_fingerprint: String,
});
event!(TaskAssigned, "task.assigned" { pub task_id: String, pub agent_instance_id: Option<String>, pub kind: String });
event!(ToolInvoked, "tool.invoked" {
    pub invocation_id: String,
    pub tool_id: String,
    pub tool_version: String,
    pub principal_id: String,
    pub succeeded: bool,
    pub duration_ms: u64,
});
event!(PolicyEvaluated, "policy.evaluated" {
    pub action: String,
    pub effect: String,
    pub matched_rules: Vec<String>,
});
event!(ApprovalRequested, "approval.requested" {
    pub approval_id: String,
    pub subject_type: String,
    pub subject_ref: String,
    pub required_role: String,
});
event!(ApprovalGranted, "approval.granted" { pub approval_id: String, pub approver_id: String });
event!(ApprovalRejected, "approval.rejected" { pub approval_id: String, pub approver_id: String });
event!(ApprovalChangesRequested, "approval.changes_requested" { pub approval_id: String, pub approver_id: String });
event!(TraceLinksRecorded, "trace.links_recorded" { pub artifact_id: String, pub links: u32 });
event!(ArtifactGenerated, "artifact.generated" {
    pub artifact_id: String,
    pub name: String,
    pub version: i64,
    pub content_hash: String,
});
event!(ValidationFailed, "validation.failed" { pub gate_id: String, pub subject: String, pub findings: u32 });
event!(WorkflowStarted, "workflow.started" { pub instance_id: String, pub definition_id: String, pub definition_version: String });
event!(WorkflowTransitioned, "workflow.transitioned" {
    pub instance_id: String,
    pub from_state: String,
    pub to_state: String,
    pub trigger: String,
});
event!(WorkflowCompleted, "workflow.completed" { pub instance_id: String, pub status: String });
event!(BudgetExhausted, "budget.exhausted" { pub owner_id: String, pub resource: String, pub limit: u64 });
event!(SupervisionEscalated, "supervision.escalated" { pub subject_id: String, pub reason: String });
event!(PluginStateChanged, "plugin.state_changed" { pub plugin_id: String, pub version: String, pub state: String });
event!(ModelInvoked, "model.invoked" {
    pub provider_id: String,
    pub model_id: String,
    pub model_version: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub succeeded: bool,
});

// ---- Phase 2: knowledge and skills ----
event!(KnowledgeIngested, "knowledge.ingested" {
    pub document_id: String,
    pub citation_base: String,
    pub status: String,
    pub chunks: u32,
});
event!(KnowledgeActivated, "knowledge.activated" { pub document_id: String, pub superseded: Option<String> });
event!(KnowledgeRetrieved, "knowledge.retrieved" { pub hits: u32, pub mode: String, pub principal_id: String });
event!(KnowledgeConflictDetected, "knowledge.conflict_detected" { pub subject: String, pub document_a: String, pub document_b: String });
event!(SkillRegistered, "skill.registered" { pub skill: String, pub version: String, pub status: String });
event!(SkillStateChanged, "skill.state_changed" { pub skill: String, pub version: String, pub status: String });

// ---- Approved design artifacts in the project workspace (ARTIFACT-MATERIALIZATION-PLAN) ----
event!(ArtifactMaterialized, "artifact.materialized" {
    pub bundle_id: String,
    pub target_dir: String,
    pub files: u32,
    pub status: String,
});
