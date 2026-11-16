import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { CheckCircle2, MessageSquareWarning, ShieldAlert, XCircle } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { api, describeError } from "@/lib/api";
import { useDecision, qk } from "@/hooks/queries";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/input";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { StatusBadge } from "@/components/StatusBadge";
import { approvalStatus } from "@/lib/status";
import { formatDateTime, shortHash } from "@/lib/format";
import type { ApprovalDocument, Decision, PortalSession } from "@/lib/types";

export const decisionSchema = z
  .object({
    decision: z.enum(["approve", "request_changes", "reject"]),
    comments: z.string().max(4000, "Comments are limited to 4000 characters"),
  })
  .superRefine((v, ctx) => {
    if (v.decision !== "approve" && v.comments.trim().length < 3) {
      ctx.addIssue({
        code: "custom",
        path: ["comments"],
        message: v.decision === "reject" ? "Explain why the document is rejected." : "Describe the changes you need.",
      });
    }
  });

type FormValues = z.infer<typeof decisionSchema>;

const LABEL: Record<Decision, string> = { approve: "Approve", request_changes: "Request changes", reject: "Reject" };
const VARIANT = { approve: "success", request_changes: "warning", reject: "destructive" } as const;
const CONSEQUENCE: Record<Decision, string> = {
  approve: "The workflow engine resumes and continues to the next stage.",
  request_changes: "The work returns to its producing stage for revision (bounded by the gate's revision limit); history is kept.",
  reject: "This is a final rejection: the workflow follows its rejection path.",
};

/** Pre-checks mirror the backend's rules for guidance only; the backend decides. */
function blockers(doc: ApprovalDocument, session: PortalSession): string[] {
  const out: string[] = [];
  const ap = session.approver;
  if (!ap) return out;
  if (ap.kind !== "human") out.push("Approvals need a human approver identity.");
  if (!ap.roles.includes(doc.approval.required_role)) out.push(`This gate requires the '${doc.approval.required_role}' role; ${ap.principal} holds ${ap.roles.join(", ") || "no roles"}.`);
  if (doc.approval.authors.includes(ap.principal)) out.push(`${ap.principal} contributed to this document and cannot approve it (separation of duties).`);
  return out;
}

export function DecisionPanel({ doc, session }: { doc: ApprovalDocument; session: PortalSession }) {
  const a = doc.approval;
  const qc = useQueryClient();
  const mutation = useDecision();
  const [confirm, setConfirm] = useState<FormValues | null>(null);
  const [stale, setStale] = useState<string | null>(null);
  const form = useForm<FormValues>({ resolver: zodResolver(decisionSchema), defaultValues: { decision: "approve", comments: "" } });
  const reviewedHash = doc.document?.artifact.content_hash ?? a.artifact_hash;

  if (a.status !== "pending") {
    const d = a.decision;
    return (
      <div className="flex flex-col gap-2 text-sm">
        <div className="flex items-center gap-2">
          <StatusBadge info={approvalStatus(a.status)} />
          {a.decided_at && <span className="text-xs text-muted-foreground">{formatDateTime(a.decided_at)}</span>}
        </div>
        {d ? (
          <dl className="grid gap-1 text-xs">
            <div>
              <dt className="inline text-muted-foreground">Approver: </dt>
              <dd className="inline">
                {d.approver_id} ({d.approver_role})
              </dd>
            </div>
            <div>
              <dt className="inline text-muted-foreground">Artifact hash: </dt>
              <dd className="inline font-mono">{shortHash(d.artifact_hash)}</dd>
            </div>
            {d.comments && <dd className="mt-1 rounded-md bg-muted/50 p-2 whitespace-pre-wrap">{d.comments}</dd>}
          </dl>
        ) : (
          a.status === "superseded" && <p className="text-xs text-muted-foreground">A newer request replaced this one (for example after a revision).</p>
        )}
        {mutation.isSuccess && (
          <p role="status" className="flex items-center gap-1.5 rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 text-xs text-emerald-900">
            <CheckCircle2 className="size-4" /> Decision recorded by the approval service.
          </p>
        )}
      </div>
    );
  }

  // Portal decisions: workflow document gates (Requirements, HLD, LLD) and skills, whose
  // full definition is shown (ADR-0028). Mandatory standards are reviewed with the CLI.
  const isSkill = a.subject_type === "skill" && !!doc.skill;
  if (!isSkill && (!a.workflow || !doc.document)) {
    return (
      <div className="rounded-md border border-slate-200 bg-muted/40 p-3 text-sm">
        <p className="font-medium">Review this {a.subject_type.replaceAll("_", " ")} with the CLI</p>
        <p className="mt-1 text-xs text-muted-foreground">
          The portal decides workflow documents (Requirements, HLD, LLD) and skills. Use <code className="font-mono">eaeh approval review --as &lt;approver-profile&gt;</code>, which shows the full content first.
        </p>
      </div>
    );
  }

  if (!session.can_decide || !session.approver) {
    return (
      <div className="rounded-md border border-slate-200 bg-muted/40 p-3 text-sm">
        <p className="font-medium">Read-only portal</p>
        <p className="mt-1 text-xs text-muted-foreground">
          Decisions are recorded only as an explicitly named approver. Restart with <code className="font-mono">eaeh portal --approver &lt;approver-profile&gt;</code>, or use{" "}
          <code className="font-mono">eaeh approval decide</code>.
        </p>
      </div>
    );
  }

  const blocked = blockers(doc, session);
  const open = (decision: Decision) => {
    form.setValue("decision", decision);
    setStale(null);
    void form.handleSubmit((v) => setConfirm(v))();
  };

  const submit = async () => {
    if (!confirm) return;
    // Stale-document protection: re-read the request and the document right before deciding.
    try {
      const fresh = await api.get<ApprovalDocument>(`/v1/approvals/${encodeURIComponent(a.id)}/document`);
      const freshHash = fresh.document?.artifact.content_hash ?? fresh.approval.artifact_hash;
      if (fresh.approval.status !== "pending" || freshHash !== reviewedHash) {
        setStale(
          fresh.approval.status !== "pending"
            ? `This request is now '${fresh.approval.status}'. Reload to see the current state.`
            : "The document changed since you opened it. Reload and review the new version before deciding.",
        );
        setConfirm(null);
        void qc.invalidateQueries({ queryKey: qk.approvalDoc(a.id) });
        return;
      }
    } catch (e) {
      setStale(describeError(e).detail);
      setConfirm(null);
      return;
    }
    mutation.mutate(
      { id: a.id, decision: confirm.decision, comments: confirm.comments.trim(), artifact_hash: reviewedHash },
      { onSettled: () => setConfirm(null) },
    );
  };

  const err = mutation.isError ? describeError(mutation.error) : null;
  return (
    <form className="flex flex-col gap-3" onSubmit={(e) => e.preventDefault()} aria-label="Approval decision">
      <p className="text-xs text-muted-foreground">
        Deciding as <b className="text-foreground">{session.approver.principal}</b> on {isSkill ? `skill ${doc.skill!.reference}` : `version v${doc.document?.artifact.version ?? "?"}`} ({isSkill ? "digest" : "hash"}{" "}
        <span className="font-mono">{shortHash(reviewedHash)}</span>).
      </p>
      {blocked.length > 0 && (
        <div className="rounded-md border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900" role="note">
          <div className="mb-1 flex items-center gap-1.5 font-medium">
            <ShieldAlert className="size-4" /> The approval service will refuse this decision
          </div>
          <ul className="list-disc pl-4">
            {blocked.map((b) => (
              <li key={b}>{b}</li>
            ))}
          </ul>
        </div>
      )}
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="comments">Reviewer comments</Label>
        <Textarea id="comments" placeholder="Required for request changes and rejection" {...form.register("comments")} aria-invalid={!!form.formState.errors.comments} />
        {form.formState.errors.comments && (
          <p className="text-xs text-red-700" role="alert">
            {form.formState.errors.comments.message}
          </p>
        )}
      </div>
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <Button type="button" variant="success" disabled={mutation.isPending || blocked.length > 0} onClick={() => open("approve")}>
          <CheckCircle2 /> Approve
        </Button>
        {!isSkill && <Button type="button" variant="warning" disabled={mutation.isPending || blocked.length > 0} onClick={() => open("request_changes")}>
          <MessageSquareWarning /> Request changes
        </Button>}
        <Button type="button" variant="destructive" disabled={mutation.isPending || blocked.length > 0} onClick={() => open("reject")}>
          <XCircle /> Reject
        </Button>
      </div>
      {stale && (
        <p role="alert" className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-900">
          {stale}
        </p>
      )}
      {err && (
        <div role="alert" className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-900">
          <p className="font-medium capitalize">Decision not recorded · {err.title}</p>
          <p>{err.detail}</p>
          {err.hint && <p className="mt-1">{err.hint}</p>}
        </div>
      )}
      <AlertDialog open={!!confirm} onOpenChange={(o) => !o && setConfirm(null)}>
        <AlertDialogContent>
          {confirm && (
            <>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  {LABEL[confirm.decision]} {isSkill ? `skill ${doc.skill!.reference}` : `${a.document_type} v${doc.document?.artifact.version ?? "?"}`}?
                </AlertDialogTitle>
                <AlertDialogDescription>{CONSEQUENCE[confirm.decision]}</AlertDialogDescription>
              </AlertDialogHeader>
              <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded-md bg-muted/50 p-3 text-xs">
                <dt className="text-muted-foreground">Project</dt>
                <dd>{a.project_name}</dd>
                <dt className="text-muted-foreground">Gate</dt>
                <dd>{a.gate ?? a.subject_type}</dd>
                <dt className="text-muted-foreground">Artifact hash</dt>
                <dd className="font-mono break-all">{reviewedHash}</dd>
                <dt className="text-muted-foreground">Approver</dt>
                <dd>{session.approver?.principal}</dd>
                {confirm.comments.trim() && (
                  <>
                    <dt className="text-muted-foreground">Comments</dt>
                    <dd className="whitespace-pre-wrap">{confirm.comments.trim()}</dd>
                  </>
                )}
              </dl>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  variant={VARIANT[confirm.decision]}
                  onClick={(e) => {
                    e.preventDefault();
                    void submit();
                  }}
                  disabled={mutation.isPending}
                >
                  Confirm {LABEL[confirm.decision].toLowerCase()}
                </AlertDialogAction>
              </AlertDialogFooter>
            </>
          )}
        </AlertDialogContent>
      </AlertDialog>
    </form>
  );
}
