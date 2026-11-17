import { useState } from "react";
import { Link, useParams } from "react-router";
import { ArrowRight, Bot, CheckCircle2, Circle, FileText, Loader2, UserCheck, XCircle } from "lucide-react";
import { useWorkflowDetail } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { StatusBadge } from "@/components/StatusBadge";
import { CopyText } from "@/components/CopyText";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { approvalStatus, taskStatus, workflowStatus } from "@/lib/status";
import { formatDateTime, formatDuration, humanize, shortHash } from "@/lib/format";
import { buildStages, STAGE_LABEL, stateKindLabel, type Stage, type StageStatus } from "@/lib/lifecycle";
import { cn } from "@/lib/utils";
import { CompletionSeal, GovernanceCard } from "@/components/GovernanceCard";
import { WorkspaceDocuments } from "@/components/WorkspaceDocuments";
import { GenerationBadge, ReplayNotice } from "@/components/Generation";
import { WorkflowOperations } from "@/components/WorkflowOperations";
import type { Generation, Lineage } from "@/lib/types";

function LineageCard({ lineage }: { lineage?: Lineage }) {
  if (!lineage || (!lineage.derived_from && !lineage.derived_runs.length)) return null;
  return (
    <Card data-testid="lineage">
      <CardHeader>
        <CardTitle>Lineage</CardTitle>
        <CardDescription>Restarts create new runs; the original runs, their artifacts and approvals are kept.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-2 text-sm">
        {lineage.derived_from && (
          <div>
            Restarted from{" "}
            <Link className="font-mono underline" to={`/workflows/${lineage.derived_from.source}`}>
              {lineage.derived_from.source}
            </Link>{" "}
            at <b>{humanize(lineage.derived_from.stage)}</b> by {lineage.derived_from.actor} · {formatDateTime(lineage.derived_from.at)}
            {lineage.derived_from.input_changed && " · new input"}
            <div className="text-xs text-muted-foreground">Reason: {lineage.derived_from.reason}</div>
          </div>
        )}
        {lineage.derived_runs.map((r) => (
          <div key={r.instance}>
            Superseded by{" "}
            <Link className="font-mono underline" to={`/workflows/${r.instance}`}>
              {r.instance}
            </Link>{" "}
            (restart at <b>{humanize(r.stage)}</b> by {r.actor} · {formatDateTime(r.at)})
            <div className="text-xs text-muted-foreground">Reason: {r.reason}</div>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

const stageStyle: Record<StageStatus, { ring: string; icon: React.ReactNode }> = {
  completed: { ring: "border-emerald-300 bg-emerald-50 text-emerald-700", icon: <CheckCircle2 className="size-4" /> },
  active: { ring: "border-sky-300 bg-sky-50 text-sky-700", icon: <Loader2 className="size-4 animate-spin" /> },
  awaiting_approval: { ring: "border-amber-300 bg-amber-50 text-amber-800", icon: <UserCheck className="size-4" /> },
  awaiting_ai: { ring: "border-violet-300 bg-violet-50 text-violet-700", icon: <Bot className="size-4" /> },
  failed: { ring: "border-red-300 bg-red-50 text-red-700", icon: <XCircle className="size-4" /> },
  rejected: { ring: "border-red-300 bg-red-50 text-red-700", icon: <XCircle className="size-4" /> },
  skipped: { ring: "border-slate-200 bg-slate-50 text-slate-500", icon: <Circle className="size-4" /> },
  not_started: { ring: "border-slate-200 bg-card text-slate-400", icon: <Circle className="size-4" /> },
};

function Lifecycle({ stages, selected, onSelect }: { stages: Stage[]; selected: string | null; onSelect: (s: string) => void }) {
  return (
    <ol className="flex flex-wrap items-stretch gap-y-3" aria-label="Workflow lifecycle">
      {stages.map((s, i) => {
        const st = stageStyle[s.status];
        const isSel = selected === s.def.name;
        return (
          <li key={s.def.name} className="flex items-center">
            <button
              type="button"
              onClick={() => onSelect(s.def.name)}
              aria-pressed={isSel}
              aria-label={`${humanize(s.def.name)}: ${STAGE_LABEL[s.status]}`}
              className={cn(
                "flex w-40 flex-col gap-1 rounded-md border px-3 py-2 text-left transition-shadow hover:shadow-sm",
                st.ring,
                isSel && "ring-2 ring-ring ring-offset-1",
                s.def.type === "approval_gate" && "border-dashed",
              )}
            >
              <span className="flex items-center gap-1.5 text-xs font-semibold">
                {st.icon}
                <span className="truncate">{humanize(s.def.name)}</span>
              </span>
              <span className="truncate text-[11px] opacity-80">{s.def.type === "approval_gate" ? "Approval gate" : s.def.type === "agent_task" ? "Agent task" : s.def.type === "terminal" ? "End" : "Platform step"}</span>
              <span className="text-[11px] font-medium">{STAGE_LABEL[s.status]}{s.revisions ? ` · ${s.revisions} revision${s.revisions > 1 ? "s" : ""}` : ""}</span>
            </button>
            {i < stages.length - 1 && <ArrowRight className={cn("mx-1 size-3.5 shrink-0", s.onMainPath ? "text-slate-300" : "text-transparent")} aria-hidden />}
          </li>
        );
      })}
    </ol>
  );
}

function StageDetail({ stage, workflowId, projectId, generation }: { stage: Stage; workflowId: string; projectId: string; generation?: Generation }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{humanize(stage.def.name)}</CardTitle>
        <CardDescription>{stateKindLabel(stage.def)}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4 text-sm">
        <dl className="grid grid-cols-2 gap-x-6 gap-y-2 md:grid-cols-4">
          <div>
            <dt className="text-xs text-muted-foreground">Status</dt>
            <dd>{STAGE_LABEL[stage.status]}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">Entered</dt>
            <dd>{formatDateTime(stage.enteredAt)}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">Left</dt>
            <dd>{formatDateTime(stage.leftAt)}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">Duration</dt>
            <dd>{stage.enteredAt ? formatDuration(stage.enteredAt, stage.leftAt) : "—"}</dd>
          </div>
        </dl>
        {stage.artifact && (
          <div className="flex flex-wrap items-center gap-2 rounded-md border bg-muted/30 px-3 py-2">
            <FileText className="size-4 text-muted-foreground" aria-hidden />
            <span className="font-medium">{stage.artifact.name}</span>
            <span className="text-xs text-muted-foreground">v{stage.artifact.version}</span>
            <CopyText value={stage.artifact.hash} display={shortHash(stage.artifact.hash)} />
            {generation && <GenerationBadge g={generation} />}
            <Button asChild size="sm" variant="ghost" className="ml-auto">
              <Link to={`/projects/${projectId}?tab=artifacts`}>Project artifacts</Link>
            </Button>
          </div>
        )}
        {stage.def.type === "approval_gate" && (
          <div className="flex flex-wrap items-center gap-2 rounded-md border px-3 py-2">
            <UserCheck className="size-4 text-muted-foreground" aria-hidden />
            <span>Reviews the output of <b>{humanize(stage.def.artifact_from)}</b> · role <b>{stage.def.required_role}</b></span>
            {stage.approval ? (
              <>
                <StatusBadge info={approvalStatus(stage.approval.status)} />
                <Button asChild size="sm" variant={stage.approval.status === "pending" ? "default" : "outline"} className="ml-auto">
                  <Link to={`/approvals/${stage.approval.id}`}>{stage.approval.status === "pending" ? "Review document" : "View decision"}</Link>
                </Button>
              </>
            ) : (
              <span className="text-xs text-muted-foreground">No approval requested yet.</span>
            )}
          </div>
        )}
        {stage.tasks.length > 0 ? (
          <Table aria-label="Task executions">
            <TableHeader>
              <TableRow>
                <TableHead>Task</TableHead>
                <TableHead>Agent</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Attempt</TableHead>
                <TableHead>Started</TableHead>
                <TableHead>Updated</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {stage.tasks.map((t) => (
                <TableRow key={t.id}>
                  <TableCell>
                    <Link className="hover:underline" to={`/tasks/${t.id}`}>
                      {humanize(t.name)}
                    </Link>
                    <div className="font-mono text-[11px] text-muted-foreground">{t.id}</div>
                    {t.error && <div className="mt-1 text-xs text-red-700">{t.error}</div>}
                  </TableCell>
                  <TableCell className="text-xs">{t.agent ?? (t.kind === "tool" ? "platform tool" : "—")}</TableCell>
                  <TableCell>
                    <StatusBadge info={taskStatus(t.status)} />
                  </TableCell>
                  <TableCell className="text-xs">{t.attempt}</TableCell>
                  <TableCell className="text-xs whitespace-nowrap">{formatDateTime(t.created_at)}</TableCell>
                  <TableCell className="text-xs whitespace-nowrap">{formatDateTime(t.updated_at)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ) : (
          stage.def.type !== "approval_gate" && stage.def.type !== "terminal" && <p className="text-xs text-muted-foreground">No task execution recorded for this stage.</p>
        )}
        <p className="text-xs text-muted-foreground">
          Workflow <span className="font-mono">{workflowId}</span>
        </p>
      </CardContent>
    </Card>
  );
}

export function WorkflowDetailPage() {
  const { id = "" } = useParams();
  const q = useWorkflowDetail(id);
  const [selected, setSelected] = useState<string | null>(null);

  if (q.isPending) return <LoadingRows rows={6} label="Loading workflow" />;
  if (q.isError) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  const d = q.data;
  const w = d.workflow;
  const st = workflowStatus(w.status, w.blocked);
  const stages = d.definition ? buildStages(d.definition, w, d.history, d.tasks, d.approvals) : [];
  const current = selected ?? w.state;
  const stage = stages.find((s) => s.def.name === current);
  const pending = d.approvals?.find((a) => a.status === "pending");

  return (
    <>
      <PageHeader
        eyebrow={
          <Link className="hover:underline" to={`/projects/${d.project.id}`}>
            {d.project.name}
          </Link>
        }
        title={
          <span className="flex items-center gap-3">
            {w.definition_id} <StatusBadge info={st} />
            {d.governance && <CompletionSeal gov={d.governance} status={w.status} />}
          </span>
        }
        description={d.definition?.description}
        actions={
          pending && (
            <Button asChild>
              <Link to={`/approvals/${pending.id}`}>
                <UserCheck /> Review {pending.document_type}
              </Link>
            </Button>
          )
        }
      />
      <div className="flex flex-col gap-6">
        <Card>
          <CardContent className="grid grid-cols-2 gap-x-6 gap-y-3 pt-5 text-sm md:grid-cols-3 xl:grid-cols-6">
            <div>
              <div className="text-xs text-muted-foreground">Instance</div>
              <CopyText value={w.id} />
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Definition</div>
              {w.definition_id}@{w.definition_version}
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Current stage</div>
              {humanize(w.state)}
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Waiting on</div>
              {st.waitingOn ?? "—"}
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Started</div>
              {formatDateTime(w.created_at)} <span className="text-xs text-muted-foreground">by {w.started_by}</span>
            </div>
            <div>
              <div className="text-xs text-muted-foreground">Duration</div>
              {formatDuration(w.created_at, ["running", "waiting_approval", "waiting_external"].includes(w.status) ? null : w.updated_at)}
            </div>
          </CardContent>
        </Card>
        <ReplayNotice provenance={d.artifact_provenance} />
        {w.status === "waiting_external" && (
          <div className="rounded-md border border-violet-200 bg-violet-50 px-4 py-3 text-sm text-violet-900">
            <b>{humanize(w.awaiting_external ?? w.state)}</b> is handed off to your connected AI client (execution mode <code>{d.execution_mode}</code>). It continues when the client or the background worker submits a result that passes validation. Open the task (stage below) for its hand-off status, attempts and Retry.
          </div>
        )}
        {w.blocked && (
          <div role="alert" className="rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-900" data-testid="blocked-alert">
            <b>Blocked at {humanize(w.blocked.gate)}:</b> the document is approved, but the approved version could not be written to the project workspace. {w.blocked.reason}
            <div className="mt-1 text-xs">The workflow continues only after the documents are written. Fix the cause, then run <code>eaeh artifact sync</code> in the project repository.</div>
          </div>
        )}
        {w.failure && (
          <div role="alert" className="rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-900">
            <b>Failure:</b> {w.failure}
            <div className="mt-1 text-xs">Recovery: use <b>Retry failed stage</b> above. It previews a new linked run that re-executes the failed stage and reuses the completed ones; this run stays as it is.</div>
          </div>
        )}
        <WorkflowOperations w={w} ops={d.operations} />
        <LineageCard lineage={d.lineage} />
        <Card>
          <CardHeader>
            <CardTitle>Lifecycle</CardTitle>
            <CardDescription>Built from the workflow definition and its recorded transitions. Select a stage for its tasks, artifact and approval.</CardDescription>
          </CardHeader>
          <CardContent>
            {stages.length ? <Lifecycle stages={stages} selected={current} onSelect={setSelected} /> : <EmptyState title="Workflow definition unavailable" />}
          </CardContent>
        </Card>
        {stage && <StageDetail stage={stage} workflowId={w.id} projectId={d.project.id} generation={d.artifact_provenance?.[stage.def.name]} />}
        {d.documents && <WorkspaceDocuments docs={d.documents} workspace={d.workspace} />}
        {d.governance && <GovernanceCard gov={d.governance} status={w.status} pendingGate={w.status === "waiting_approval" ? w.state : null} />}
        <Card>
          <CardHeader>
            <CardTitle>Transition history</CardTitle>
          </CardHeader>
          <CardContent className="px-0">
            {d.history.length ? (
              <Table aria-label="Transition history">
                <TableHeader>
                  <TableRow>
                    <TableHead>#</TableHead>
                    <TableHead>From</TableHead>
                    <TableHead>To</TableHead>
                    <TableHead>Trigger</TableHead>
                    <TableHead>Actor</TableHead>
                    <TableHead>At</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {d.history.map((h) => (
                    <TableRow key={h.seq}>
                      <TableCell className="text-xs tabular-nums">{h.seq}</TableCell>
                      <TableCell>{humanize(h.from_state)}</TableCell>
                      <TableCell>{humanize(h.to_state)}</TableCell>
                      <TableCell className="text-xs">{humanize(h.trigger)}</TableCell>
                      <TableCell className="text-xs">{h.actor_id}</TableCell>
                      <TableCell className="text-xs whitespace-nowrap">{formatDateTime(h.occurred_at)}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            ) : (
              <div className="px-5">
                <EmptyState title="No transitions recorded yet" />
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </>
  );
}
