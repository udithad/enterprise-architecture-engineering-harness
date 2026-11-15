import { useMemo, useState } from "react";
import { useSearchParams } from "react-router";
import { ChevronLeft, ChevronRight, Search } from "lucide-react";
import { useActivity, useAudit } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { ProjectFilter } from "@/components/ProjectFilter";
import { ActivityList } from "@/components/ActivityList";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ACTIVITY_LABELS } from "@/lib/status";
import { formatDateTime } from "@/lib/format";
import { ApiError } from "@/lib/api";
import { cn } from "@/lib/utils";

function WorkflowActivity() {
  const [project, setProject] = useState("all");
  const [kind, setKind] = useState("all");
  const q = useActivity(project);
  const items = useMemo(() => (q.data?.items ?? []).filter((a) => kind === "all" || a.kind === kind), [q.data, kind]);
  return (
    <Card>
      <CardHeader>
        <CardTitle>Workflow activity</CardTitle>
        <CardDescription>Workflow starts and recorded stage transitions, newest first (workflow.read).</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="mb-3 flex flex-wrap gap-2">
          <ProjectFilter value={project} onChange={setProject} />
          <Select value={kind} onValueChange={setKind}>
            <SelectTrigger className="w-52" aria-label="Filter by event type">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All event types</SelectItem>
              {Object.entries(ACTIVITY_LABELS).map(([k, l]) => (
                <SelectItem key={k} value={k}>
                  {l}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        {q.isPending && <LoadingRows />}
        {q.isError && <ErrorState error={q.error} onRetry={() => void q.refetch()} />}
        {q.data && <ActivityList items={items} empty={kind === "all" ? "No workflow activity yet." : "No events of this type."} />}
      </CardContent>
    </Card>
  );
}

function AuditLog() {
  const [cursors, setCursors] = useState<number[]>([]);
  const before = cursors.length ? cursors[cursors.length - 1]! : null;
  const q = useAudit(before);
  const [search, setSearch] = useState("");
  const items = useMemo(() => {
    const s = search.trim().toLowerCase();
    return (q.data?.items ?? []).filter((r) => !s || [r.action, r.actor_id, r.resource_type, r.resource_id, r.outcome, r.details].some((x) => x.toLowerCase().includes(s)));
  }, [q.data, search]);
  const forbidden = q.error instanceof ApiError && q.error.status === 403;
  return (
    <Card>
      <CardHeader>
        <CardTitle>Audit log</CardTitle>
        <CardDescription>Tamper-evident audit records of the tenant, newest first. Requires audit.read (administrator or auditor).</CardDescription>
      </CardHeader>
      <CardContent>
        {forbidden ? (
          <EmptyState title="The viewing identity cannot read the audit log">
            audit.read is held by administrators and auditors. Start the portal with <code>--as &lt;auditor-profile&gt;</code> to see it, or use <code>eaeh audit</code>.
          </EmptyState>
        ) : (
          <>
            <div className="mb-3 flex flex-wrap items-center gap-2">
              <div className="relative w-full sm:w-72">
                <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" aria-hidden />
                <Input className="pl-8" placeholder="Filter this page…" value={search} onChange={(e) => setSearch(e.target.value)} aria-label="Filter audit records" />
              </div>
              <div className="ml-auto flex gap-1">
                <Button size="sm" variant="outline" disabled={!cursors.length} onClick={() => setCursors((c) => c.slice(0, -1))}>
                  <ChevronLeft /> Newer
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!q.data?.next_cursor}
                  onClick={() => q.data?.next_cursor && setCursors((c) => [...c, Number(q.data!.next_cursor)])}
                >
                  Older <ChevronRight />
                </Button>
              </div>
            </div>
            {q.isPending && <LoadingRows />}
            {q.isError && <ErrorState error={q.error} onRetry={() => void q.refetch()} />}
            {q.data && !items.length && <EmptyState title="No audit records" />}
            {!!items.length && (
              <Table aria-label="Audit records">
                <TableHeader>
                  <TableRow>
                    <TableHead>#</TableHead>
                    <TableHead>Time</TableHead>
                    <TableHead>Action</TableHead>
                    <TableHead>Actor</TableHead>
                    <TableHead>Resource</TableHead>
                    <TableHead>Outcome</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {items.map((r) => (
                    <TableRow key={r.seq}>
                      <TableCell className="text-xs tabular-nums">{r.seq}</TableCell>
                      <TableCell className="text-xs whitespace-nowrap">{formatDateTime(r.occurred_at)}</TableCell>
                      <TableCell className="font-mono text-xs">{r.action}</TableCell>
                      <TableCell className="text-xs">
                        {r.actor_id} <span className="text-muted-foreground">({r.actor_kind})</span>
                      </TableCell>
                      <TableCell className="max-w-64 truncate text-xs" title={`${r.resource_type} ${r.resource_id}`}>
                        {r.resource_type} <span className="font-mono text-muted-foreground">{r.resource_id}</span>
                      </TableCell>
                      <TableCell>
                        <span className={cn("rounded px-1.5 py-0.5 text-xs", r.outcome === "success" ? "bg-emerald-50 text-emerald-800" : r.outcome === "denied" ? "bg-red-50 text-red-800" : "bg-slate-100")}>{r.outcome}</span>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}

export function ActivityPage() {
  const [params, setParams] = useSearchParams();
  const tab = params.get("tab") === "audit" ? "audit" : "workflow";
  return (
    <>
      <PageHeader title="Activity / Audit" description="What happened, from workflow records and the audit log." />
      <Tabs value={tab} onValueChange={(v) => setParams({ tab: v }, { replace: true })}>
        <TabsList>
          <TabsTrigger value="workflow">Workflow activity</TabsTrigger>
          <TabsTrigger value="audit">Audit log</TabsTrigger>
        </TabsList>
        <TabsContent value="workflow">
          <WorkflowActivity />
        </TabsContent>
        <TabsContent value="audit">
          <AuditLog />
        </TabsContent>
      </Tabs>
    </>
  );
}
