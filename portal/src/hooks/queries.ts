import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
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

export function useWorkflowQueue(status: "active" | "all", project: string) {
  return useQuery({
    queryKey: qk.workflows(status, project),
    queryFn: () =>
      api.get<Page<{ project_id: string; project_name: string; workflow: WorkflowView }>>(
        `/v1/workflow-queue${qs({ status, project_id: project === "all" ? undefined : project })}`,
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

export type { PortalSession };
