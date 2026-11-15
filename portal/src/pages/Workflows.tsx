import { useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { Search } from "lucide-react";
import { useDefinitions, useWorkflowQueue } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { ProjectFilter } from "@/components/ProjectFilter";
import { StatusBadge } from "@/components/StatusBadge";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { workflowStatus } from "@/lib/status";
import { formatDateTime, formatDuration, formatRelative, humanize, shortId } from "@/lib/format";
import { progress } from "@/lib/lifecycle";

export function WorkflowsPage() {
  const [params, setParams] = useSearchParams();
  const status = params.get("status") === "all" ? "all" : "active";
  const project = params.get("project") ?? "all";
  const [search, setSearch] = useState("");
  const q = useWorkflowQueue(status, project);
  const defs = useDefinitions();
  const navigate = useNavigate();

  const items = useMemo(() => {
    const s = search.trim().toLowerCase();
    return (q.data?.items ?? []).filter(
      ({ workflow: w, project_name }) =>
        !s || [w.id, w.definition_id, w.state, w.status, project_name, w.started_by].some((x) => x.toLowerCase().includes(s)),
    );
  }, [q.data, search]);

  const set = (k: string, v: string) => {
    const n = new URLSearchParams(params);
    n.set(k, v);
    setParams(n, { replace: true });
  };

  return (
    <>
      <PageHeader title="Workflows" description="Workflow instances and their current lifecycle stage." />
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <Tabs value={status} onValueChange={(v) => set("status", v)}>
          <TabsList>
            <TabsTrigger value="active">In progress</TabsTrigger>
            <TabsTrigger value="all">All</TabsTrigger>
          </TabsList>
        </Tabs>
        <ProjectFilter value={project} onChange={(v) => set("project", v)} />
        <div className="relative ml-auto w-full sm:w-72">
          <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" aria-hidden />
          <Input className="pl-8" placeholder="Search id, workflow, stage, project…" value={search} onChange={(e) => setSearch(e.target.value)} aria-label="Search workflows" />
        </div>
      </div>
      <Card className="overflow-hidden">
        {q.isPending && <div className="p-4"><LoadingRows /></div>}
        {q.isError && <div className="p-4"><ErrorState error={q.error} onRetry={() => void q.refetch()} /></div>}
        {q.data && !items.length && (
          <div className="p-4">
            <EmptyState title={search ? "No workflows match your search" : status === "active" ? "No workflows in progress" : "No workflows yet"} />
          </div>
        )}
        {!!items.length && (
          <Table aria-label="Workflows">
            <TableHeader>
              <TableRow>
                <TableHead>Workflow</TableHead>
                <TableHead>Project</TableHead>
                <TableHead>Stage</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Started by</TableHead>
                <TableHead>Started</TableHead>
                <TableHead>Updated</TableHead>
                <TableHead className="text-right">Duration</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {items.map(({ workflow: w, project_name }) => {
                const def = defs.data?.items.find((d) => d.id === w.definition_id && d.version === w.definition_version);
                const p = progress(def, w);
                const terminal = !["running", "waiting_approval", "waiting_external"].includes(w.status);
                return (
                  <TableRow
                    key={w.id}
                    className="cursor-pointer"
                    tabIndex={0}
                    onClick={() => navigate(`/workflows/${w.id}`)}
                    onKeyDown={(e) => e.key === "Enter" && navigate(`/workflows/${w.id}`)}
                  >
                    <TableCell>
                      <div className="font-medium">{w.definition_id}</div>
                      <div className="font-mono text-xs text-muted-foreground">…{shortId(w.id)} · v{w.definition_version}</div>
                    </TableCell>
                    <TableCell>{project_name}</TableCell>
                    <TableCell>
                      {humanize(w.state)}
                      {p && <div className="text-xs text-muted-foreground">Stage {p.index} of {p.total}</div>}
                    </TableCell>
                    <TableCell>
                      <StatusBadge info={workflowStatus(w.status, w.blocked)} />
                    </TableCell>
                    <TableCell className="text-xs">{w.started_by}</TableCell>
                    <TableCell className="text-xs whitespace-nowrap">{formatDateTime(w.created_at)}</TableCell>
                    <TableCell className="text-xs whitespace-nowrap">{formatRelative(w.updated_at)}</TableCell>
                    <TableCell className="text-right text-xs tabular-nums">{formatDuration(w.created_at, terminal ? w.updated_at : null)}</TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </Card>
    </>
  );
}
