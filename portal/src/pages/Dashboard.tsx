import { Link, useNavigate } from "react-router";
import { AlertOctagon, ArrowRight, CheckCircle2, CheckSquare, Cpu, FolderKanban, GitBranch, RefreshCw } from "lucide-react";
import { useDashboard, useDefinitions } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { InstallationPanel } from "@/components/InstallationPanel";
import { StatCard } from "@/components/StatCard";
import { StatusBadge } from "@/components/StatusBadge";
import { ActivityList } from "@/components/ActivityList";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { approvalStatus, taskStatus, workflowStatus } from "@/lib/status";
import { formatDateTime, formatDuration, formatRelative, humanize, shortHash, shortId } from "@/lib/format";
import type { Dashboard as DashboardData, WorkflowDefinition, WorkflowView } from "@/lib/types";
import { progress } from "@/lib/lifecycle";

/** Stage k of n along the definition's main path, plus approved gates. */
function stageInfo(w: WorkflowView, defs: WorkflowDefinition[] | undefined): string {
  const def = defs?.find((d) => d.id === w.definition_id && d.version === w.definition_version);
  const p = progress(def, w);
  const gates = Object.keys(w.approved_gates).length;
  const g = `${gates} gate${gates === 1 ? "" : "s"} approved`;
  return p ? `Stage ${p.index} of ${p.total} · ${g}` : g;
}

function InProgress({ data }: { data: DashboardData }) {
  const navigate = useNavigate();
  const defs = useDefinitions();
  if (!data.active.length) {
    return (
      <EmptyState title="No engineering work in progress">
        Start a workflow with <code className="font-mono">eaeh workflow run requirements|design --yes</code>; it appears here while it runs.
      </EmptyState>
    );
  }
  return (
    <Table aria-label="In-progress engineering work">
      <TableHeader>
        <TableRow>
          <TableHead>Project</TableHead>
          <TableHead>Workflow</TableHead>
          <TableHead>Lifecycle stage</TableHead>
          <TableHead>Task / agent</TableHead>
          <TableHead>Status</TableHead>
          <TableHead>Waiting on</TableHead>
          <TableHead>Started</TableHead>
          <TableHead>Updated</TableHead>
          <TableHead className="text-right">Duration</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {data.active.map(({ project_name, workflow: w, tasks }) => {
          const st = workflowStatus(w.status, w.blocked);
          const task = tasks.find((t) => t.state === w.state) ?? tasks[0];
          return (
            <TableRow
              key={w.id}
              className="cursor-pointer"
              tabIndex={0}
              onClick={() => navigate(`/workflows/${w.id}`)}
              onKeyDown={(e) => {
                if (e.key === "Enter") navigate(`/workflows/${w.id}`);
              }}
              aria-label={`Open workflow ${w.definition_id} ${w.id}`}
            >
              <TableCell className="font-medium">{project_name}</TableCell>
              <TableCell>
                <div className="font-medium">{w.definition_id}</div>
                <div className="font-mono text-xs text-muted-foreground">…{shortId(w.id)}</div>
              </TableCell>
              <TableCell>
                <div>{humanize(w.state)}</div>
                <div className="text-xs text-muted-foreground">{stageInfo(w, defs.data?.items)}</div>
              </TableCell>
              <TableCell>
                {task ? (
                  <>
                    <div className="flex items-center gap-1.5">
                      {humanize(task.name)}
                      <StatusBadge info={taskStatus(task.status)} className="scale-90" />
                    </div>
                    <div className="text-xs text-muted-foreground">{task.agent ?? (task.kind === "tool" ? "platform tool" : "—")}</div>
                  </>
                ) : (
                  <span className="text-muted-foreground">—</span>
                )}
              </TableCell>
              <TableCell>
                <StatusBadge info={st} />
              </TableCell>
              <TableCell className="text-xs">{st.waitingOn ?? "—"}</TableCell>
              <TableCell className="text-xs whitespace-nowrap">{formatDateTime(w.created_at)}</TableCell>
              <TableCell className="text-xs whitespace-nowrap" title={formatDateTime(w.updated_at)}>
                {formatRelative(w.updated_at)}
              </TableCell>
              <TableCell className="text-right text-xs tabular-nums">{formatDuration(w.created_at)}</TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}

function PendingApprovals({ data }: { data: DashboardData }) {
  if (!data.pending_approvals.length) return <EmptyState title="No approvals waiting">Requirements, HLD and LLD gates appear here when a workflow reaches them.</EmptyState>;
  return (
    <Table aria-label="Pending approvals">
      <TableHeader>
        <TableRow>
          <TableHead>Document</TableHead>
          <TableHead>Project</TableHead>
          <TableHead>Workflow</TableHead>
          <TableHead>Version</TableHead>
          <TableHead>Approver role</TableHead>
          <TableHead>Status</TableHead>
          <TableHead>Requested</TableHead>
          <TableHead />
        </TableRow>
      </TableHeader>
      <TableBody>
        {data.pending_approvals.map((a) => (
          <TableRow key={a.id}>
            <TableCell className="font-medium">{a.document_type}</TableCell>
            <TableCell>{a.project_name}</TableCell>
            <TableCell className="text-xs">
              {a.workflow ? (
                <Link className="hover:underline" to={`/workflows/${a.workflow.id}`}>
                  {a.workflow.definition_id} · …{shortId(a.workflow.id)}
                </Link>
              ) : (
                a.subject_type
              )}
            </TableCell>
            <TableCell className="text-xs">
              {a.artifact ? `v${a.artifact.version}` : "—"} <span className="font-mono text-muted-foreground">{shortHash(a.artifact_hash)}</span>
            </TableCell>
            <TableCell className="text-xs capitalize">{a.required_role}</TableCell>
            <TableCell>
              <StatusBadge info={approvalStatus(a.status)} />
            </TableCell>
            <TableCell className="text-xs whitespace-nowrap" title={formatDateTime(a.created_at)}>
              {formatRelative(a.created_at)}
            </TableCell>
            <TableCell className="text-right">
              <Button asChild size="sm" variant="outline">
                <Link to={`/approvals/${a.id}`}>
                  Review <ArrowRight />
                </Link>
              </Button>
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

export function DashboardPage() {
  const q = useDashboard();
  const d = q.data;
  return (
    <>
      <PageHeader
        title="Dashboard"
        description="Live state of engineering workflows and human approval gates across your projects."
        actions={
          <>
            {d && <span className="text-xs text-muted-foreground">Updated {formatRelative(d.generated_at)} · refreshes every 10 s</span>}
            <Button size="sm" variant="outline" onClick={() => void q.refetch()} disabled={q.isFetching}>
              <RefreshCw className={q.isFetching ? "animate-spin" : undefined} /> Refresh
            </Button>
          </>
        }
      />
      {q.isError && !d && <ErrorState error={q.error} onRetry={() => void q.refetch()} />}
      {q.isError && d && (
        <div className="mb-4">
          <ErrorState error={q.error} />
        </div>
      )}
      {q.isPending && <LoadingRows rows={6} label="Loading dashboard" />}
      {d && (
        <div className="flex flex-col gap-6">
          <InstallationPanel projectCount={d.projects.length} compact />
          <section aria-label="Overview" className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
            <StatCard label="Projects" value={d.summary.projects} icon={<FolderKanban />} to="/projects" />
            <StatCard
              label="Workflows in progress"
              value={d.summary.in_progress}
              icon={<GitBranch />}
              to="/workflows?status=active"
              hint={`${d.summary.waiting_external} waiting for AI · ${d.summary.waiting_approval} for approval`}
              accent="bg-sky-50 text-sky-700"
            />
            <StatCard label="Tasks executing" value={d.summary.executing_tasks} icon={<Cpu />} accent="bg-violet-50 text-violet-700" hint="running or handed off to AI" />
            <StatCard label="Pending approvals" value={d.summary.pending_approvals} icon={<CheckSquare />} to="/approvals" accent="bg-amber-50 text-amber-700" />
            <StatCard label="Completed workflows" value={d.summary.completed} icon={<CheckCircle2 />} to="/workflows?status=all" accent="bg-emerald-50 text-emerald-700" />
            <StatCard label="Failed or blocked" value={d.summary.failed_or_blocked} icon={<AlertOctagon />} to="/workflows?status=all" accent="bg-red-50 text-red-700" />
          </section>

          <Card>
            <CardHeader>
              <CardTitle>In-progress engineering work</CardTitle>
              <CardDescription>Select a row to open its lifecycle view. Waiting-for-AI work is handed off to your connected AI client; waiting-for-approval work needs a human decision.</CardDescription>
            </CardHeader>
            <CardContent className="px-0 pb-1">
              <InProgress data={d} />
            </CardContent>
          </Card>

          <div className="grid gap-6 xl:grid-cols-5">
            <Card className="xl:col-span-3">
              <CardHeader className="flex-row items-center justify-between">
                <div>
                  <CardTitle>Pending approvals</CardTitle>
                  <CardDescription>Across all projects you can see.</CardDescription>
                </div>
                <Button asChild size="sm" variant="ghost">
                  <Link to="/approvals">
                    All approvals <ArrowRight />
                  </Link>
                </Button>
              </CardHeader>
              <CardContent className="px-0 pb-1">
                <PendingApprovals data={d} />
              </CardContent>
            </Card>
            <Card className="xl:col-span-2">
              <CardHeader className="flex-row items-center justify-between">
                <div>
                  <CardTitle>Recent activity</CardTitle>
                  <CardDescription>From recorded workflow transitions.</CardDescription>
                </div>
                <Button asChild size="sm" variant="ghost">
                  <Link to="/activity">
                    View all <ArrowRight />
                  </Link>
                </Button>
              </CardHeader>
              <CardContent>
                <ActivityList items={d.activity.slice(0, 12)} />
              </CardContent>
            </Card>
          </div>
          {d.projects.some((p) => p.restricted) && (
            <p className="text-xs text-muted-foreground">
              Some projects are hidden from these figures because the viewing identity lacks <code>workflow.read</code> / <code>approval.read</code> there.
            </p>
          )}
        </div>
      )}
    </>
  );
}
