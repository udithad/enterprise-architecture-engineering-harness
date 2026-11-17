import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { History, Play, RotateCcw, Square } from "lucide-react";
import { useCancel, useRestart, useRestartPreview, useResume, useSession } from "@/hooks/queries";
import { describeError } from "@/lib/api";
import { humanize, shortHash } from "@/lib/format";
import type { RestartPlan, StagePlan, WorkflowOperations as Ops, WorkflowView } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { cn } from "@/lib/utils";

const DISPOSITION: Record<StagePlan["disposition"], { label: string; cls: string }> = {
  carried: { label: "Reused as is", cls: "bg-emerald-50 text-emerald-800 border-emerald-200" },
  restart_point: { label: "Restart point", cls: "bg-sky-50 text-sky-800 border-sky-300" },
  rerun: { label: "Re-executed · new version", cls: "bg-amber-50 text-amber-900 border-amber-200" },
  pending: { label: "Not reached", cls: "bg-slate-50 text-slate-600 border-slate-200" },
};

const APPROVAL: Record<string, string> = {
  carried: "Approval stays valid",
  repeat: "Approval must be repeated",
  required: "Approval required",
};

function ErrorLine({ e }: { e: unknown }) {
  if (!e) return null;
  const d = describeError(e);
  return (
    <p role="alert" className="text-sm text-red-700">
      {d.detail}
      {d.hint && <span className="block text-xs">{d.hint}</span>}
    </p>
  );
}

/** The engine's preview, stage by stage (nothing is written until confirmed). */
export function RestartPreview({ plan }: { plan: RestartPlan }) {
  return (
    <div className="flex flex-col gap-3 text-sm" data-testid="restart-preview">
      <ol className="flex flex-col gap-1" aria-label="Restart plan">
        {plan.stages.map((s) => (
          <li key={s.name} className={cn("flex flex-wrap items-center gap-2 rounded border px-2 py-1", DISPOSITION[s.disposition].cls)}>
            <span className="w-44 truncate font-medium">{humanize(s.name)}</span>
            <span className="text-xs">{DISPOSITION[s.disposition].label}</span>
            {s.artifact && (
              <span className="text-xs text-muted-foreground">
                current {s.artifact.name} v{s.artifact.version} · {shortHash(s.artifact.hash)}
              </span>
            )}
            {s.approval && <span className="ml-auto text-xs font-medium">{APPROVAL[s.approval] ?? s.approval}</span>}
          </li>
        ))}
      </ol>
      <dl className="grid grid-cols-1 gap-1 text-xs md:grid-cols-2">
        <div>
          <dt className="text-muted-foreground">Approvals kept</dt>
          <dd>{plan.carried_approvals.length ? plan.carried_approvals.map(humanize).join(", ") : "none"}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Approvals to repeat</dt>
          <dd>{plan.repeat_approvals.length ? plan.repeat_approvals.map(humanize).join(", ") : "none"}</dd>
        </div>
        <div className="md:col-span-2">
          <dt className="text-muted-foreground">Workspace documents</dt>
          <dd>{plan.workspace_effect}</dd>
        </div>
      </dl>
      {plan.source_will_be_cancelled && (
        <p className="rounded border border-amber-200 bg-amber-50 px-2 py-1 text-xs text-amber-900">
          This run is still active. It will be cancelled as <b>superseded</b>; its history, artifacts and approvals are kept unchanged.
        </p>
      )}
      <p className="text-xs text-muted-foreground">
        A new run is created and linked to this one. Approved versions are never overwritten; re-executed stages produce new versions that need new approvals.
      </p>
    </div>
  );
}

function RestartDialog({ w, open, onOpenChange }: { w: WorkflowView; open: boolean; onOpenChange: (o: boolean) => void }) {
  const preview = useRestartPreview();
  const restart = useRestart();
  const navigate = useNavigate();
  const [stage, setStage] = useState<string>("");
  const [reason, setReason] = useState("");
  const [input, setInput] = useState("");
  const [inputError, setInputError] = useState<string | null>(null);
  const plan = preview.data;
  const { mutate: previewMutate, isIdle } = preview;

  // The preview is computed as soon as the dialog opens (default restart point).
  useEffect(() => {
    if (open && isIdle) previewMutate({ id: w.id });
  }, [open, isIdle, previewMutate, w.id]);

  const run = (s?: string) => {
    let parsed: unknown;
    if (input.trim()) {
      try {
        parsed = JSON.parse(input);
        setInputError(null);
      } catch {
        setInputError("The new input must be JSON, e.g. {\"brd\": \"source/subject@v2\"}");
        return;
      }
    }
    preview.mutate({ id: w.id, stage: s || undefined, input: parsed });
  };

  return (
    <AlertDialog
      open={open}
      onOpenChange={(o) => {
        onOpenChange(o);
        if (!o) {
          preview.reset();
          restart.reset();
          setStage("");
          setReason("");
          setInput("");
        }
      }}
    >
      <AlertDialogContent className="max-h-[90vh] max-w-3xl overflow-y-auto">
        <AlertDialogHeader>
          <AlertDialogTitle>Restart from a stage</AlertDialogTitle>
          <AlertDialogDescription>
            Retry a failed stage, re-execute a stage, or re-run from an earlier stage after a change. Only stages of this workflow definition that do not skip a required approval can be chosen.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-end gap-2">
            <div className="flex flex-col gap-1">
              <Label htmlFor="restart-stage">Restart point</Label>
              <select
                id="restart-stage"
                className="h-9 rounded-md border bg-background px-2 text-sm"
                value={stage || plan?.stage || ""}
                onChange={(e) => {
                  setStage(e.target.value);
                  run(e.target.value);
                }}
              >
                {(plan?.allowed_stages ?? []).map((s) => (
                  <option key={s} value={s}>
                    {humanize(s)}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex min-w-64 flex-1 flex-col gap-1">
              <Label htmlFor="restart-input">New input (optional, JSON; restarts from the first stage)</Label>
              <Input id="restart-input" value={input} onChange={(e) => setInput(e.target.value)} placeholder='{"brd": "project-brd/brd@v2"}' />
            </div>
            <Button variant="outline" size="sm" onClick={() => run(stage || plan?.stage)} disabled={preview.isPending}>
              <History /> Preview
            </Button>
          </div>
          {inputError && <p className="text-sm text-red-700">{inputError}</p>}
          <ErrorLine e={preview.error} />
          {preview.isPending && <p className="text-sm text-muted-foreground">Computing the plan…</p>}
          {plan && <RestartPreview plan={plan} />}
          {plan && (
            <div className="flex flex-col gap-1">
              <Label htmlFor="restart-reason">Reason (recorded in lineage and audit)</Label>
              <Input id="restart-reason" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="e.g. architecture changed: switch to event sourcing" />
            </div>
          )}
          <ErrorLine e={restart.error} />
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel>Close</AlertDialogCancel>
          <Button
            disabled={!plan || !reason.trim() || restart.isPending || preview.isPending}
            onClick={() => {
              if (!plan) return;
              let parsed: unknown;
              if (input.trim()) parsed = JSON.parse(input);
              restart.mutate(
                { id: w.id, stage: plan.stage, fingerprint: plan.fingerprint, reason: reason.trim(), input: parsed },
                {
                  onSuccess: (r) => {
                    onOpenChange(false);
                    navigate(`/workflows/${r.id}`);
                  },
                },
              );
            }}
          >
            <RotateCcw /> Confirm restart from {plan ? humanize(plan.stage) : "…"}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/**
 * Resume / Cancel / Restart for a run. Shown only when the portal runs with `--operator`;
 * the backend authorizes every call and the engine decides what is possible.
 */
export function WorkflowOperations({ w, ops }: { w: WorkflowView; ops?: Ops }) {
  const session = useSession();
  const resume = useResume();
  const cancel = useCancel();
  const [restartOpen, setRestartOpen] = useState(false);
  const [cancelOpen, setCancelOpen] = useState(false);
  if (!ops) return null;
  if (!session.data?.can_operate) {
    return (
      <p className="text-xs text-muted-foreground" data-testid="ops-hidden">
        Operations (resume, cancel, restart) need the portal to run with <code>--operator &lt;profile&gt;</code>.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2" role="toolbar" aria-label="Workflow operations">
        {ops.resume && (
          <Button size="sm" variant="outline" onClick={() => resume.mutate(w.id)} disabled={resume.isPending}>
            <Play /> Resume
          </Button>
        )}
        {ops.restart && (
          <Button size="sm" variant="outline" onClick={() => setRestartOpen(true)}>
            <RotateCcw /> {w.status === "failed" ? "Retry failed stage…" : "Restart from stage…"}
          </Button>
        )}
        {ops.cancel && (
          <Button size="sm" variant="outline" className="text-red-700" onClick={() => setCancelOpen(true)}>
            <Square /> Cancel
          </Button>
        )}
        {resume.isSuccess && <span className="text-xs text-muted-foreground">Resumed from its persisted state.</span>}
      </div>
      <ErrorLine e={resume.error ?? cancel.error} />
      <RestartDialog w={w} open={restartOpen} onOpenChange={setRestartOpen} />
      <AlertDialog open={cancelOpen} onOpenChange={setCancelOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Cancel this workflow?</AlertDialogTitle>
            <AlertDialogDescription>
              The run stops at {humanize(w.state)}. Its history, artifacts and approvals are kept, and it can be restarted later as a new linked run.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep running</AlertDialogCancel>
            <Button
              variant="destructive"
              onClick={() => cancel.mutate(w.id, { onSettled: () => setCancelOpen(false) })}
              disabled={cancel.isPending}
            >
              Cancel workflow
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
