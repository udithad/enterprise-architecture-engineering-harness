// Shapes returned by the EAEH /v1 API (crates/eaeh-api) and the portal server.

export type WorkflowStatus =
  | "running"
  | "waiting_approval"
  | "waiting_external"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "escalated";

export type ApprovalStatus = "pending" | "approved" | "rejected" | "changes_requested" | "superseded";
export type Decision = "approve" | "request_changes" | "reject";

export interface Acting {
  profile: string;
  principal: string;
  kind: string;
  roles: string[];
  projects: unknown;
}

export interface PortalSession {
  installation: { data_dir: string; config_dir: string; label?: string | null };
  viewer: Acting;
  approver: Acting | null;
  /** Identity used only to register skill files (`eaeh portal --steward`). */
  steward?: Acting | null;
  can_decide: boolean;
  can_register_skills?: boolean;
  execution_mode: string;
  api_version: string;
  local_only: boolean;
  authentication: string;
}

export interface ArtifactRef {
  id: string;
  name: string;
  version: number;
  hash: string;
}

export interface WorkflowView {
  id: string;
  definition_id: string;
  definition_version: string;
  project_id: string;
  state: string;
  status: WorkflowStatus;
  version: number;
  started_by: string;
  correlation_id: string;
  cancel_requested: boolean;
  created_at: string;
  updated_at: string;
  artifacts: Record<string, ArtifactRef>;
  approved_gates: Record<string, string>;
  current_approval: Record<string, string>;
  revisions: Record<string, number>;
  visits: Record<string, number>;
  quality: Record<string, unknown>;
  feedback: Record<string, string>;
  failure: string | null;
  awaiting_external: string | null;
  /** Held at an approved gate: the approved documents could not be written to the workspace. */
  blocked?: { gate: string; reason: string; since: string } | null;
}

export interface TaskSummary {
  id: string;
  workflow_instance_id: string;
  state: string;
  kind: string;
  name: string;
  status: string;
  attempt: number;
  agent: string | null;
  error: string | null;
  created_at: string;
  updated_at: string;
}

export interface DecisionRecord {
  approver_id: string;
  approver_role: string;
  decision: string;
  comments: string;
  artifact_hash: string | null;
  decided_at: string;
}

export interface ApprovalView {
  id: string;
  project_id: string;
  project_name: string;
  subject_type: string;
  subject_ref: string;
  document_type: string;
  gate: string | null;
  status: ApprovalStatus;
  required_role: string;
  authors: string[];
  requested_by: string;
  created_at: string;
  decided_at: string | null;
  decision: DecisionRecord | null;
  artifact_id: string | null;
  artifact_hash: string | null;
  artifact: { name: string; version: number; hash: string; media_type: string; size_bytes: number; created_at: string } | null;
  workflow: { id: string; definition_id: string; definition_version: string; state: string; status: WorkflowStatus } | null;
}

export interface ActivityItem {
  kind: string;
  occurred_at: string;
  workflow_instance_id: string;
  definition_id: string;
  state: string | null;
  to_state: string | null;
  actor_id: string;
  project_id?: string;
  project_name?: string;
}

/** Lifecycle status of a project, derived by the backend from its workflow records. */
export type ProjectStatus = "ready" | "in_progress" | "waiting_approval" | "blocked" | "completed" | "failed" | "cancelled";

export interface ProjectStack {
  language: string;
  framework: string;
  build_tool: string;
  constraints?: string[];
}

export interface CurrentWorkflow {
  id: string;
  definition_id: string;
  status: WorkflowStatus;
  state: string;
  blocked: { gate: string; reason: string; since: string } | null;
  updated_at: string;
  created_at: string;
}

export interface DashboardProject {
  id: string;
  name: string;
  restricted: boolean;
  workflow_counts?: Record<string, number>;
  active_tasks?: number;
  pending_approvals?: number;
  status?: ProjectStatus;
  current_workflow?: CurrentWorkflow | null;
  last_activity?: string | null;
  description?: string | null;
  stack?: ProjectStack | null;
  workspace?: WorkspaceRegistration | null;
  registered_workflows?: string[] | null;
  manifest_issue?: string | null;
}

export interface Dashboard {
  generated_at: string;
  execution_mode: string;
  summary: {
    projects: number;
    in_progress: number;
    executing_tasks: number;
    waiting_external: number;
    waiting_approval: number;
    pending_approvals: number;
    completed: number;
    failed_or_blocked: number;
  };
  projects: DashboardProject[];
  active: { project_id: string; project_name: string; workflow: WorkflowView; tasks: TaskSummary[] }[];
  pending_approvals: ApprovalView[];
  activity: ActivityItem[];
}

export interface Transition {
  seq: number;
  from_state: string;
  to_state: string;
  trigger: string;
  actor_id: string;
  occurred_at: string;
}

export interface StateDef {
  name: string;
  type: "agent_task" | "tool_step" | "planned" | "approval_gate" | "terminal";
  task_kind?: string;
  tool?: string;
  artifact?: string | null;
  artifact_from?: string;
  required_role?: string;
  outcome?: "succeeded" | "failed" | "cancelled";
  transitions: Record<string, string>;
  requires_approval_of?: string[];
}

export interface WorkflowDefinition {
  id: string;
  version: string;
  description: string;
  initial: string;
  states: StateDef[];
}

export interface GateCheck {
  gate: string;
  artifact_from: string;
  approval_id: string | null;
  approver_id: string | null;
  approver_role: string | null;
  decided_at: string | null;
  approved_hash: string | null;
  current_hash: string | null;
  issues: string[];
}

export interface Governance {
  gates: GateCheck[];
  gates_total: number;
  gates_granted: number;
  gates_with_issues: number;
  completion_check: { outcome: string; occurred_at: string; details: { violations?: string[] } | null } | null;
  /** null while the workflow is not finished successfully. */
  completion_verified: boolean | null;
}

export type GenerationSource = "external_ai" | "deterministic_replay" | "direct_provider" | "platform";

export interface Generation {
  source: GenerationSource;
  models: string[];
  agent_definition: string | null;
  produced_by: string;
  created_at: string;
  artifact: string;
  version: number;
}

export interface WorkflowDetail {
  project: { id: string; name: string };
  workflow: WorkflowView;
  definition: WorkflowDefinition | null;
  history: Transition[];
  tasks: TaskSummary[];
  approvals: ApprovalView[] | null;
  execution_mode: string;
  governance: Governance;
  artifact_provenance: Record<string, Generation>;
  documents?: GateDocument[];
  workspace?: WorkspaceRegistration | null;
}

export interface WorkspaceRegistration {
  root: string;
  registered_by: string;
  registered_at: string;
}

/** Verified state of a document folder in the project workspace. `verified` is reported
 * only when every file on disk was re-hashed for this response and equals the approved version. */
export type MaterializationState =
  | "verified"
  | "modified"
  | "missing"
  | "outdated"
  | "failed"
  | "conflict"
  | "not_materialized"
  | "awaiting_approval"
  | "no_workspace";

export interface MaterializationStatus {
  state: MaterializationState;
  explanation: string;
  root: string | null;
  target_dir: string;
  files: { path: string; expected_hash: string; actual_hash: string | null; state: "verified" | "modified" | "missing" }[];
  materialized_bundle: string | null;
  last_success_at: string | null;
  last_attempt: { id: string; bundle_id: string; bundle_version: number; status: string; detail: string; actor_id: string; created_at: string } | null;
  checked_at: string;
}

export interface GateDocument {
  gate: string;
  artifact_from: string;
  artifact: ArtifactRef | null;
  approval: GateCheck;
  approved: boolean;
  target_dir: string | null;
  materialization: MaterializationStatus | null;
  blocked: string | null;
}

export interface DocumentFile {
  path: string;
  artifact_id: string;
  name: string;
  version: number;
  hash: string;
  media_type: string;
  content: string | null;
}

export interface TraceLink {
  from_kind: string;
  from_id: string;
  relation: string;
  to_kind: string;
  to_id: string;
  artifact_id: string;
  artifact_name: string;
  artifact_version: number;
  artifact_hash: string;
  workflow_instance_id: string;
  approval_id: string;
}

export interface ApprovalDocument {
  approval: ApprovalView;
  document: {
    artifact: { id: string; name: string; version: number; content_hash: string; size_bytes: number; media_type: string; created_at: string };
    bundle: boolean;
    files: DocumentFile[];
  } | null;
  workflow: {
    instance: WorkflowView;
    previous_requests: ApprovalView[];
    trace_links: TraceLink[];
    artifact_provenance: Record<string, Generation>;
    execution_mode: string;
    workspace_document?: GateDocument | null;
    workspace?: WorkspaceRegistration | null;
  } | null;
  /** Set for skill approvals: the full definition under review. */
  skill?: SkillView | null;
  decision_endpoint: string;
}

export interface SkillView {
  id: string;
  version: string;
  reference: string;
  description: string;
  task_kinds: string[];
  capabilities: string[];
  /** `workflow/stage` layers this skill is applied to (from its task kinds). */
  layers: string[];
  required_tools: string[];
  required_knowledge: { query: string; k: number }[];
  max_risk: string;
  dependencies: { id: string; version: string }[];
  evaluation_criteria: string[];
  output_schema: unknown;
  instructions: string;
  /** Skill directories: the captured package (all files are covered by the digest). */
  package?: { dir: string; source_version: string; revision: number; tree_hash: string; file_count: number; size: number } | null;
  digest?: string;
  status?: string;
  registered_by?: string;
  file?: string | null;
  files_endpoint?: string;
}

export interface SkillFileEntry {
  path: string;
  sha256: string;
  size: number;
  kind: "markdown" | "json" | "yaml" | "toml" | "text" | "code" | "binary" | string;
  content: string | null;
  /** Compared with the version agents use now: `added`, `modified`, `unchanged` or null. */
  change?: string | null;
}

export interface SkillFilesView extends SkillView {
  approval_id: string | null;
  in_force_version: string | null;
  compared_with: string | null;
  files: SkillFileEntry[];
  removed_files: string[];
  history: { version: string; approval_id: string; status: string; created_at: string; decided_at: string | null; decision: { approver_id: string; decision: string; comments: string; decided_at: string } | null }[];
}

export interface DiscoveryOutcome {
  dir: string;
  skill: string | null;
  version: string | null;
  action: "new" | "changed" | "unchanged" | "invalid" | string;
  detail: string;
  tree_hash: string | null;
  approval_id: string | null;
}

export interface SkillCatalogItem extends SkillView {
  format?: "markdown" | "json" | "directory";
  in_force?: boolean;
  file_digest?: string;
  registration: { status: string; approval_id: string | null; digest: string; registered_by: string } | null;
  file_changed: boolean;
}

export interface SkillCatalog {
  skills_dir: string;
  file_error: string | null;
  items: SkillCatalogItem[];
  directories?: { dir: string; skill?: string; manifest_version?: string; tree_hash?: string; registered?: string | null; error?: string }[];
  layers: Record<string, string[]>;
}

export interface SkillSyncResult {
  reference: string;
  action: "registered" | "already_registered" | "missing_definition" | "failed" | string;
  detail: string;
  approval_id: string | null;
}

export interface ProjectRef {
  id: string;
  name: string;
}

export interface ReadinessCheck {
  area: string;
  status: "ok" | "warn" | "block" | string;
  message: string;
  fix: string | null;
}

export interface Readiness {
  project: string;
  ready: boolean;
  blocking: number;
  warnings: number;
  checks: ReadinessCheck[];
}

export interface ProjectManifestView {
  description: string;
  stack: ProjectStack;
  workflows: string[];
  brd: { path: string; citation: string } | null;
  knowledge: { path: string; citation: string }[];
  execution_mode: string | null;
  roles: { developer: string; steward: string; approvers: string[] };
}

export interface ProjectOverview {
  project: { id: string; name: string; domain: string | null };
  workspace?: WorkspaceRegistration | null;
  manifest?: ProjectManifestView | null;
  manifest_issue?: string | null;
  readiness?: Readiness | { error: string };
  current_workflow?: CurrentWorkflow | null;
  documents?: { workflow_instance_id: string; items: GateDocument[] };
  audit?: AuditRecord[];
  status?: ProjectStatus;
  workflow_counts?: Record<string, number>;
  workflows?: WorkflowView[];
  artifacts?: {
    id: string; name: string; version: number; hash: string; state: string; document_type: string;
    workflow_instance_id: string; workflow_status: WorkflowStatus; gate: string | null; gate_approved: boolean | null;
  }[];
  activity?: ActivityItem[];
  approvals?: ApprovalView[];
}

export interface AuditRecord {
  seq: number;
  id: string;
  occurred_at: string;
  tenant_id: string | null;
  project_id: string | null;
  actor_id: string;
  actor_kind: string;
  action: string;
  resource_type: string;
  resource_id: string;
  outcome: string;
  correlation_id: string | null;
  details: string;
}

export interface Page<T> {
  items: T[];
  next_cursor?: string | null;
  restricted_projects?: string[];
}
