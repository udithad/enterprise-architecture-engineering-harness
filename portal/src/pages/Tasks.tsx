import { useState } from "react";
import { Link, useSearchParams } from "react-router";
import { useDefinitions, useTaskQueue } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { ProjectFilter } from "@/components/ProjectFilter";
import { StatusBadge } from "@/components/StatusBadge";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { taskStatus } from "@/lib/status";
import { formatDateTime, formatRelative, humanize, shortId } from "@/lib/format";

/** Task statuses recorded by the engine (task table). */
const TASK_STATUSES = ["pending", "running", "awaiting_external", "succeeded", "failed", "interrupted", "escalated", "cancelled"];

export function TasksPage() {
  const [params, setParams] = useSearchParams();
  const get = (k: string) => params.get(k) ?? "";
  const project = params.get("project") ?? "all";
  const [cursors, setCursors] = useState<string[]>([]);
  const cursor = cursors[cursors.length - 1];
  const since = get("since");
  const q = useTaskQueue(
    { project, workflow: get("workflow") || undefined, stage: get("stage") || undefined, status: get("status") || undefined, since: since ? new Date(since).toISOString() : undefined, cursor },
    50,
  );
  const defs = useDefinitions();
  const set = (k: string, v: string) => {
    const n = new URLSearchParams(params);
    if (v && v !== "all") n.set(k, v);
    else n.delete(k);
    setParams(n, { replace: true });
    setCursors([]);
  };
  const stages = [...new Set((defs.data?.items ?? []).filter((d) => !get("workflow") || d.id === get("workflow")).flatMap((d) => d.states.map((s) => s.name)))].sort();
  const items = q.data?.items ?? [];

  return (
    <>
      <PageHeader title="Tasks" description="Task executions across your projects, newest first, from the engine's task records." />
      <div className="mb-4 flex flex-wrap items-end gap-3" role="search" aria-label="Task filters">
        <div className="flex flex-col gap-1">
          <Label>Project</Label>
          <ProjectFilter value={project} onChange={(v) => set("project", v)} />
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="f-workflow">Workflow</Label>
          <select id="f-workflow" className="h-9 rounded-md border bg-background px-2 text-sm" value={get("workflow")} onChange={(e) => set("workflow", e.target.value)}>
            <option value="">All workflows</option>
            {[...new Set((defs.data?.items ?? []).map((d) => d.id))].map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="f-stage">Stage</Label>
          <select id="f-stage" className="h-9 rounded-md border bg-background px-2 text-sm" value={get("stage")} onChange={(e) => set("stage", e.target.value)}>
            <option value="">All stages</option>
            {stages.map((s) => (
              <option key={s} value={s}>
                {humanize(s)}
              </option>
            ))}
          </select>
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="f-status">Task status</Label>
          <select id="f-status" className="h-9 rounded-md border bg-background px-2 text-sm" value={get("status")} onChange={(e) => set("status", e.target.value)}>
            <option value="">Any status</option>
            {TASK_STATUSES.map((s) => (
              <option key={s} value={s}>
                {taskStatus(s).label}
              </option>
            ))}
          </select>
        </div>
        <div className="flex flex-col gap-1">
          <Label htmlFor="f-since">Updated since</Label>
          <Input id="f-since" type="date" className="w-40" value={since} onChange={(e) => set("since", e.target.value)} />
        </div>
      </div>
      <Card className="overflow-hidden">
        {q.isPending && <div className="p-4"><LoadingRows /></div>}
        {q.isError && <div className="p-4"><ErrorState error={q.error} onRetry={() => void q.refetch()} /></div>}
        {q.data && !items.length && <div className="p-4"><EmptyState title="No tasks match these filters" /></div>}
        {!!items.length && (
          <Table aria-label="Tasks">
            <TableHeader>
              <TableRow>
                <TableHead>Task</TableHead>
                <TableHead>Project</TableHead>
                <TableHead>Stage</TableHead>
                <TableHead>Agent</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Workflow</TableHead>
                <TableHead>Started</TableHead>
                <TableHead>Updated</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {items.map(({ task: t, project_name, project_id }) => (
                <TableRow key={t.id}>
                  <TableCell>
                    <Link className="font-medium hover:underline" to={`/tasks/${t.id}`}>
                      {humanize(t.name)}
                    </Link>
                    {t.error && <div className="max-w-xs truncate text-xs text-red-700" title={t.error}>{t.error}</div>}
                  </TableCell>
                  <TableCell>
                    <Link className="hover:underline" to={`/projects/${project_id}`}>
                      {project_name}
                    </Link>
                  </TableCell>
                  <TableCell>{humanize(t.state)}</TableCell>
                  <TableCell className="text-xs">{t.agent ?? (t.kind === "tool" ? "platform tool" : "—")}</TableCell>
                  <TableCell>
                    <StatusBadge info={taskStatus(t.status)} />
                  </TableCell>
                  <TableCell>
                    <Link className="font-mono text-xs hover:underline" to={`/workflows/${t.workflow_instance_id}`}>
                      …{shortId(t.workflow_instance_id)}
                    </Link>
                  </TableCell>
                  <TableCell className="text-xs whitespace-nowrap">{formatDateTime(t.created_at)}</TableCell>
                  <TableCell className="text-xs whitespace-nowrap">{formatRelative(t.updated_at)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>
      <div className="mt-3 flex gap-2">
        <Button size="sm" variant="outline" disabled={!cursors.length} onClick={() => setCursors(cursors.slice(0, -1))}>
          Newer
        </Button>
        <Button size="sm" variant="outline" disabled={!q.data?.next_cursor} onClick={() => q.data?.next_cursor && setCursors([...cursors, q.data.next_cursor])}>
          Older
        </Button>
      </div>
    </>
  );
}
