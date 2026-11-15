import { Link } from "react-router";
import { ACTIVITY_LABELS } from "@/lib/status";
import { formatDateTime, formatRelative, humanize, shortId } from "@/lib/format";
import type { ActivityItem } from "@/lib/types";
import { cn } from "@/lib/utils";
import { EmptyState } from "./States";

const dot: Record<string, string> = {
  workflow_started: "bg-sky-500",
  task_completed: "bg-slate-400",
  artifact_generated: "bg-indigo-500",
  approval_requested: "bg-amber-500",
  approval_granted: "bg-emerald-500",
  approval_rejected: "bg-red-500",
  changes_requested: "bg-orange-500",
  stage_failed: "bg-red-500",
  workflow_completed: "bg-emerald-600",
  workflow_failed: "bg-red-600",
  workflow_cancelled: "bg-slate-500",
};

export function describeActivity(a: ActivityItem): string {
  switch (a.kind) {
    case "workflow_started":
      return `${a.definition_id} started`;
    case "approval_requested":
      return `${humanize(a.state)} is waiting for a decision`;
    case "approval_granted":
    case "approval_rejected":
    case "changes_requested":
      return `${humanize(a.state)} → ${humanize(a.to_state)}`;
    case "artifact_generated":
    case "task_completed":
      return `${humanize(a.state)} finished`;
    case "stage_failed":
      return `${humanize(a.state)} failed`;
    default:
      return a.to_state ? `${humanize(a.state)} → ${humanize(a.to_state)}` : humanize(a.state);
  }
}

export function ActivityList({ items, showProject = true, empty = "No workflow activity yet." }: { items: ActivityItem[]; showProject?: boolean; empty?: string }) {
  if (!items.length) return <EmptyState title={empty} />;
  return (
    <ol className="flex flex-col" aria-label="Recent activity">
      {items.map((a, i) => (
        <li key={`${a.workflow_instance_id}-${a.kind}-${a.occurred_at}-${i}`} className="flex gap-3 border-b py-2.5 last:border-0">
          <span className={cn("mt-1.5 size-2 shrink-0 rounded-full", dot[a.kind] ?? "bg-slate-300")} aria-hidden />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-baseline gap-x-2">
              <span className="text-sm font-medium">{ACTIVITY_LABELS[a.kind] ?? humanize(a.kind)}</span>
              <span className="truncate text-sm text-muted-foreground">{describeActivity(a)}</span>
            </div>
            <div className="mt-0.5 flex flex-wrap gap-x-2 text-xs text-muted-foreground">
              <time dateTime={a.occurred_at} title={formatDateTime(a.occurred_at)}>
                {formatRelative(a.occurred_at)}
              </time>
              {showProject && a.project_name && <span>· {a.project_name}</span>}
              <span>· by {a.actor_id}</span>
              <Link className="underline-offset-2 hover:underline" to={`/workflows/${a.workflow_instance_id}`}>
                · workflow …{shortId(a.workflow_instance_id)}
              </Link>
            </div>
          </div>
        </li>
      ))}
    </ol>
  );
}
