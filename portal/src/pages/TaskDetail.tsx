import { Link, useParams } from "react-router";
import { RefreshCcw } from "lucide-react";
import { useSession, useTaskDetail, useTaskRetry } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { StatusBadge } from "@/components/StatusBadge";
import { CopyText } from "@/components/CopyText";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { taskStatus, workflowStatus } from "@/lib/status";
import { formatDateTime, humanize, shortHash } from "@/lib/format";
import { describeError } from "@/lib/api";

function Json({ value, label }: { value: unknown; label: string }) {
  if (value === null || value === undefined) return <p className="text-xs text-muted-foreground">No {label.toLowerCase()} recorded.</p>;
  return (
    <pre aria-label={label} className="max-h-80 overflow-auto rounded-md border bg-muted/30 p-3 text-xs">
      {JSON.stringify(value, null, 2)}
    </pre>
  );
}

export function TaskDetailPage() {
  const { id = "" } = useParams();
  const q = useTaskDetail(id);
  const session = useSession();
  const retry = useTaskRetry();
  if (q.isPending) return <LoadingRows rows={6} label="Loading task" />;
  if (q.isError) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  const d = q.data;
  const t = d.task;
  const canOperate = !!session.data?.can_operate;
  return (
    <>
      <PageHeader
        eyebrow={
          <span className="flex gap-1">
            <Link className="hover:underline" to={`/projects/${d.project}`}>
              Project
            </Link>
            ·
            <Link className="hover:underline" to={`/workflows/${d.workflow.id}`}>
              {d.workflow.definition}
            </Link>
          </span>
        }
        title={
          <span className="flex items-center gap-3">
            {humanize(t.name)} <StatusBadge info={taskStatus(t.status)} />
          </span>
        }
        description={`Stage ${humanize(t.state)} · ${t.kind === "agent" ? "agent task" : "platform tool"}`}
      />
      <div className="flex flex-col gap-6">
        <Card>
          <CardContent className="grid grid-cols-2 gap-x-6 gap-y-3 pt-5 text-sm md:grid-cols-3 xl:grid-cols-6">
            <div>
              <div className="text-xs text-muted-foreground">Task</div>
              <CopyText value={t.id} />
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Workflow</div>
              <Link className="hover:underline" to={`/workflows/${d.workflow.id}`}>
                {humanize(d.workflow.state)}
              </Link>{" "}
              <StatusBadge info={workflowStatus(d.workflow.status)} />
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Attempt</div>
              {t.attempt}
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Agent</div>
              {t.agent ?? "—"}
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Started</div>
              {formatDateTime(t.created_at)}
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Updated</div>
              {formatDateTime(t.updated_at)}
            </div>
          </CardContent>
        </Card>
        {t.error && (
          <div role="alert" className="rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-900">
            <b>Error:</b> {t.error}
          </div>
        )}
        <Card>
          <CardHeader>
            <CardTitle>Recovery</CardTitle>
            <CardDescription>What the backend supports for this task now.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-2 text-sm">
            {d.operations.retry_handoff ? (
              canOperate ? (
                <div className="flex flex-wrap items-center gap-2">
                  <Button size="sm" onClick={() => retry.mutate(t.id)} disabled={retry.isPending}>
                    <RefreshCcw /> Retry AI hand-off
                  </Button>
                  <span className="text-xs text-muted-foreground">The worker stopped on this hand-off ({d.handoff?.dispatch_state}); retry makes it eligible again.</span>
                </div>
              ) : (
                <p className="text-xs">The hand-off can be retried; start the portal with <code>--operator &lt;profile&gt;</code> to do it here.</p>
              )
            ) : (
              <p className="text-xs text-muted-foreground">No hand-off retry is needed.</p>
            )}
            <p className="text-xs">
              To re-execute this stage, open the{" "}
              <Link className="underline" to={`/workflows/${d.workflow.id}`}>
                workflow
              </Link>{" "}
              and use <b>Restart from stage</b> with <b>{humanize(d.operations.reexecute_stage)}</b>: it shows what is reused and what must be approved again before anything runs.
            </p>
            {retry.isError && <p className="text-sm text-red-700">{describeError(retry.error).detail}</p>}
            {retry.isSuccess && <p className="text-xs text-emerald-700">The hand-off is pending again.</p>}
          </CardContent>
        </Card>
        <div className="grid gap-6 lg:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle>Agent and skills</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-2 text-sm">
              {d.agent_definition ? (
                <>
                  <div>
                    <b>{d.agent_definition.reference}</b> · {d.agent_definition.status}
                  </div>
                  <div className="text-xs text-muted-foreground">{d.agent_definition.purpose}</div>
                  <div className="text-xs">Skills: {d.agent_definition.skills.length ? d.agent_definition.skills.join(", ") : "none"}</div>
                  <div className="text-xs">Capabilities: {d.agent_definition.capabilities.join(", ")}</div>
                  <div className="text-xs">Allowed tools: {d.agent_definition.allowed_tools.join(", ") || "none"}</div>
                </>
              ) : (
                <p className="text-xs text-muted-foreground">Platform step (no agent).</p>
              )}
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>Dependencies</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-1 text-sm">
              <div className="text-xs">Consumes outputs of: {d.depends_on.length ? d.depends_on.map(humanize).join(", ") : "the workflow input only"}</div>
              <div className="text-xs">Requires approval of: {d.requires_approval_of.length ? d.requires_approval_of.map(humanize).join(", ") : "—"}</div>
            </CardContent>
          </Card>
        </div>
        <Card>
          <CardHeader>
            <CardTitle>Output and artifact</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            {d.artifact ? (
              <div className="flex flex-wrap items-center gap-2 rounded-md border bg-muted/30 px-3 py-2">
                <b>{d.artifact.name}</b> v{d.artifact.version} <CopyText value={d.artifact.hash} display={shortHash(d.artifact.hash)} />
                {d.artifact.provenance?.models?.length ? <span className="text-xs text-muted-foreground">by {d.artifact.provenance.models.join(", ")}</span> : null}
                <Button asChild size="sm" variant="ghost" className="ml-auto">
                  <Link to={`/projects/${d.project}?tab=artifacts`}>Project artifacts</Link>
                </Button>
              </div>
            ) : (
              <p className="text-xs text-muted-foreground">This task produced no artifact.</p>
            )}
            <div>
              <div className="mb-1 text-xs font-medium">Validation</div>
              <Json value={d.quality} label="Validation results" />
            </div>
            <details>
              <summary className="cursor-pointer text-xs font-medium">Input</summary>
              <Json value={d.input} label="Input" />
            </details>
            <details>
              <summary className="cursor-pointer text-xs font-medium">Output</summary>
              <Json value={d.output} label="Output" />
            </details>
          </CardContent>
        </Card>
        {d.handoff && (
          <Card>
            <CardHeader>
              <CardTitle>External AI hand-off</CardTitle>
              <CardDescription>Executed by a connected AI client or the background worker on your subscription; EAEH validates the result.</CardDescription>
            </CardHeader>
            <CardContent className="grid grid-cols-2 gap-3 text-sm md:grid-cols-4">
              <div>
                <div className="text-xs text-muted-foreground">Status</div>
                {d.handoff.status}
              </div>
              <div>
                <div className="text-xs text-muted-foreground">Dispatch</div>
                {d.handoff.dispatch_state}
                {d.handoff.lease_owner && <span className="text-xs"> · {d.handoff.lease_owner}</span>}
              </div>
              <div>
                <div className="text-xs text-muted-foreground">Submissions</div>
                {d.handoff.submissions}
              </div>
              <div>
                <div className="text-xs text-muted-foreground">Last failure</div>
                {d.handoff.last_failure ?? "—"}
              </div>
              {!!d.handoff.feedback.length && (
                <div className="col-span-full text-xs">
                  Validation feedback: <ul className="list-disc pl-5">{d.handoff.feedback.map((f, i) => <li key={i}>{f}</li>)}</ul>
                </div>
              )}
            </CardContent>
          </Card>
        )}
        <Card>
          <CardHeader>
            <CardTitle>Execution attempts</CardTitle>
            <CardDescription>Recorded worker executions (detail is a redacted excerpt).</CardDescription>
          </CardHeader>
          <CardContent className="px-0">
            {d.executions.length ? (
              <Table aria-label="Execution attempts">
                <TableHeader>
                  <TableRow>
                    <TableHead>#</TableHead>
                    <TableHead>Outcome</TableHead>
                    <TableHead>Failure</TableHead>
                    <TableHead>Context delivery</TableHead>
                    <TableHead>Worker</TableHead>
                    <TableHead>Started</TableHead>
                    <TableHead>Detail</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {d.executions.map((e) => (
                    <TableRow key={e.id}>
                      <TableCell className="text-xs">{e.attempt}</TableCell>
                      <TableCell className="text-xs">{e.outcome}</TableCell>
                      <TableCell className="text-xs" title={e.remedy ?? ""}>{e.failure_class ?? "—"}</TableCell>
                      <TableCell className="text-xs" title={e.retrieval_status ?? ""}>
                        {e.delivery_status
                          ? `${e.delivery_status}${e.package_bytes ? ` · ${Math.round(e.package_bytes / 1024)} KB` : ""}${e.sections_total ? ` · ${e.sections_retrieved ?? 0}/${e.sections_total} sections` : ""}${e.acknowledged_at ? " · acknowledged" : ""}${e.denied_calls ? ` · ${e.denied_calls} denied` : ""}`
                          : "—"}
                      </TableCell>
                      <TableCell className="text-xs">{e.worker ?? "—"}</TableCell>
                      <TableCell className="text-xs whitespace-nowrap">{formatDateTime(e.started_at)}</TableCell>
                      <TableCell className="max-w-md truncate text-xs" title={`${e.detail ?? ""}${e.remedy ? "\n\nFix: " + e.remedy : ""}`}>
                        {e.detail ?? "—"}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            ) : (
              <div className="px-5">
                <EmptyState title="No worker execution recorded">Platform steps and interactively submitted tasks have no worker attempts.</EmptyState>
              </div>
            )}
            {!!d.attempts.length && (
              <div className="px-5 pt-3 text-xs">
                Other executions of this stage in the run:{" "}
                {d.attempts.map((a) => (
                  <Link key={a.id} className="mr-2 font-mono underline" to={`/tasks/${a.id}`}>
                    {a.id.slice(-8)} ({a.status})
                  </Link>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </>
  );
}
