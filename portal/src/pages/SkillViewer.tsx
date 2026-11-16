import { Link, useParams } from "react-router";
import { UserCheck } from "lucide-react";
import { useSkillFiles } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { ErrorState, LoadingRows } from "@/components/States";
import { StatusBadge } from "@/components/StatusBadge";
import { CopyText } from "@/components/CopyText";
import { SkillFiles } from "@/components/SkillFiles";
import { FrontMatter } from "@/components/FrontMatter";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { skillStatus } from "@/lib/status";
import { formatDateTime, humanize, shortHash } from "@/lib/format";

/** Complete content of one skill version: metadata, every file (nested tree, previews,
 * hashes, changes against the version in force) and its approval history. */
export function SkillViewerPage() {
  const { id = "", version = "" } = useParams();
  const q = useSkillFiles(id, version);
  if (q.isPending) return <LoadingRows rows={6} label="Loading skill" />;
  if (q.isError) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  const s = q.data;
  const st = skillStatus({ registration: { status: s.status ?? "" }, file_changed: false });
  const pending = s.status === "pending_approval" && s.approval_id;
  return (
    <>
      <PageHeader
        eyebrow={
          <Link className="hover:underline" to="/skills">
            Skills
          </Link>
        }
        title={
          <span className="flex items-center gap-3">
            <code>{s.reference}</code> <StatusBadge info={st} />
          </span>
        }
        description={s.description}
        actions={
          pending && (
            <Button asChild>
              <Link to={`/approvals/${s.approval_id}`}>
                <UserCheck /> Review and decide
              </Link>
            </Button>
          )
        }
      />
      <div className="flex flex-col gap-6">
        <Card>
          <CardContent className="grid grid-cols-1 gap-x-6 gap-y-3 pt-5 text-sm md:grid-cols-4">
            <Meta label="Layers">
              {s.layers.length ? s.layers.join(", ") : s.task_kinds.join(", ") || "none – not applied to any stage"}
              {s.layers_from && <div className="text-xs text-muted-foreground">{s.layers_from}</div>}
            </Meta>
            <Meta label="Version in force">{s.in_force_version ?? "none approved yet"}</Meta>
            <Meta label="Source">
              {s.package ? `directory ${s.package.dir}/ · manifest ${s.package.source_version} · revision ${s.package.revision}` : "single file"}
            </Meta>
            <Meta label="Registered by">{s.registered_by ?? "—"}</Meta>
            <Meta label="Tree hash">{s.package ? <CopyText value={s.package.tree_hash} display={shortHash(s.package.tree_hash)} /> : "—"}</Meta>
            <Meta label="Definition digest (approved)">{s.digest ? <CopyText value={s.digest} display={shortHash(s.digest)} /> : "—"}</Meta>
            <Meta label="Highest tool risk">{s.max_risk}</Meta>
            <Meta label="Files">{s.package ? `${s.package.file_count} · ${(s.package.size / 1024).toFixed(1)} KiB` : s.files.length}</Meta>
          </CardContent>
        </Card>
        {s.frontmatter && (
          <Card>
            <CardHeader>
              <CardTitle>SKILL.md front matter</CardTitle>
              <CardDescription>The YAML header as written: name, description, license, metadata and any other properties.</CardDescription>
            </CardHeader>
            <CardContent>
              <FrontMatter value={s.frontmatter} />
            </CardContent>
          </Card>
        )}
        <Card>
          <CardHeader>
            <CardTitle>Files</CardTitle>
            <CardDescription>
              Every file of this version, exactly as stored and covered by the approval. {s.compared_with ? `Changes are marked against ${s.compared_with}, the version agents use now.` : ""} Agents receive the
              SKILL.md text and the text reference files (Markdown, JSON, YAML, TOML, text). Scripts and binaries are only listed, never run.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <SkillFiles files={s.files} removed={s.removed_files} />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Approval history</CardTitle>
            <CardDescription>All versions of this skill, from the EAEH approval service.</CardDescription>
          </CardHeader>
          <CardContent>
            <ol className="flex flex-col gap-2 text-sm" data-testid="skill-history">
              {s.history.map((h) => (
                <li key={h.approval_id} className="rounded-md border p-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <code>{h.version}</code>
                    <span className="text-xs">{humanize(h.status)}</span>
                    <span className="text-xs text-muted-foreground">requested {formatDateTime(h.created_at)}</span>
                    <Link className="ml-auto text-xs hover:underline" to={`/approvals/${h.approval_id}`}>
                      Open
                    </Link>
                  </div>
                  {h.decision && (
                    <div className="mt-1 text-xs">
                      {h.decision.approver_id} · {h.decision.decision} · {formatDateTime(h.decision.decided_at)}
                      {h.decision.comments && <p className="mt-1 whitespace-pre-wrap">{h.decision.comments}</p>}
                    </div>
                  )}
                </li>
              ))}
            </ol>
          </CardContent>
        </Card>
      </div>
    </>
  );
}

function Meta({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="break-words">{children}</div>
    </div>
  );
}
