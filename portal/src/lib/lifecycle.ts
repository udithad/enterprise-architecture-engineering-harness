// Lifecycle model built from the workflow's own definition and execution records – no
// workflow-specific assumptions. The "main path" follows each state's success transition
// (`succeeded`, or `approved` for gates) from the initial state.

import type { ApprovalView, StateDef, TaskSummary, Transition, WorkflowDefinition, WorkflowView } from "./types";

export type StageStatus = "completed" | "active" | "awaiting_approval" | "awaiting_ai" | "failed" | "rejected" | "skipped" | "not_started";

export interface Stage {
  def: StateDef;
  status: StageStatus;
  enteredAt: string | null;
  leftAt: string | null;
  visits: number;
  tasks: TaskSummary[];
  artifact: WorkflowView["artifacts"][string] | null;
  approval: ApprovalView | null;
  onMainPath: boolean;
  /** For gates: number of `changes_requested` revisions honoured. */
  revisions: number;
}

export function mainPath(def: WorkflowDefinition): StateDef[] {
  const byName = new Map(def.states.map((s) => [s.name, s]));
  const out: StateDef[] = [];
  const seen = new Set<string>();
  let cur = byName.get(def.initial);
  while (cur && !seen.has(cur.name)) {
    seen.add(cur.name);
    if (cur.type === "terminal") break;
    out.push(cur);
    const next = cur.transitions["succeeded"] ?? cur.transitions["approved"];
    cur = next ? byName.get(next) : undefined;
  }
  return out;
}

/** "Stage 3 of 11" style progress along the main path, or null when not on it. */
export function progress(def: WorkflowDefinition | undefined, w: WorkflowView): { index: number; total: number } | null {
  if (!def) return null;
  const path = mainPath(def);
  const i = path.findIndex((s) => s.name === w.state);
  if (w.status === "succeeded") return { index: path.length, total: path.length };
  return i >= 0 ? { index: i + 1, total: path.length } : null;
}

export function buildStages(
  def: WorkflowDefinition,
  w: WorkflowView,
  history: Transition[],
  tasks: TaskSummary[],
  approvals: ApprovalView[] | null,
): Stage[] {
  const path = mainPath(def);
  const onPath = new Set(path.map((s) => s.name));
  const visited = new Set<string>([...history.map((h) => h.from_state), ...history.map((h) => h.to_state), w.state]);
  // Off-path states that were actually reached (e.g. a failure terminal) are appended.
  const extra = def.states.filter((s) => !onPath.has(s.name) && visited.has(s.name));
  const all = [...path, ...extra];
  const terminalFailed = ["failed", "escalated", "cancelled"].includes(w.status);

  return all.map((s) => {
    const entered = history.filter((h) => h.to_state === s.name);
    const left = history.filter((h) => h.from_state === s.name);
    const isCurrent = w.state === s.name;
    const stTasks = tasks.filter((t) => t.state === s.name);
    const lastLeft = left[left.length - 1];
    const gateApprovals = (approvals ?? []).filter((a) => a.gate === s.name);
    const approval = gateApprovals.find((a) => a.status === "pending") ?? gateApprovals[0] ?? null;

    let status: StageStatus = "not_started";
    if (isCurrent) {
      if (s.type === "terminal") status = s.outcome === "succeeded" ? "completed" : "failed";
      else if (w.status === "waiting_approval") status = "awaiting_approval";
      else if (w.status === "waiting_external") status = "awaiting_ai";
      else if (terminalFailed) status = "failed";
      else if (w.status === "succeeded") status = "completed";
      else status = "active";
    } else if (lastLeft) {
      if (lastLeft.trigger === "failed" || lastLeft.trigger === "timed_out") status = "failed";
      else if (lastLeft.trigger === "rejected") status = "rejected";
      else status = "completed";
    } else if (entered.length) {
      status = "completed";
    } else if (s.type === "tool_step" && s.name === def.initial && history.length) {
      status = "completed";
    }
    if (status === "not_started" && w.status === "succeeded" && onPath.has(s.name)) status = "completed";

    return {
      def: s,
      status,
      enteredAt: entered[0]?.occurred_at ?? (s.name === def.initial ? w.created_at : null),
      leftAt: lastLeft?.occurred_at ?? null,
      visits: w.visits[s.name] ?? entered.length,
      tasks: stTasks,
      artifact: w.artifacts[s.name] ?? null,
      approval,
      onMainPath: onPath.has(s.name),
      revisions: w.revisions[s.name] ?? 0,
    };
  });
}

export const STAGE_LABEL: Record<StageStatus, string> = {
  completed: "Completed",
  active: "In progress",
  awaiting_approval: "Waiting for approval",
  awaiting_ai: "Waiting for AI",
  failed: "Failed",
  rejected: "Rejected",
  skipped: "Skipped",
  not_started: "Not started",
};

export function stateKindLabel(s: StateDef): string {
  switch (s.type) {
    case "agent_task":
      return `Agent task${s.task_kind ? ` · ${s.task_kind}` : ""}`;
    case "tool_step":
      return `Platform tool${s.tool ? ` · ${s.tool}` : ""}`;
    case "planned":
      return "Planned (hybrid) step";
    case "approval_gate":
      return `Approval gate · ${s.required_role ?? "approver"}`;
    case "terminal":
      return `End · ${s.outcome ?? ""}`;
  }
}
