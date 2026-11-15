import type { WorkflowStatus } from "./types";

export type Tone = "running" | "ai" | "approval" | "success" | "failed" | "neutral" | "warning";

export interface StatusInfo {
  label: string;
  tone: Tone;
  /** What the work is waiting on, for in-progress items. */
  waitingOn?: "External AI" | "Human approval" | "System processing" | "Human action";
}

/** `blocked` – the run is held at an approved gate because the approved documents could
 * not be written to the project workspace (`WorkflowView.blocked`). */
export function workflowStatus(s: WorkflowStatus | string, blocked?: unknown): StatusInfo {
  if (s === "running" && blocked) return { label: "Blocked", tone: "failed", waitingOn: "Human action" };
  switch (s) {
    case "running":
      return { label: "Running", tone: "running", waitingOn: "System processing" };
    case "waiting_external":
      return { label: "Waiting for AI", tone: "ai", waitingOn: "External AI" };
    case "waiting_approval":
      return { label: "Waiting for approval", tone: "approval", waitingOn: "Human approval" };
    case "succeeded":
      return { label: "Completed", tone: "success" };
    case "failed":
      return { label: "Failed", tone: "failed" };
    case "escalated":
      return { label: "Escalated", tone: "failed" };
    case "cancelled":
      return { label: "Cancelled", tone: "neutral" };
    default:
      return { label: s, tone: "neutral" };
  }
}

export function projectStatus(s: string | undefined, restricted = false): StatusInfo {
  if (restricted) return { label: "Restricted", tone: "neutral" };
  switch (s) {
    case "ready":
      return { label: "Ready", tone: "neutral" };
    case "in_progress":
      return { label: "In progress", tone: "running", waitingOn: "System processing" };
    case "waiting_approval":
      return { label: "Waiting for approval", tone: "approval", waitingOn: "Human approval" };
    case "blocked":
      return { label: "Blocked", tone: "failed", waitingOn: "Human action" };
    case "completed":
      return { label: "Completed", tone: "success" };
    case "failed":
      return { label: "Failed", tone: "failed" };
    case "cancelled":
      return { label: "Cancelled", tone: "neutral" };
    default:
      return { label: s ?? "—", tone: "neutral" };
  }
}

export function taskStatus(s: string): StatusInfo {
  switch (s) {
    case "pending":
      return { label: "Queued", tone: "neutral", waitingOn: "System processing" };
    case "running":
      return { label: "Running", tone: "running", waitingOn: "System processing" };
    case "awaiting_external":
      return { label: "Waiting for AI", tone: "ai", waitingOn: "External AI" };
    case "succeeded":
      return { label: "Completed", tone: "success" };
    case "failed":
      return { label: "Failed", tone: "failed" };
    case "interrupted":
      return { label: "Interrupted", tone: "warning" };
    case "escalated":
      return { label: "Escalated", tone: "failed" };
    case "cancelled":
      return { label: "Cancelled", tone: "neutral" };
    default:
      return { label: s, tone: "neutral" };
  }
}

export function approvalStatus(s: string): StatusInfo {
  switch (s) {
    case "pending":
      return { label: "Pending", tone: "approval" };
    case "approved":
      return { label: "Approved", tone: "success" };
    case "rejected":
      return { label: "Rejected", tone: "failed" };
    case "changes_requested":
      return { label: "Changes requested", tone: "warning" };
    case "superseded":
      return { label: "Superseded", tone: "neutral" };
    default:
      return { label: s, tone: "neutral" };
  }
}

export const ACTIVE_STATUSES: WorkflowStatus[] = ["running", "waiting_external", "waiting_approval"];

export function isActive(s: string): boolean {
  return (ACTIVE_STATUSES as string[]).includes(s);
}

export const ACTIVITY_LABELS: Record<string, string> = {
  workflow_started: "Workflow started",
  task_completed: "Task completed",
  artifact_generated: "Artifact generated",
  approval_requested: "Approval requested",
  approval_granted: "Approval granted",
  approval_rejected: "Approval rejected",
  changes_requested: "Changes requested",
  stage_failed: "Stage failed",
  workflow_completed: "Workflow completed",
  workflow_failed: "Workflow failed",
  workflow_cancelled: "Workflow cancelled",
  transition: "Transition",
};
