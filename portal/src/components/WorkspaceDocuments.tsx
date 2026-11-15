import { AlertTriangle, CheckCircle2, CircleDashed, FileWarning, FolderGit2, Hourglass } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatDateTime, humanize, shortHash } from "@/lib/format";
import type { GateDocument, MaterializationState, WorkspaceRegistration } from "@/lib/types";

const STATE: Record<MaterializationState, { text: string; cls: string; icon: React.ReactNode }> = {
  verified: { text: "In workspace · verified", cls: "text-emerald-700", icon: <CheckCircle2 className="size-4" /> },
  modified: { text: "Modified locally", cls: "text-amber-700", icon: <FileWarning className="size-4" /> },
  missing: { text: "Missing from workspace", cls: "text-red-700", icon: <AlertTriangle className="size-4" /> },
  outdated: { text: "Workspace holds another version", cls: "text-amber-700", icon: <FileWarning className="size-4" /> },
  failed: { text: "Write failed", cls: "text-red-700", icon: <AlertTriangle className="size-4" /> },
  conflict: { text: "Not written – conflict", cls: "text-red-700", icon: <AlertTriangle className="size-4" /> },
  not_materialized: { text: "Not written yet", cls: "text-slate-600", icon: <CircleDashed className="size-4" /> },
  awaiting_approval: { text: "Awaiting approval", cls: "text-slate-600", icon: <Hourglass className="size-4" /> },
  no_workspace: { text: "No workspace registered", cls: "text-red-700", icon: <AlertTriangle className="size-4" /> },
};

export function materializationLabel(state: MaterializationState) {
  return STATE[state] ?? { text: humanize(state), cls: "text-slate-600", icon: <CircleDashed className="size-4" /> };
}

/** One document's workspace state (also used on the review page). */
export function WorkspaceState({ doc }: { doc: GateDocument }) {
  const m = doc.materialization;
  if (!doc.target_dir || !m) return <span className="text-xs text-muted-foreground">Not written to the workspace</span>;
  const l = materializationLabel(m.state);
  return (
    <div className="flex flex-col gap-0.5" data-testid={`workspace-state-${doc.gate}`}>
      <span className={`inline-flex items-center gap-1.5 text-xs font-medium ${l.cls}`}>
        {l.icon} {l.text}
      </span>
      <span className="text-xs text-muted-foreground">{m.explanation}</span>
      {m.files
        .filter((f) => f.state !== "verified")
        .slice(0, 8)
        .map((f) => (
          <span key={f.path} className="font-mono text-[11px] text-red-700">
            {f.state}: {f.path}
          </span>
        ))}
      {doc.blocked && <span className="text-xs font-medium text-red-700">Workflow held at this gate: {doc.blocked}</span>}
    </div>
  );
}

export function WorkspaceDocuments({ docs, workspace }: { docs: GateDocument[]; workspace: WorkspaceRegistration | null | undefined }) {
  const rows = docs.filter((d) => d.target_dir);
  if (!rows.length) return null;
  const needsSync = rows.some((d) => d.approved && d.materialization && d.materialization.state !== "verified");
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <FolderGit2 className="size-4" /> Documents in the project workspace
        </CardTitle>
        <CardDescription>
          EAEH's artifact store is authoritative and keeps every version. Once a document is approved, the engine writes that version into the application repository and re-reads every file to check its hash.
          The state below comes from re-hashing the files on disk for this page. "Verified" means every file matched.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-0">
        <div className="px-5 text-xs">
          Workspace:{" "}
          {workspace ? (
            <code className="break-all">{workspace.root}</code>
          ) : (
            <span className="font-medium text-red-700">not registered – run `eaeh artifact sync` inside the project repository</span>
          )}
        </div>
        <Table aria-label="Workspace documents">
          <TableHeader>
            <TableRow>
              <TableHead>Document</TableHead>
              <TableHead>Version</TableHead>
              <TableHead>Approval</TableHead>
              <TableHead>Workspace path</TableHead>
              <TableHead>Materialization</TableHead>
              <TableHead>Last written</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((d) => (
              <TableRow key={d.gate}>
                <TableCell className="font-medium">
                  {humanize(d.gate.replace(/^approve_/, ""))}
                  <div className="text-xs font-normal text-muted-foreground">{d.artifact?.name ?? d.artifact_from}</div>
                </TableCell>
                <TableCell className="text-xs whitespace-nowrap">
                  {d.artifact ? (
                    <>
                      v{d.artifact.version} <span className="font-mono text-muted-foreground">{shortHash(d.artifact.hash)}</span>
                    </>
                  ) : (
                    "—"
                  )}
                </TableCell>
                <TableCell className="text-xs">
                  {d.approved ? (
                    <span className="text-emerald-700">Approved by {d.approval.approver_id}</span>
                  ) : d.approval.approval_id ? (
                    <span className="text-red-700">Not valid</span>
                  ) : (
                    <span className="text-muted-foreground">Not approved</span>
                  )}
                </TableCell>
                <TableCell className="font-mono text-xs">{d.target_dir}/</TableCell>
                <TableCell>
                  <WorkspaceState doc={d} />
                </TableCell>
                <TableCell className="text-xs whitespace-nowrap">{formatDateTime(d.materialization?.last_success_at ?? null)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        {needsSync && (
          <p className="px-5 text-xs text-amber-800">
            An approved document is not verified in the workspace. Run <code>eaeh artifact sync</code> in the project repository to write the approved version (it will not overwrite files you changed locally), then{" "}
            <code>eaeh artifact status</code> for details.
          </p>
        )}
      </CardContent>
    </Card>
  );
}
