import { useMemo, useState } from "react";
import { Link } from "react-router";
import { RefreshCw } from "lucide-react";
import { useProjects, useSession, useSkillCatalog, useSkillDiscover, useSkillSync } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { StatusBadge } from "@/components/StatusBadge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { describeError } from "@/lib/api";
import { skillStatus } from "@/lib/status";


const LAYER_ORDER = ["requirements-analysis", "requirements-review", "hld-design", "lld-design", "implementation-planning", "implementation"];

export function SkillsPage() {
  const q = useSkillCatalog();
  const session = useSession();
  const projects = useProjects();
  const sync = useSkillSync();
  const discover = useSkillDiscover();
  const [project, setProject] = useState("");
  const kinds = useMemo(() => {
    const ks = Object.keys(q.data?.layers ?? {});
    return [...LAYER_ORDER.filter((k) => ks.includes(k)), ...ks.filter((k) => !LAYER_ORDER.includes(k)).sort()];
  }, [q.data]);
  const canRegister = !!session.data?.can_register_skills;
  const pid = project || projects.data?.items[0]?.id || "";
  const err = sync.isError ? describeError(sync.error) : null;

  return (
    <>
      <PageHeader
        title="Skills"
        description="Reusable instructions EAEH adds to each layer (stage) of a workflow. Skill files live in the installation's skills folder, as Markdown or JSON. A skill is applied only after a human approves it."
      />
      <div className="flex flex-col gap-6">
        <Card>
          <CardHeader>
            <CardTitle>Skill directories</CardTitle>
            <CardDescription>
              Copy a skill folder (a <code>SKILL.md</code> plus any nested files) into the skills folder. EAEH finds new and changed folders automatically, about every 30 seconds while the portal runs, and registers each one as a version waiting for approval. A change to any file creates a new version; the approved version stays in use until the new one is approved.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-wrap items-center gap-3 text-sm">
            <Button size="sm" variant="outline" disabled={discover.isPending} onClick={() => discover.mutate()}>
              <RefreshCw className={discover.isPending ? "animate-spin" : undefined} /> Scan now
            </Button>
            {discover.isError && (
              <span role="alert" className="text-xs text-red-700">
                {describeError(discover.error).detail}
              </span>
            )}
            {discover.data && (
              <ul className="w-full text-xs" data-testid="discovery-results">
                {discover.data.results.length === 0 && <li>No skill directories found.</li>}
                {discover.data.results.map((r) => (
                  <li key={r.dir}>
                    <code>{r.dir}/</code>: <b>{r.action}</b>
                    {r.version && ` (${r.version})`} – {r.detail}
                  </li>
                ))}
              </ul>
            )}
            {(q.data?.directories ?? [])
              .filter((d) => d.error)
              .map((d) => (
                <div key={d.dir} role="alert" className="w-full text-xs text-red-700">
                  <code>{d.dir}/</code> is not a valid skill: {d.error}
                </div>
              ))}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Register skill files</CardTitle>
            <CardDescription>
              Registers every skill file in <code className="break-all">{q.data?.skills_dir ?? "the skills folder"}</code> that isn't registered yet. Each one becomes a request <b>waiting for approval</b>. Nothing is approved here, and the registrant can never approve their own registration.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-wrap items-center gap-3 text-sm">
            {canRegister ? (
              <>
                <label className="flex items-center gap-2 text-xs">
                  Approval requests for project
                  <select className="rounded border px-2 py-1" value={pid} onChange={(e) => setProject(e.target.value)} aria-label="Project">
                    {(projects.data?.items ?? []).map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                </label>
                <Button size="sm" disabled={!pid || sync.isPending} onClick={() => sync.mutate(pid)}>
                  <RefreshCw className={sync.isPending ? "animate-spin" : undefined} /> Register skill files
                </Button>
                <span className="text-xs text-muted-foreground">as {session.data?.steward?.principal}</span>
              </>
            ) : (
              <span className="text-xs text-muted-foreground">
                To register skill files from the portal, restart it with <code>--steward steward</code>. Or run <code>eaeh skill sync</code> in a project folder.
              </span>
            )}
            {err && (
              <div role="alert" className="w-full text-xs text-red-700">
                {err.title}: {err.detail}
              </div>
            )}
            {sync.data && (
              <ul className="w-full text-xs" data-testid="sync-results">
                {sync.data.results.map((r) => (
                  <li key={r.reference}>
                    <code>{r.reference}</code>: <b>{r.action.replaceAll("_", " ")}</b>
                    {r.action !== "already_registered" && ` – ${r.detail}`}
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>

        {q.isPending && <LoadingRows rows={4} />}
        {q.isError && <ErrorState error={q.error} onRetry={() => void q.refetch()} />}
        {q.data?.file_error && (
          <div role="alert" className="rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-900">
            A skill file can't be read: {q.data.file_error}
          </div>
        )}
        {q.data && !q.data.items.length && <EmptyState title="No skills yet">Add a skill file to the skills folder (see the user guide, "Skills").</EmptyState>}
        {q.data && !!q.data.items.length && (
          <Card className="overflow-hidden">
            <Table aria-label="Skills">
              <TableHeader>
                <TableRow>
                  <TableHead>Skill</TableHead>
                  <TableHead>Layers</TableHead>
                  <TableHead>Format</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Files</TableHead>
                  <TableHead>Review</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {q.data.items.map((i) => (
                  <TableRow key={i.reference} data-testid={`skill-row-${i.id}`}>
                    <TableCell className="max-w-80">
                      <div className="font-mono text-xs font-medium">
                        {i.reference} {i.in_force && <span className="ml-1 rounded bg-emerald-100 px-1 font-sans text-[10px] text-emerald-900">in force</span>}
                      </div>
                      <div className="text-xs text-muted-foreground">{i.description}</div>
                    </TableCell>
                    <TableCell className="text-xs">{i.layers.length ? i.layers.join(", ") : <span className="text-muted-foreground">{i.task_kinds.join(", ") || "—"}</span>}</TableCell>
                    <TableCell className="text-xs">{i.format ?? (i.file ? "" : "registered only")}</TableCell>
                    <TableCell>
                      <StatusBadge info={skillStatus(i)} />
                    </TableCell>
                    <TableCell className="text-xs">
                      {i.registration ? (
                        <Link className="hover:underline" to={`/skills/${encodeURIComponent(i.id)}/${encodeURIComponent(i.version)}`}>
                          {i.package ? `${i.package.file_count} files` : "view"}
                        </Link>
                      ) : i.package ? (
                        `${i.package.file_count} files`
                      ) : (
                        "—"
                      )}
                    </TableCell>
                    <TableCell className="text-xs">
                      {i.registration?.approval_id ? (
                        <Link className="hover:underline" to={`/approvals/${i.registration.approval_id}`}>
                          {i.registration.status === "pending_approval" ? "Review" : "Open"}
                        </Link>
                      ) : (
                        "—"
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </Card>
        )}
        {q.data && (
          <Card>
            <CardHeader>
              <CardTitle>Layers</CardTitle>
              <CardDescription>Which workflow stages each task kind is. A skill whose `task_kinds` include a kind is applied at those stages once approved.</CardDescription>
            </CardHeader>
            <CardContent>
              <ul className="grid grid-cols-1 gap-1 text-xs md:grid-cols-2">
                {kinds.map((k) => (
                  <li key={k}>
                    <code>{k}</code>: {(q.data.layers[k] ?? []).join(", ")}
                  </li>
                ))}
              </ul>
            </CardContent>
          </Card>
        )}
      </div>
    </>
  );
}
