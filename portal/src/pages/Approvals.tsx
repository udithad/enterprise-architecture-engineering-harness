import { useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { ArrowRight, Search } from "lucide-react";
import { useApprovalQueue } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { ProjectFilter } from "@/components/ProjectFilter";
import { StatusBadge } from "@/components/StatusBadge";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { approvalStatus } from "@/lib/status";
import { formatDateTime, formatRelative, humanize, shortHash, shortId } from "@/lib/format";

const STATUSES = [
  ["pending", "Pending"],
  ["approved", "Approved"],
  ["changes_requested", "Changes requested"],
  ["rejected", "Rejected"],
  ["superseded", "Superseded"],
  ["all", "All statuses"],
] as const;

export function ApprovalsPage() {
  const [params, setParams] = useSearchParams();
  const status = params.get("status") ?? "pending";
  const project = params.get("project") ?? "all";
  const [type, setType] = useState("all");
  const [search, setSearch] = useState("");
  const q = useApprovalQueue(status, project);

  const types = useMemo(() => Array.from(new Set((q.data?.items ?? []).map((a) => a.document_type))).sort(), [q.data]);
  const items = useMemo(() => {
    const s = search.trim().toLowerCase();
    return (q.data?.items ?? []).filter(
      (a) =>
        (type === "all" || a.document_type === type) &&
        (!s || [a.id, a.document_type, a.project_name, a.gate ?? "", a.subject_ref, a.workflow?.definition_id ?? ""].some((x) => x.toLowerCase().includes(s))),
    );
  }, [q.data, search, type]);

  const set = (k: string, v: string) => {
    const n = new URLSearchParams(params);
    n.set(k, v);
    setParams(n, { replace: true });
  };

  return (
    <>
      <PageHeader title="Approvals" description="Human approval requests for Requirements, HLD and LLD documents (and other governed subjects)." />
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <Select value={status} onValueChange={(v) => set("status", v)}>
          <SelectTrigger className="w-48" aria-label="Filter by status">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {STATUSES.map(([v, l]) => (
              <SelectItem key={v} value={v}>
                {l}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <ProjectFilter value={project} onChange={(v) => set("project", v)} />
        <Select value={type} onValueChange={setType}>
          <SelectTrigger className="w-44" aria-label="Filter by document type">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All document types</SelectItem>
            {types.map((t) => (
              <SelectItem key={t} value={t}>
                {t}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <div className="relative ml-auto w-full sm:w-72">
          <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" aria-hidden />
          <Input className="pl-8" placeholder="Search approvals…" value={search} onChange={(e) => setSearch(e.target.value)} aria-label="Search approvals" />
        </div>
      </div>
      <Card className="overflow-hidden">
        {q.isPending && <div className="p-4"><LoadingRows /></div>}
        {q.isError && <div className="p-4"><ErrorState error={q.error} onRetry={() => void q.refetch()} /></div>}
        {q.data && !items.length && (
          <div className="p-4">
            <EmptyState title={status === "pending" && !search && type === "all" ? "Nothing awaits your review" : "No approvals match these filters"} />
          </div>
        )}
        {!!items.length && (
          <Table aria-label="Approval requests">
            <TableHeader>
              <TableRow>
                <TableHead>Document</TableHead>
                <TableHead>Project</TableHead>
                <TableHead>Workflow / gate</TableHead>
                <TableHead>Artifact version</TableHead>
                <TableHead>Approver role</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Created</TableHead>
                <TableHead />
              </TableRow>
            </TableHeader>
            <TableBody>
              {items.map((a) => (
                <TableRow key={a.id}>
                  <TableCell>
                    <div className="font-medium">{a.document_type}</div>
                    <div className="font-mono text-[11px] text-muted-foreground">…{shortId(a.id)}</div>
                  </TableCell>
                  <TableCell>{a.project_name}</TableCell>
                  <TableCell className="text-xs">
                    {a.workflow ? (
                      <Link className="hover:underline" to={`/workflows/${a.workflow.id}`}>
                        {a.workflow.definition_id}
                      </Link>
                    ) : (
                      humanize(a.subject_type)
                    )}
                    {a.gate && <div className="text-muted-foreground">{humanize(a.gate)}</div>}
                  </TableCell>
                  <TableCell className="text-xs">
                    {a.artifact ? `v${a.artifact.version}` : "—"} <span className="font-mono text-muted-foreground">{shortHash(a.artifact_hash)}</span>
                  </TableCell>
                  <TableCell className="text-xs capitalize">{a.required_role}</TableCell>
                  <TableCell>
                    <StatusBadge info={approvalStatus(a.status)} />
                  </TableCell>
                  <TableCell className="text-xs whitespace-nowrap" title={formatDateTime(a.created_at)}>
                    {formatRelative(a.created_at)}
                  </TableCell>
                  <TableCell className="text-right">
                    <Button asChild size="sm" variant={a.status === "pending" ? "default" : "outline"}>
                      <Link to={`/approvals/${a.id}`}>
                        {a.status === "pending" ? "Review" : "Open"} <ArrowRight />
                      </Link>
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>
      {!!q.data?.restricted_projects?.length && (
        <p className="mt-3 text-xs text-muted-foreground">{q.data.restricted_projects.length} project(s) not shown: the viewing identity lacks approval.read there.</p>
      )}
    </>
  );
}
