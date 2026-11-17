import { Link, useSearchParams } from "react-router";
import { useExecutions, useIntegrations, useProjects } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { ProjectFilter } from "@/components/ProjectFilter";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatDateTime, formatRelative, humanize } from "@/lib/format";

/** Workers, hand-offs, execution attempts and the AI-client integration of a project (read-only). */
export function ExecutionPage() {
  const [params, setParams] = useSearchParams();
  const projects = useProjects();
  const project = params.get("project") ?? projects.data?.items[0]?.id ?? "all";
  const ex = useExecutions(project);
  const integ = useIntegrations(project);
  return (
    <>
      <PageHeader title="Execution and integrations" description="External AI execution through your connected clients (Claude Code, GitHub Copilot) on their own subscriptions. EAEH prepares, dispatches and validates; it makes no model calls in external_ai mode." />
      <div className="mb-4">
        <ProjectFilter value={project} onChange={(v) => setParams({ project: v }, { replace: true })} />
      </div>
      {project === "all" ? (
        <EmptyState title="Choose a project" />
      ) : (
        <div className="flex flex-col gap-6">
          <Card>
            <CardHeader>
              <CardTitle>Integration</CardTitle>
            </CardHeader>
            <CardContent className="text-sm">
              {integ.isPending && <LoadingRows rows={2} />}
              {integ.isError && <ErrorState error={integ.error} />}
              {integ.data && (
                <dl className="grid grid-cols-1 gap-2 md:grid-cols-3">
                  <div><dt className="text-xs text-muted-foreground">Installation execution mode</dt><dd>{integ.data.installation_execution_mode} <span className="text-xs text-muted-foreground">– {integ.data.billing}</span></dd></div>
                  <div><dt className="text-xs text-muted-foreground">Project clients</dt><dd>{integ.data.project_execution ? `${integ.data.project_execution.mode}: ${integ.data.project_execution.clients.join(", ")}` : "—"}</dd></div>
                  <div><dt className="text-xs text-muted-foreground">Direct model calls by EAEH</dt><dd>{integ.data.direct_model_calls ? "yes (direct_provider mode)" : "no"}</dd></div>
                  <div className="md:col-span-3"><dt className="text-xs text-muted-foreground">Client files in {integ.data.workspace ?? "the workspace"}</dt>
                    <dd className="flex flex-wrap gap-3 text-xs">{(integ.data.client_files ?? []).map((f) => <span key={f.file}>{f.present ? "✓" : "✗"} {f.file}</span>)}</dd></div>
                  {integ.data.issue && <div className="md:col-span-3 text-xs text-amber-800">{integ.data.issue}</div>}
                </dl>
              )}
            </CardContent>
          </Card>
          {ex.isError && <ErrorState error={ex.error} />}
          <Card>
            <CardHeader>
              <CardTitle>Workers</CardTitle>
              <CardDescription>Start and stop workers with <code>eaeh worker start|stop</code>; the portal starts one when you start a workflow.</CardDescription>
            </CardHeader>
            <CardContent className="px-0">
              {ex.data?.workers.length ? (
                <Table aria-label="Workers"><TableHeader><TableRow><TableHead>Worker</TableHead><TableHead>Status</TableHead><TableHead>Principal</TableHead><TableHead>Host / pid</TableHead><TableHead>Heartbeat</TableHead><TableHead>Detail</TableHead></TableRow></TableHeader>
                  <TableBody>{ex.data.workers.map((w) => (
                    <TableRow key={w.id}><TableCell className="font-mono text-xs">{w.id}</TableCell><TableCell className="text-xs">{w.status}{w.live ? " (live)" : ""}</TableCell><TableCell className="text-xs">{w.principal}</TableCell><TableCell className="text-xs">{w.host} / {w.pid}</TableCell><TableCell className="text-xs">{formatRelative(w.heartbeat_at)}</TableCell><TableCell className="text-xs">{w.detail ?? ""}</TableCell></TableRow>
                  ))}</TableBody></Table>
              ) : (
                <div className="px-5"><EmptyState title="No worker has run for this project" /></div>
              )}
            </CardContent>
          </Card>
          <Card>
            <CardHeader><CardTitle>AI hand-offs</CardTitle></CardHeader>
            <CardContent className="px-0">
              {ex.data?.handoffs.length ? (
                <Table aria-label="Hand-offs"><TableHeader><TableRow><TableHead>Task</TableHead><TableHead>Stage</TableHead><TableHead>Status</TableHead><TableHead>Dispatch</TableHead><TableHead>Attempts</TableHead><TableHead>Last failure</TableHead><TableHead>Updated</TableHead></TableRow></TableHeader>
                  <TableBody>{ex.data.handoffs.map((h) => (
                    <TableRow key={h.task_id}><TableCell><Link className="font-mono text-xs underline" to={`/tasks/${h.task_id}`}>{h.task_id.slice(-10)}</Link></TableCell><TableCell>{humanize(h.state)}</TableCell><TableCell className="text-xs">{h.status}</TableCell><TableCell className="text-xs">{h.dispatch_state}</TableCell><TableCell className="text-xs">{h.dispatch_attempts}</TableCell><TableCell className="text-xs">{h.last_failure ?? "—"}</TableCell><TableCell className="text-xs">{formatRelative(h.updated_at)}</TableCell></TableRow>
                  ))}</TableBody></Table>
              ) : (
                <div className="px-5"><EmptyState title="No hand-offs" /></div>
              )}
            </CardContent>
          </Card>
          <Card>
            <CardHeader><CardTitle>Execution attempts</CardTitle><CardDescription>Outcomes and failures recorded by workers; details are redacted excerpts. Full logs: <code>eaeh worker log</code>.</CardDescription></CardHeader>
            <CardContent className="px-0">
              {ex.isPending && <div className="px-5"><LoadingRows /></div>}
              {ex.data?.items.length ? (
                <Table aria-label="Execution attempts"><TableHeader><TableRow><TableHead>Task</TableHead><TableHead>Stage</TableHead><TableHead>#</TableHead><TableHead>Outcome</TableHead><TableHead>Failure</TableHead><TableHead>Context delivery</TableHead><TableHead>Started</TableHead><TableHead>Detail</TableHead></TableRow></TableHeader>
                  <TableBody>{ex.data.items.map((e) => (
                    <TableRow key={e.id}><TableCell><Link className="font-mono text-xs underline" to={`/tasks/${e.task_id}`}>{e.task_id.slice(-10)}</Link></TableCell><TableCell>{humanize(e.stage)}</TableCell><TableCell className="text-xs">{e.attempt}</TableCell><TableCell className="text-xs">{e.outcome}</TableCell><TableCell className="text-xs" title={e.remedy ?? ""}>{e.failure_class ?? "—"}</TableCell><TableCell className="text-xs" title={e.retrieval_status ?? ""}>{e.delivery_status ? `${e.delivery_status}${e.package_bytes ? ` · ${Math.round(e.package_bytes / 1024)} KB` : ""}${e.sections_total ? ` · ${e.sections_retrieved ?? 0}/${e.sections_total} sections` : ""}${e.acknowledged_at ? " · acknowledged" : ""}${e.denied_calls ? ` · ${e.denied_calls} denied` : ""}` : "—"}</TableCell><TableCell className="text-xs whitespace-nowrap">{formatDateTime(e.started_at)}</TableCell><TableCell className="max-w-md truncate text-xs" title={`${e.detail ?? ""}${e.remedy ? "\n\nFix: " + e.remedy : ""}`}>{e.detail ?? "—"}</TableCell></TableRow>
                  ))}</TableBody></Table>
              ) : (
                ex.data && <div className="px-5"><EmptyState title="No execution attempts recorded" /></div>
              )}
            </CardContent>
          </Card>
        </div>
      )}
    </>
  );
}
