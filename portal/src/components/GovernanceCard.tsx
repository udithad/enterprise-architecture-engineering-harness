import { Link } from "react-router";
import { CheckCircle2, CircleDashed, Hourglass, ShieldAlert, ShieldCheck } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatDateTime, humanize, shortHash } from "@/lib/format";
import type { Governance, WorkflowStatus } from "@/lib/types";

export type GateState = "verified" | "issues" | "awaiting" | "not_reached";

export function gateState(g: Governance["gates"][number], pendingGate: string | null): GateState {
  if (g.approval_id) return g.issues.length ? "issues" : "verified";
  return pendingGate === g.gate ? "awaiting" : "not_reached";
}

const LABEL: Record<GateState, { text: string; icon: React.ReactNode; cls: string }> = {
  verified: { text: "Verified approval", icon: <CheckCircle2 className="size-4" />, cls: "text-emerald-700" },
  issues: { text: "Approval invalid", icon: <ShieldAlert className="size-4" />, cls: "text-red-700" },
  awaiting: { text: "Waiting for decision", icon: <Hourglass className="size-4" />, cls: "text-amber-700" },
  not_reached: { text: "Not reached", icon: <CircleDashed className="size-4" />, cls: "text-slate-500" },
};

/** Completion summary shown next to the workflow status. */
export function CompletionSeal({ gov, status }: { gov: Governance; status: WorkflowStatus }) {
  if (status !== "succeeded") return null;
  return gov.completion_verified ? (
    <span className="inline-flex items-center gap-1 rounded-md border border-emerald-300 bg-emerald-50 px-2 py-0.5 text-xs font-medium text-emerald-800" data-testid="completion-seal">
      <ShieldCheck className="size-3.5" /> Verified: {gov.gates_granted}/{gov.gates_total} gates approved
    </span>
  ) : (
    <span className="inline-flex items-center gap-1 rounded-md border border-red-300 bg-red-50 px-2 py-0.5 text-xs font-medium text-red-800" data-testid="completion-seal">
      <ShieldAlert className="size-3.5" /> Completion not verified
    </span>
  );
}

export function GovernanceCard({ gov, status, pendingGate }: { gov: Governance; status: WorkflowStatus; pendingGate: string | null }) {
  const cc = gov.completion_check;
  return (
    <Card>
      <CardHeader>
        <CardTitle>Governance verification</CardTitle>
        <CardDescription>
          Each approval gate is checked with the same rules the engine applies before recording success: an approved decision by a human approver who is not an author,
          bound to the hash of the current artifact version.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-0">
        <Table aria-label="Gate verification">
          <TableHeader>
            <TableRow>
              <TableHead>Gate</TableHead>
              <TableHead>Verification</TableHead>
              <TableHead>Approver</TableHead>
              <TableHead>Decided</TableHead>
              <TableHead>Approved hash</TableHead>
              <TableHead>Current artifact</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {gov.gates.map((g) => {
              const s = gateState(g, pendingGate);
              const l = LABEL[s];
              return (
                <TableRow key={g.gate}>
                  <TableCell className="font-medium">{humanize(g.gate)}</TableCell>
                  <TableCell>
                    <span className={`inline-flex items-center gap-1.5 text-xs font-medium ${l.cls}`}>
                      {l.icon} {l.text}
                    </span>
                    {g.issues.map((i) => (
                      <div key={i} className="mt-1 text-xs text-red-700">
                        {i}
                      </div>
                    ))}
                  </TableCell>
                  <TableCell className="text-xs">
                    {g.approver_id ? (
                      <>
                        {g.approver_id} <span className="text-muted-foreground">({g.approver_role})</span>
                      </>
                    ) : (
                      "—"
                    )}
                  </TableCell>
                  <TableCell className="text-xs whitespace-nowrap">{formatDateTime(g.decided_at)}</TableCell>
                  <TableCell className="font-mono text-xs">
                    {g.approval_id ? (
                      <Link className="hover:underline" to={`/approvals/${g.approval_id}`}>
                        {shortHash(g.approved_hash)}
                      </Link>
                    ) : (
                      "—"
                    )}
                  </TableCell>
                  <TableCell className="font-mono text-xs">
                    {shortHash(g.current_hash)}
                    {g.approved_hash && g.current_hash && (g.approved_hash === g.current_hash ? " ✓" : " ≠")}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
        <div className="px-5 text-xs">
          {cc ? (
            <span className={cc.outcome === "succeeded" ? "text-emerald-800" : "text-red-800"}>
              Engine completion check: <b>{cc.outcome}</b> at {formatDateTime(cc.occurred_at)}
              {cc.details?.violations?.length ? ` – ${cc.details.violations.join("; ")}` : ""}
            </span>
          ) : status === "succeeded" ? (
            <span className="text-red-800">
              No engine completion check is recorded for this run (it finished before the completion guard existed). The gate table above is the verification.
            </span>
          ) : (
            <span className="text-muted-foreground">The engine runs its completion check when the workflow reaches a successful end.</span>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
