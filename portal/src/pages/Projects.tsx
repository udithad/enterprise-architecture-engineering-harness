import { useMemo, useState } from "react";
import { Link, useNavigate } from "react-router";
import { Search } from "lucide-react";
import { useDashboard } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { StatusBadge } from "@/components/StatusBadge";
import { InstallationPanel } from "@/components/InstallationPanel";
import { Card } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { FolderPlus } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { projectStatus } from "@/lib/status";
import { formatRelative, humanize, shortId } from "@/lib/format";
import type { DashboardProject } from "@/lib/types";

function stackText(p: Pick<DashboardProject, "stack">): string {
  const s = p.stack;
  return s ? [s.language, s.framework, s.build_tool].filter(Boolean).join(" · ") : "";
}

export function ProjectsPage() {
  // The dashboard read model carries every project registered in this installation that the
  // identity can access – in every lifecycle state, with or without workflows.
  const q = useDashboard();
  const navigate = useNavigate();
  const [search, setSearch] = useState("");
  const items = useMemo(() => {
    const s = search.trim().toLowerCase();
    return (q.data?.projects ?? [])
      .filter((p) => !s || [p.name, p.id, p.description ?? "", stackText(p)].some((v) => v.toLowerCase().includes(s)))
      .sort((a, b) => (b.last_activity ?? "").localeCompare(a.last_activity ?? "") || a.name.localeCompare(b.name));
  }, [q.data, search]);
  return (
    <>
      <PageHeader
        title="Projects"
        description="Every project registered in this EAEH installation that your identity can access. Register a project here or with `eaeh project init`."
        actions={
          <Button asChild>
            <Link to="/projects/register">
              <FolderPlus /> Register existing project
            </Link>
          </Button>
        }
      />
      <div className="mb-4 flex flex-col gap-3">
        <InstallationPanel projectCount={q.data?.projects.length} />
        <div className="relative w-full sm:w-72">
          <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" aria-hidden />
          <Input className="pl-8" placeholder="Search projects…" value={search} onChange={(e) => setSearch(e.target.value)} aria-label="Search projects" />
        </div>
      </div>
      <Card className="overflow-hidden">
        {q.isPending && <div className="p-4"><LoadingRows /></div>}
        {q.isError && <div className="p-4"><ErrorState error={q.error} onRetry={() => void q.refetch()} /></div>}
        {q.data && !items.length && (
          <div className="p-4">
            <EmptyState title={search ? "No projects match your search" : "No projects visible in this installation"}>
              {!search && "Register one with `eaeh project init` (same configuration as this portal), or ask an administrator for access (`eaeh principal grant`)."}
            </EmptyState>
          </div>
        )}
        {!!items.length && (
          <Table aria-label="Projects">
            <TableHeader>
              <TableRow>
                <TableHead>Project</TableHead>
                <TableHead>Stack</TableHead>
                <TableHead>Workspace</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Current workflow</TableHead>
                <TableHead className="text-right">Pending approvals</TableHead>
                <TableHead>Last activity</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {items.map((p) => {
                const cw = p.current_workflow;
                return (
                  <TableRow
                    key={p.id}
                    className="cursor-pointer"
                    tabIndex={0}
                    data-testid={`project-row-${p.name}`}
                    onClick={() => navigate(`/projects/${p.id}`)}
                    onKeyDown={(e) => e.key === "Enter" && navigate(`/projects/${p.id}`)}
                  >
                    <TableCell className="max-w-72">
                      <div className="font-medium">{p.name}</div>
                      {p.description && <div className="truncate text-xs text-muted-foreground">{p.description}</div>}
                      <div className="font-mono text-[11px] text-muted-foreground">{p.id}</div>
                    </TableCell>
                    <TableCell className="text-xs">
                      {stackText(p) || "—"}
                      {!!p.registered_workflows?.length && <div className="text-[11px] text-muted-foreground">{p.registered_workflows.join(", ")}</div>}
                    </TableCell>
                    <TableCell className="max-w-56 text-xs">
                      {p.workspace ? <code className="break-all text-[11px]">{p.workspace.root}</code> : <span className="text-muted-foreground" title={p.manifest_issue ?? ""}>not registered</span>}
                    </TableCell>
                    <TableCell>
                      <StatusBadge info={projectStatus(p.status, p.restricted)} />
                    </TableCell>
                    <TableCell className="text-xs">
                      {cw ? (
                        <Link className="hover:underline" to={`/workflows/${cw.id}`} onClick={(e) => e.stopPropagation()}>
                          {cw.definition_id} · {humanize(cw.state)}
                          <div className="font-mono text-[11px] text-muted-foreground">…{shortId(cw.id)}</div>
                        </Link>
                      ) : p.restricted ? (
                        "—"
                      ) : (
                        <span className="text-muted-foreground">none yet</span>
                      )}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">{p.pending_approvals ?? "—"}</TableCell>
                    <TableCell className="text-xs whitespace-nowrap">{p.last_activity ? formatRelative(p.last_activity) : "—"}</TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </Card>
    </>
  );
}
