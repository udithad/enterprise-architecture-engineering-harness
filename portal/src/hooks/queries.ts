import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, request } from "@/lib/api";
import { isActive } from "@/lib/status";
import type {
  ActivityItem,
  ApprovalDocument,
  ApprovalView,
  AuditRecord,
  Dashboard,
  Decision,
  Page,
  PortalSession,
  ProjectOverview,
  ProjectRef,
  WorkflowDefinition,
  WorkflowDetail,
  WorkflowView,
  RestartPlan,
  TaskSummary as TaskSummaryT,
  TaskDetailView,
  TaskQueueItem,
  ProjectPlan,
  ExecutionAttempt,
  WorkerInfo,
  ExternalHandoff,
  Integrations,
  DirListing,
  Inspection,
  RegisterInput,
  RegisterResult,
} from "@/lib/types";

// Polling: frequent enough to follow live work, light enough for a local SQLite backend.
// TanStack Query pauses polling while the tab is hidden.
export const POLL = { dashboard: 10_000, active: 5_000, lists: 15_000 } as const;

export const qk = {
  session: ["session"] as const,
  dashboard: ["dashboard"] as const,
  projects: ["projects"] as const,
  project: (id: string) => ["project", id] as const,
  workflows: (status: string, project: string) => ["workflows", status, project] as const,
  workflow: (id: string) => ["workflow", id] as const,
  approvals: (status: string, project: string) => ["approvals", status, project] as const,
  approvalDoc: (id: string) => ["approval-doc", id] as const,
  activity: (project: string) => ["activity", project] as const,
  audit: (before: number | null) => ["audit", before] as const,
  skills: ["skills"] as const,
};

const qs = (o: Record<string, string | undefined>) => {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(o)) if (v) p.set(k, v);
  const s = p.toString();
  return s ? `?${s}` : "";
};

export function useSession(enabled = true) {
  return useQuery({ queryKey: qk.session, queryFn: () => api.session(), staleTime: 60_000, retry: false, enabled });
}

export function useDashboard() {
  return useQuery({ queryKey: qk.dashboard, queryFn: () => api.get<Dashboard>("/v1/dashboard"), refetchInterval: POLL.dashboard });
}

export function useDefinitions() {
  return useQuery({
    queryKey: ["definitions"],
    queryFn: () => api.get<{ items: WorkflowDefinition[] }>("/v1/workflow-definitions"),
    staleTime: 5 * 60_000,
  });
}

export function useProjects() {
  return useQuery({ queryKey: qk.projects, queryFn: () => api.get<{ items: ProjectRef[] }>("/v1/projects"), staleTime: 30_000 });
}

export function useProjectOverview(id: string) {
  return useQuery({
    queryKey: qk.project(id),
    queryFn: () => api.get<ProjectOverview>(`/v1/projects/${encodeURIComponent(id)}/overview`),
    refetchInterval: POLL.lists,
  });
}

export function useWorkflowQueue(status: "active" | "all", project: string, f: { definition?: string; stage?: string; since?: string } = {}) {
  return useQuery({
    queryKey: [...qk.workflows(status, project), f],
    queryFn: () =>
      api.get<Page<{ project_id: string; project_name: string; workflow: WorkflowView; pending_action?: import("@/lib/types").PendingAction; current_task?: TaskSummaryT | null }>>(
        `/v1/workflow-queue${qs({ status, project_id: project === "all" ? undefined : project, definition: f.definition || undefined, stage: f.stage || undefined, since: f.since ? new Date(f.since).toISOString() : undefined })}`,
      ),
    refetchInterval: POLL.lists,
  });
}

export function useWorkflowDetail(id: string) {
  return useQuery({
    queryKey: qk.workflow(id),
    queryFn: () => api.get<WorkflowDetail>(`/v1/workflows/${encodeURIComponent(id)}/detail`),
    refetchInterval: (q) => (q.state.data && isActive(q.state.data.workflow.status) ? POLL.active : false),
  });
}

export function useApprovalQueue(status: string, project: string) {
  return useQuery({
    queryKey: qk.approvals(status, project),
    queryFn: () => api.get<Page<ApprovalView>>(`/v1/approval-queue${qs({ status, project_id: project === "all" ? undefined : project })}`),
    refetchInterval: POLL.lists,
  });
}

export function useApprovalDocument(id: string) {
  return useQuery({
    queryKey: qk.approvalDoc(id),
    queryFn: () => api.get<ApprovalDocument>(`/v1/approvals/${encodeURIComponent(id)}/document`),
    // A pending request can be superseded by a revision; re-check its status periodically.
    refetchInterval: (q) => (q.state.data?.approval.status === "pending" ? POLL.lists : false),
  });
}

export function useActivity(project: string) {
  return useQuery({
    queryKey: qk.activity(project),
    queryFn: () => api.get<Page<ActivityItem>>(`/v1/activity${qs({ limit: "100", project_id: project === "all" ? undefined : project })}`),
    refetchInterval: POLL.lists,
  });
}

export function useAudit(before: number | null) {
  return useQuery({
    queryKey: qk.audit(before),
    queryFn: () => api.get<Page<AuditRecord>>(`/v1/audit${qs({ order: "desc", limit: "50", before_seq: before ? String(before) : undefined })}`),
    retry: false,
  });
}

export interface DecisionInput {
  id: string;
  decision: Decision;
  comments: string;
  artifact_hash: string | null;
}

export function useSkillCatalog() {
  return useQuery({ queryKey: qk.skills, queryFn: () => api.get<import("@/lib/types").SkillCatalog>("/v1/skills/catalog"), refetchInterval: POLL.lists });
}

export function useSkillFiles(id: string, version: string) {
  return useQuery({
    queryKey: ["skill-files", id, version],
    queryFn: () => api.get<import("@/lib/types").SkillFilesView>(`/v1/skills/${encodeURIComponent(id)}/${encodeURIComponent(version)}/files`),
    enabled: !!id && !!version,
    refetchInterval: POLL.lists,
  });
}

/** "Scan now": automatic discovery on demand; registers new or changed skill directories as pending. */
export function useSkillDiscover() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.post<{ results: import("@/lib/types").DiscoveryOutcome[] }>("/v1/skills/discover", {}),
    onSettled: () => {
      void qc.invalidateQueries();
    },
  });
}

/** "Register skill files": the same registration service as `eaeh skill sync` (steward). */
export function useSkillSync() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (project_id: string) =>
      api.post<{ results: import("@/lib/types").SkillSyncResult[] }>("/v1/skills/sync", { project_id }),
    onSettled: () => {
      void qc.invalidateQueries();
    },
  });
}

/** Submits through the existing approval service; the backend is authoritative. */
export function useDecision() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (d: DecisionInput) =>
      api.post<ApprovalView>(`/v1/approvals/${encodeURIComponent(d.id)}/decision`, {
        decision: d.decision,
        comments: d.comments,
        artifact_hash: d.artifact_hash,
      }),
    onSettled: () => {
      void qc.invalidateQueries();
    },
  });
}

// ---------------------------------------------------------------- operations (operator)

const enc = encodeURIComponent;

/** Invalidate everything after a state change; the backend is the source of truth. */
function useOp<I, O>(fn: (i: I) => Promise<O>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSettled: () => {
      void qc.invalidateQueries();
    },
  });
}

export const useResume = () => useOp((id: string) => api.post<{ id: string }>(`/v1/workflows/${enc(id)}/resume`, {}));
export const useCancel = () => useOp((id: string) => api.post<{ id: string; status: string }>(`/v1/workflows/${enc(id)}/cancel`, {}));

/** Preview only: nothing is written. */
export function useRestartPreview() {
  return useMutation({
    mutationFn: (i: { id: string; stage?: string; input?: unknown }) =>
      api.post<RestartPlan>(`/v1/workflows/${enc(i.id)}/restart/preview`, { stage: i.stage, input: i.input }),
  });
}

export const useRestart = () =>
  useOp((i: { id: string; stage: string; fingerprint: string; reason: string; input?: unknown }) =>
    api.post<{ id: string; source: string; stage: string; source_cancelled: boolean }>(`/v1/workflows/${enc(i.id)}/restart`, {
      stage: i.stage,
      fingerprint: i.fingerprint,
      reason: i.reason,
      input: i.input,
    }),
  );

export const useTaskRetry = () => useOp((id: string) => api.post<{ task: string }>(`/v1/tasks/${enc(id)}/retry`, {}));

export const useStartWorkflow = () =>
  useOp((i: { definition: string; project_id: string; input: unknown }) => api.post<{ id: string; status: string }>("/v1/workflows", i));

export const useEnsureWorker = () =>
  useOp((project: string) => request<{ worker: string | null }>(`/portal/projects/${enc(project)}/worker`, { method: "POST", body: "{}" }));

export function useTaskDetail(id: string) {
  return useQuery({
    queryKey: ["task", id],
    queryFn: () => api.get<TaskDetailView>(`/v1/tasks/${enc(id)}`),
    refetchInterval: (q) => (q.state.data && isActive(q.state.data.workflow.status) ? POLL.active : false),
  });
}

export interface TaskFilters {
  project?: string;
  workflow?: string;
  stage?: string;
  status?: string;
  since?: string;
  cursor?: string;
}

export function useTaskQueue(f: TaskFilters, limit = 50) {
  return useQuery({
    queryKey: ["task-queue", f, limit],
    queryFn: () =>
      api.get<Page<TaskQueueItem> & { restricted_projects: string[] }>(
        `/v1/task-queue${qs({ project_id: f.project === "all" ? undefined : f.project, workflow: f.workflow, stage: f.stage, status: f.status, since: f.since, cursor: f.cursor, limit: String(limit) })}`,
      ),
    refetchInterval: POLL.lists,
  });
}

export function useProjectPlan(project: string, workflow: string) {
  return useQuery({
    queryKey: ["plan", project, workflow],
    queryFn: () => api.get<ProjectPlan>(`/v1/projects/${enc(project)}/plan${qs({ workflow })}`),
    enabled: !!project && !!workflow,
    retry: false,
  });
}

export function useExecutions(project: string) {
  return useQuery({
    queryKey: ["executions", project],
    queryFn: () =>
      api.get<{ items: ExecutionAttempt[]; workers: WorkerInfo[]; handoffs: ExternalHandoff[] }>(`/v1/executions${qs({ project_id: project, limit: "100" })}`),
    enabled: !!project && project !== "all",
    refetchInterval: POLL.lists,
  });
}

export function useIntegrations(project: string) {
  return useQuery({
    queryKey: ["integrations", project],
    queryFn: () => api.get<Integrations>(`/v1/integrations${qs({ project_id: project })}`),
    enabled: !!project && project !== "all",
    refetchInterval: POLL.lists,
  });
}

export function useCatalogList<T>(path: string) {
  return useQuery({ queryKey: ["catalog", path], queryFn: () => api.get<{ items: T[] }>(path), staleTime: 30_000 });
}

// ---------------------------------------------------------------- register existing project (admin)

export function useBrowse(path: string | null) {
  return useQuery({
    queryKey: ["fs", path],
    queryFn: () => request<DirListing>(`/portal/fs${qs({ path: path ?? undefined })}`),
    retry: false,
  });
}

export function useInspect() {
  return useMutation({ mutationFn: (path: string) => request<Inspection>("/portal/projects/inspect", { method: "POST", body: JSON.stringify({ path }) }) });
}

export const useRegisterProject = () =>
  useOp((i: RegisterInput) => request<RegisterResult>("/portal/projects/register", { method: "POST", body: JSON.stringify(i) }));

export type { PortalSession };
