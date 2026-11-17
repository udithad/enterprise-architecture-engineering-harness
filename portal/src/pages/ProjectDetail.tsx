import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import { useProjectOverview } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { StatusBadge } from "@/components/StatusBadge";
import { ActivityList } from "@/components/ActivityList";
import { CopyText } from "@/components/CopyText";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Play } from "lucide-react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { approvalStatus, projectStatus, workflowStatus } from "@/lib/status";
import { WorkspaceDocuments } from "@/components/WorkspaceDocuments";
import { InstallationPanel } from "@/components/InstallationPanel";
import type { ProjectOverview, Readiness } from "@/lib/types";
import { formatDateTime, formatRelative, humanize, shortHash, shortId } from "@/lib/format";

const TABS = ["workflows", "approvals", "artifacts", "activity", "audit"] as const;

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="break-words">{children}</div>
    </div>
  );
}

function ProjectInfo({ d }: { d: ProjectOverview }) {
  const m = d.manifest;
  const cw = d.current_workflow;
  return (
    <Card className="mb-6">
      <CardContent className="grid grid-cols-1 gap-x-6 gap-y-3 pt-5 text-sm md:grid-cols-3">
        <Field label="Project id">
          <CopyText value={d.project.id} display={`…${shortId(d.project.id, 12)}`} />
        </Field>
        <Field label="Status">
          <StatusBadge info={projectStatus(d.status)} />
        </Field>
        <Field label="Current workflow">
          {cw ? (
            <Link className="hover:underline" to={`/workflows/${cw.id}`}>
              {cw.definition_id} · {humanize(cw.state)} <StatusBadge info={workflowStatus(cw.status, cw.blocked)} className="ml-1 scale-90" />
            </Link>
          ) : (
            "none yet"
          )}
        </Field>
        <Field label="Technology stack">{m ? [m.stack.language, m.stack.framework, m.stack.build_tool].filter(Boolean).join(" · ") || "—" : "—"}</Field>
        <Field label="Workspace">
          {d.workspace ? (
            <>
              <code className="text-xs">{d.workspace.root}</code>
              <div className="text-[11px] text-muted-foreground">registered {formatDateTime(d.workspace.registered_at)} by {d.workspace.registered_by}</div>
            </>
          ) : (
            <span className="text-muted-foreground">not registered</span>
          )}
        </Field>
        <Field label="BRD">
          {m?.brd ? (
            <>
              <code className="text-xs">{m.brd.path}</code>
              <div className="text-[11px] text-muted-foreground">{m.brd.citation}</div>
            </>
          ) : (
            "—"
          )}
        </Field>
        <Field label="Registered workflows">{m?.workflows.length ? m.workflows.join(", ") : "—"}</Field>
        <Field label="Roles">{m ? `developer ${m.roles.developer} · steward ${m.roles.steward} · approvers ${m.roles.approvers.join(", ")}` : "—"}</Field>
        <Field label="Execution mode">{m?.execution_mode ?? "—"}</Field>
        {m?.stack.constraints?.length ? (
          <div className="md:col-span-3">
            <div className="text-xs text-muted-foreground">Constraints</div>
            <ul className="list-disc pl-5 text-xs">
              {m.stack.constraints.map((c) => (
                <li key={c}>{c}</li>
              ))}
            </ul>
          </div>
        ) : null}
        {d.manifest_issue && <div className="text-xs text-amber-800 md:col-span-3">Project details unavailable: {d.manifest_issue}</div>}
      </CardContent>
    </Card>
  );
}

function ReadinessCard({ r }: { r: ProjectOverview["readiness"] }) {
  if (!r) return null;
  if ("error" in r) return <div className="mb-6 text-xs text-amber-800">Readiness could not be evaluated: {r.error}</div>;
  const rr = r as Readiness;
  const shown = rr.checks.filter((c) => c.status !== "ok");
  return (
    <Card className="mb-6" data-testid="readiness">
      <CardContent className="pt-5 text-sm">
        <div className="mb-2 font-medium">
          Readiness: <span className={rr.ready ? "text-emerald-700" : "text-red-700"}>{rr.ready ? "READY" : "NOT READY"}</span>{" "}
          <span className="text-xs text-muted-foreground">
            ({rr.blocking} blocking, {rr.warnings} warning(s), {rr.checks.length} checks – same as `eaeh project readiness`)
          </span>
        </div>
        {shown.length ? (
          <ul className="flex flex-col gap-1 text-xs">
            {shown.map((c, i) => (
              <li key={i}>
                <b className={c.status === "block" ? "text-red-700" : "text-amber-800"}>{c.status.toUpperCase()}</b> [{c.area}] {c.message}
                {c.fix && <span className="text-muted-foreground"> → {c.fix}</span>}
              </li>
            ))}
          </ul>
        ) : (
          <div className="text-xs text-muted-foreground">All checks passed.</div>
        )}
      </CardContent>
    </Card>
  );
}

export function ProjectDetailPage() {
  const { id = "" } = useParams();
  const [params, setParams] = useSearchParams();
  const tab = (TABS as readonly string[]).includes(params.get("tab") ?? "") ? (params.get("tab") as string) : "workflows";
  const q = useProjectOverview(id);
  const navigate = useNavigate();
  if (q.isPending) return <LoadingRows rows={6} label="Loading project" />;
  if (q.isError) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  const d = q.data;
  const pending = (d.approvals ?? []).filter((a) => a.status === "pending");

  return (
    <>
      <PageHeader
        eyebrow={<Link className="hover:underline" to="/projects">Projects</Link>}
        title={d.project.name}
        description={d.manifest?.description || (d.project.domain ? `Knowledge domain: ${d.project.domain}` : undefined)}
        actions={
          <div className="flex gap-2">
            <Button asChild variant="outline">
              <Link to={`/execution?project=${d.project.id}`}>Execution</Link>
            </Button>
            <Button asChild>
              <Link to={`/projects/${d.project.id}/start`}>
                <Play /> Start workflow
              </Link>
            </Button>
          </div>
        }
      />
      <ProjectInfo d={d} />
      <ReadinessCard r={d.readiness} />
      {d.documents && d.documents.items.some((x) => x.target_dir) && (
        <div className="mb-6">
          <WorkspaceDocuments docs={d.documents.items} workspace={d.workspace} />
        </div>
      )}
      <Tabs
        value={tab}
        onValueChange={(v) => {
          const n = new URLSearchParams(params);
          n.set("tab", v);
          setParams(n, { replace: true });
        }}
      >
        <TabsList>
          <TabsTrigger value="workflows">Workflows</TabsTrigger>
          <TabsTrigger value="approvals">Approvals{pending.length ? ` (${pending.length})` : ""}</TabsTrigger>
          <TabsTrigger value="artifacts">Artifacts</TabsTrigger>
          <TabsTrigger value="activity">Recent activity</TabsTrigger>
          {d.audit && <TabsTrigger value="audit">Audit</TabsTrigger>}
        </TabsList>
        <TabsContent value="workflows">
          <Card className="overflow-hidden">
            {!d.workflows ? (
              <div className="p-4"><EmptyState title="workflow.read is required to see workflows" /></div>
            ) : !d.workflows.length ? (
              <div className="p-4">
                <EmptyState title="No workflows in this project yet">
                  The project is registered and ready. Start the design in the project folder{d.workspace ? ` (${d.workspace.root})` : ""}: <code>eaeh workflow run design --yes</code>
                </EmptyState>
              </div>
            ) : (
              <Table aria-label="Project workflows">
                <TableHeader>
                  <TableRow>
                    <TableHead>Workflow</TableHead>
                    <TableHead>Stage</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>Started</TableHead>
                    <TableHead>Updated</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {d.workflows.map((w) => (
                    <TableRow key={w.id} className="cursor-pointer" tabIndex={0} onClick={() => navigate(`/workflows/${w.id}`)} onKeyDown={(e) => e.key === "Enter" && navigate(`/workflows/${w.id}`)}>
                      <TableCell>
                        <div className="font-medium">{w.definition_id}</div>
                        <div className="font-mono text-[11px] text-muted-foreground">…{shortId(w.id)}</div>
                      </TableCell>
                      <TableCell>{humanize(w.state)}</TableCell>
                      <TableCell><StatusBadge info={workflowStatus(w.status, w.blocked)} /></TableCell>
                      <TableCell className="text-xs">{formatDateTime(w.created_at)}</TableCell>
                      <TableCell className="text-xs">{formatRelative(w.updated_at)}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Card>
        </TabsContent>
        <TabsContent value="approvals">
          <Card className="overflow-hidden">
            {!d.approvals ? (
              <div className="p-4"><EmptyState title="approval.read is required to see approvals" /></div>
            ) : !d.approvals.length ? (
              <div className="p-4"><EmptyState title="No approval requests in this project" /></div>
            ) : (
              <Table aria-label="Project approvals">
                <TableHeader>
                  <TableRow>
                    <TableHead>Document</TableHead>
                    <TableHead>Gate</TableHead>
                    <TableHead>Version</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>Created</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {d.approvals.map((a) => (
                    <TableRow key={a.id} className="cursor-pointer" tabIndex={0} onClick={() => navigate(`/approvals/${a.id}`)} onKeyDown={(e) => e.key === "Enter" && navigate(`/approvals/${a.id}`)}>
                      <TableCell className="font-medium">{a.document_type}</TableCell>
                      <TableCell className="text-xs">{humanize(a.gate ?? a.subject_type)}</TableCell>
                      <TableCell className="text-xs">{a.artifact ? `v${a.artifact.version}` : "—"}</TableCell>
                      <TableCell><StatusBadge info={approvalStatus(a.status)} /></TableCell>
                      <TableCell className="text-xs">{formatDateTime(a.created_at)}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Card>
        </TabsContent>
        <TabsContent value="artifacts">
          <Card className="overflow-hidden">
            {!d.artifacts?.length ? (
              <div className="p-4"><EmptyState title="No engineering artifacts produced yet" /></div>
            ) : (
              <Table aria-label="Engineering artifacts">
                <TableHeader>
                  <TableRow>
                    <TableHead>Artifact</TableHead>
                    <TableHead>Type</TableHead>
                    <TableHead>Version</TableHead>
                    <TableHead>Hash</TableHead>
                    <TableHead>Gate</TableHead>
                    <TableHead>Workflow</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {d.artifacts.map((a) => (
                    <TableRow key={a.name}>
                      <TableCell className="font-medium">{a.name}</TableCell>
                      <TableCell className="text-xs">{a.document_type}</TableCell>
                      <TableCell className="text-xs">v{a.version}</TableCell>
                      <TableCell><CopyText value={a.hash} display={shortHash(a.hash)} /></TableCell>
                      <TableCell className="text-xs">
                        {a.gate ? (a.gate_approved ? `${humanize(a.gate)} · approved` : `${humanize(a.gate)} · not approved`) : "—"}
                      </TableCell>
                      <TableCell className="text-xs">
                        <Link className="hover:underline" to={`/workflows/${a.workflow_instance_id}`}>…{shortId(a.workflow_instance_id)}</Link>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Card>
        </TabsContent>
        <TabsContent value="audit">
          <Card className="overflow-hidden">
            {!d.audit?.length ? (
              <div className="p-4"><EmptyState title="No audit records for this project" /></div>
            ) : (
              <Table aria-label="Project audit">
                <TableHeader>
                  <TableRow>
                    <TableHead>#</TableHead>
                    <TableHead>When</TableHead>
                    <TableHead>Actor</TableHead>
                    <TableHead>Action</TableHead>
                    <TableHead>Resource</TableHead>
                    <TableHead>Outcome</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {d.audit.map((r) => (
                    <TableRow key={r.seq}>
                      <TableCell className="tabular-nums text-xs">{r.seq}</TableCell>
                      <TableCell className="text-xs whitespace-nowrap">{formatDateTime(r.occurred_at)}</TableCell>
                      <TableCell className="text-xs">{r.actor_id}</TableCell>
                      <TableCell className="font-mono text-xs">{r.action}</TableCell>
                      <TableCell className="text-xs">{r.resource_type} …{shortId(r.resource_id)}</TableCell>
                      <TableCell className="text-xs">{r.outcome}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Card>
        </TabsContent>
        <TabsContent value="activity">
          <Card>
            <CardContent className="pt-4">
              <ActivityList items={d.activity ?? []} showProject={false} />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
      <div className="mt-6">
        <InstallationPanel compact />
      </div>
    </>
  );
}
