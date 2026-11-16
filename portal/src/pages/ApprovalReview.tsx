import { useState } from "react";
import { Link, useParams } from "react-router";
import { CheckCircle2, CircleAlert, GitBranch, Link2 } from "lucide-react";
import { useApprovalDocument, useSession } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { StatusBadge } from "@/components/StatusBadge";
import { CopyText } from "@/components/CopyText";
import { DocumentViewer } from "@/components/DocumentViewer";
import { DecisionPanel } from "@/components/DecisionPanel";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { approvalStatus, workflowStatus } from "@/lib/status";
import { formatDateTime, humanize, shortHash } from "@/lib/format";
import type { ApprovalDocument, TraceLink } from "@/lib/types";
import { GenerationBadge, ReplayNotice } from "@/components/Generation";
import { WorkspaceState } from "@/components/WorkspaceDocuments";
import { SkillReview } from "@/components/SkillReview";

function Meta({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="truncate text-sm">{children}</dd>
    </div>
  );
}

/** Quality-gate results recorded for the workflow (shape defined by the validation engine). */
function QualityResults({ quality }: { quality: Record<string, unknown> }) {
  const entries = Object.entries(quality);
  if (!entries.length) return <EmptyState title="No validation or policy results recorded for this workflow" />;
  return (
    <div className="flex flex-col gap-3">
      {entries.map(([state, v]) => {
        // The engine records a summary such as "requirements:passed (0 blocking, 4 advisory)".
        const text = typeof v === "string" ? v : null;
        const o = (text === null && v && typeof v === "object" ? v : {}) as Record<string, unknown>;
        const passed =
          text !== null
            ? /:passed\b/.test(text) ? true : /:failed\b/.test(text) ? false : null
            : typeof o.passed === "boolean" ? o.passed : typeof o.outcome === "string" ? o.outcome === "passed" || o.outcome === "pass" : null;
        const findings = (Array.isArray(o.findings) ? o.findings : Array.isArray(o.issues) ? o.issues : null) as unknown[] | null;
        return (
          <div key={state} className="rounded-md border p-3">
            <div className="flex items-center gap-2 text-sm font-medium">
              {passed === true && <CheckCircle2 className="size-4 text-emerald-600" aria-hidden />}
              {passed === false && <CircleAlert className="size-4 text-red-600" aria-hidden />}
              {humanize(state)}
              {typeof o.gate === "string" && <span className="text-xs font-normal text-muted-foreground">gate {o.gate}</span>}
              {passed !== null && <span className="text-xs font-normal">{passed ? "passed" : "failed"}</span>}
            </div>
            {text && <p className="mt-1 font-mono text-xs text-muted-foreground">{text}</p>}
            {findings && findings.length > 0 && (
              <ul className="mt-2 list-disc pl-5 text-xs">
                {findings.map((f, i) => (
                  <li key={i}>{typeof f === "string" ? f : JSON.stringify(f)}</li>
                ))}
              </ul>
            )}
            {text === null && <details className="mt-2 text-xs">
              <summary className="cursor-pointer text-muted-foreground">Recorded result</summary>
              <pre className="mt-1 overflow-x-auto rounded bg-muted/50 p-2">{JSON.stringify(v, null, 2)}</pre>
            </details>}
          </div>
        );
      })}
    </div>
  );
}

function Traceability({ links }: { links: TraceLink[] }) {
  const [q, setQ] = useState("");
  const s = q.trim().toLowerCase();
  const items = links.filter((l) => !s || [l.from_id, l.to_id, l.relation, l.from_kind, l.to_kind].some((x) => x.toLowerCase().includes(s)));
  if (!links.length)
    return <EmptyState title="No traceability links yet">Links are recorded from approved Requirements, HLD and LLD artifacts, so they appear once earlier gates of this workflow are approved.</EmptyState>;
  return (
    <div className="flex flex-col gap-2">
      <Input placeholder="Filter by id, kind or relation…" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Filter traceability links" className="max-w-sm" />
      <Table aria-label="Traceability links">
        <TableHeader>
          <TableRow>
            <TableHead>From</TableHead>
            <TableHead>Relation</TableHead>
            <TableHead>To</TableHead>
            <TableHead>Source artifact</TableHead>
            <TableHead>Approval</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.slice(0, 300).map((l, i) => (
            <TableRow key={i}>
              <TableCell className="text-xs">
                <span className="text-muted-foreground">{l.from_kind}</span> {l.from_id}
              </TableCell>
              <TableCell className="text-xs">{l.relation}</TableCell>
              <TableCell className="text-xs">
                <span className="text-muted-foreground">{l.to_kind}</span> {l.to_id}
              </TableCell>
              <TableCell className="text-xs">
                {l.artifact_name} v{l.artifact_version}
              </TableCell>
              <TableCell className="text-xs">
                <Link className="hover:underline" to={`/approvals/${l.approval_id}`}>
                  open
                </Link>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      {items.length > 300 && <p className="text-xs text-muted-foreground">Showing 300 of {items.length} links.</p>}
    </div>
  );
}

function RelatedArtifacts({ doc }: { doc: ApprovalDocument }) {
  const inst = doc.workflow?.instance;
  if (!inst) return null;
  const arts = Object.entries(inst.artifacts);
  const approvedByGate = inst.approved_gates;
  return (
    <ul className="flex flex-col gap-1.5 text-sm">
      {arts.map(([state, a]) => {
        const current = a.id === doc.approval.artifact_id;
        return (
          <li key={state} className="flex flex-wrap items-center gap-2">
            <span className={current ? "font-semibold" : ""}>{a.name}</span>
            <span className="text-xs text-muted-foreground">v{a.version}</span>
            {doc.workflow?.artifact_provenance?.[state] && <GenerationBadge g={doc.workflow.artifact_provenance[state]!} />}
            {current && <span className="rounded bg-primary px-1.5 text-[11px] text-primary-foreground">under review</span>}
          </li>
        );
      })}
      {Object.entries(approvedByGate).map(([gate, id]) => (
        <li key={gate} className="text-xs">
          <Link className="inline-flex items-center gap-1 hover:underline" to={`/approvals/${id}`}>
            <Link2 className="size-3" /> {humanize(gate)} – approved request
          </Link>
        </li>
      ))}
    </ul>
  );
}

export function ApprovalReviewPage() {
  const { id = "" } = useParams();
  const q = useApprovalDocument(id);
  const session = useSession();
  if (q.isPending) return <LoadingRows rows={8} label="Loading document" />;
  if (q.isError) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  const d = q.data;
  const a = d.approval;
  const art = d.document?.artifact;
  const prev = d.workflow?.previous_requests ?? [];

  return (
    <>
      <PageHeader
        eyebrow={
          <span>
            <Link className="hover:underline" to="/approvals">
              Approvals
            </Link>{" "}
            / {a.project_name}
          </span>
        }
        title={
          <span className="flex items-center gap-3">
            {a.document_type} review <StatusBadge info={approvalStatus(a.status)} />
          </span>
        }
        description={a.gate ? `${humanize(a.gate)} · ${a.workflow?.definition_id ?? a.subject_type}` : humanize(a.subject_type)}
      />
      <ReplayNotice provenance={d.workflow?.artifact_provenance} className="mb-6 block" />
      <div className="grid gap-6 xl:grid-cols-[1fr_360px]">
        <div className="flex min-w-0 flex-col gap-6">
          <Card>
            <CardContent className="pt-5">
              <dl className="grid grid-cols-2 gap-x-6 gap-y-3 md:grid-cols-4">
                <Meta label="Project">
                  <Link className="hover:underline" to={`/projects/${a.project_id}`}>
                    {a.project_name}
                  </Link>
                </Meta>
                <Meta label="Workflow">
                  {a.workflow ? (
                    <Link className="inline-flex items-center gap-1 hover:underline" to={`/workflows/${a.workflow.id}`}>
                      <GitBranch className="size-3" /> {a.workflow.definition_id}
                    </Link>
                  ) : (
                    "—"
                  )}
                </Meta>
                <Meta label="Workflow status">{a.workflow ? <StatusBadge info={workflowStatus(a.workflow.status)} /> : "—"}</Meta>
                <Meta label="Approver role">{a.required_role}</Meta>
                <Meta label="Artifact">{art ? art.name : "—"}</Meta>
                <Meta label="Version">{art ? `v${art.version}` : "—"}</Meta>
                <Meta label="Artifact hash">{a.artifact_hash ? <CopyText value={a.artifact_hash} display={shortHash(a.artifact_hash)} /> : "—"}</Meta>
                <Meta label="Requested">{formatDateTime(a.created_at)}</Meta>
                <Meta label="Requested by">{a.requested_by}</Meta>
                <Meta label="Authors (cannot approve)">{a.authors.join(", ") || "—"}</Meta>
                <Meta label="Size">{art ? `${(art.size_bytes / 1024).toFixed(1)} KiB${d.document?.bundle ? ` · ${d.document.files.length} files` : ""}` : "—"}</Meta>
                <Meta label="Approval id">
                  <CopyText value={a.id} display={`…${a.id.slice(-8)}`} />
                </Meta>
              </dl>
            </CardContent>
          </Card>
          {d.skill ? (
            <SkillReview skill={d.skill} />
          ) : (
          <Tabs defaultValue="document">
            <TabsList>
              <TabsTrigger value="document">Document</TabsTrigger>
              <TabsTrigger value="validation">Validation &amp; policy</TabsTrigger>
              <TabsTrigger value="trace">Traceability</TabsTrigger>
              <TabsTrigger value="history">History{prev.length ? ` (${prev.length})` : ""}</TabsTrigger>
            </TabsList>
            <TabsContent value="document">
              <Card>
                <CardContent className="pt-5">{d.document ? <DocumentViewer files={d.document.files} /> : <EmptyState title="No document attached to this request" />}</CardContent>
              </Card>
            </TabsContent>
            <TabsContent value="validation">
              <Card>
                <CardHeader>
                  <CardTitle>Validation and quality-gate results</CardTitle>
                  <CardDescription>Recorded by the platform before the document was stored. Documents that fail a gate never reach approval.</CardDescription>
                </CardHeader>
                <CardContent>{d.workflow ? <QualityResults quality={d.workflow.instance.quality} /> : <EmptyState title="Not a workflow approval" />}</CardContent>
              </Card>
            </TabsContent>
            <TabsContent value="trace">
              <Card>
                <CardContent className="pt-5">{d.workflow ? <Traceability links={d.workflow.trace_links} /> : <EmptyState title="Not a workflow approval" />}</CardContent>
              </Card>
            </TabsContent>
            <TabsContent value="history">
              <Card>
                <CardContent className="pt-5">
                  {prev.length ? (
                    <ol className="flex flex-col gap-3">
                      {prev.map((p) => (
                        <li key={p.id} className="rounded-md border p-3 text-sm">
                          <div className="flex flex-wrap items-center gap-2">
                            <StatusBadge info={approvalStatus(p.status)} />
                            <span className="text-xs text-muted-foreground">
                              requested {formatDateTime(p.created_at)} · v{p.artifact?.version ?? "?"} <span className="font-mono">{shortHash(p.artifact_hash)}</span>
                            </span>
                            <Link className="ml-auto text-xs hover:underline" to={`/approvals/${p.id}`}>
                              Open
                            </Link>
                          </div>
                          {p.decision && (
                            <div className="mt-2 text-xs">
                              <span className="text-muted-foreground">
                                {p.decision.approver_id} · {formatDateTime(p.decision.decided_at)}
                              </span>
                              {p.decision.comments && <p className="mt-1 whitespace-pre-wrap">{p.decision.comments}</p>}
                            </div>
                          )}
                        </li>
                      ))}
                    </ol>
                  ) : (
                    <EmptyState title="No earlier requests at this gate" />
                  )}
                </CardContent>
              </Card>
            </TabsContent>
          </Tabs>
          )}
        </div>
        <div className="flex flex-col gap-6">
          <Card className="xl:sticky xl:top-20">
            <CardHeader>
              <CardTitle>Decision</CardTitle>
              <CardDescription>Submitted to the EAEH approval service, which enforces role, separation of duties and the artifact hash.</CardDescription>
            </CardHeader>
            <CardContent>
              {session.data ? <DecisionPanel doc={d} session={session.data} /> : session.isError ? <ErrorState error={session.error} /> : <LoadingRows rows={2} />}
            </CardContent>
          </Card>
          {d.workflow?.workspace_document?.target_dir && (
            <Card>
              <CardHeader>
                <CardTitle>Project workspace</CardTitle>
                <CardDescription>
                  When this version is approved, it is written to <code>{d.workflow.workspace_document.target_dir}/</code> in{" "}
                  <code className="break-all">{d.workflow.workspace?.root ?? "the project repository (not registered yet)"}</code>. Drafts are never written there.
                </CardDescription>
              </CardHeader>
              <CardContent>
                <WorkspaceState doc={d.workflow.workspace_document} />
              </CardContent>
            </Card>
          )}
          {d.workflow && (
            <Card>
              <CardHeader>
                <CardTitle>Related artifacts</CardTitle>
                <CardDescription>Produced by this workflow run.</CardDescription>
              </CardHeader>
              <CardContent>
                <RelatedArtifacts doc={d} />
              </CardContent>
            </Card>
          )}
        </div>
      </div>
    </>
  );
}
