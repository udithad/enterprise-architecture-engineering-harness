import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { DocumentViewer } from "@/components/DocumentViewer";
import { CopyText } from "@/components/CopyText";
import { shortHash } from "@/lib/format";
import { Link } from "react-router";
import { useSkillFiles } from "@/hooks/queries";
import { SkillFiles } from "@/components/SkillFiles";
import { FrontMatter } from "@/components/FrontMatter";
import type { SkillView } from "@/lib/types";

/** The complete skill under review: metadata, the layers it is applied to, and the
 * instruction text exactly as agents receive it. */
export function SkillReview({ skill }: { skill: SkillView }) {
  return (
    <div className="flex flex-col gap-6" data-testid="skill-review">
      <Card>
        <CardHeader>
          <CardTitle>
            Skill <code>{skill.reference}</code>
          </CardTitle>
          <CardDescription>{skill.description}</CardDescription>
        </CardHeader>
        <CardContent>
          <dl className="grid grid-cols-1 gap-x-6 gap-y-3 text-sm md:grid-cols-2">
            <Row label="Applied at (layers)">
              {skill.layers.length ? (
                <ul className="list-disc pl-5" data-testid="skill-layers">
                  {skill.layers.map((l) => (
                    <li key={l}>
                      <code className="text-xs">{l}</code>
                    </li>
                  ))}
                </ul>
              ) : (
                <span className="text-muted-foreground">no current workflow stage (task kinds: {skill.task_kinds.join(", ") || "none"})</span>
              )}
            </Row>
            <Row label="Task kinds">
              {skill.task_kinds.join(", ") || "—"}
              {skill.layers_from && <div className="text-xs text-muted-foreground" data-testid="layers-from">layers: {skill.layers_from}</div>}
            </Row>
            <Row label="Highest tool risk">{skill.max_risk}</Row>
            <Row label="Required tools">{skill.required_tools.join(", ") || "none"}</Row>
            <Row label="Required knowledge">{skill.required_knowledge.length ? skill.required_knowledge.map((k) => `"${k.query}" (top ${k.k})`).join("; ") : "none"}</Row>
            <Row label="Dependencies">{skill.dependencies.length ? skill.dependencies.map((d) => `${d.id}@${d.version}`).join(", ") : "none"}</Row>
            <Row label="Evaluation criteria">{skill.evaluation_criteria.length ? skill.evaluation_criteria.join("; ") : "—"}</Row>
            <Row label="Source file">{skill.file ? <code className="break-all text-xs">{skill.file}</code> : "—"}</Row>
            <Row label="Definition digest">{skill.digest ? <CopyText value={skill.digest} display={shortHash(skill.digest)} /> : "—"}</Row>
            <Row label="Registered by">{skill.registered_by ?? "—"}</Row>
          </dl>
          <p className="mt-3 text-xs text-muted-foreground">
            A skill never grants permissions. It binds to an agent only if the agent already allows its tools and its risk level. Once approved, its instructions are added to every matching stage, and its reference is recorded in each document's provenance.
          </p>
        </CardContent>
      </Card>
      {skill.frontmatter && (
        <Card>
          <CardHeader>
            <CardTitle>SKILL.md front matter</CardTitle>
            <CardDescription>Every property as written in the skill's YAML header, including license and metadata. All of it is part of what you approve.</CardDescription>
          </CardHeader>
          <CardContent>
            <FrontMatter value={skill.frontmatter} />
          </CardContent>
        </Card>
      )}
      {skill.package && <PackageFiles skill={skill} />}
      <Card>
        <CardHeader>
          <CardTitle>Instructions</CardTitle>
          <CardDescription>The exact text agents receive, as `APPROVED SKILL [{skill.reference}]`.</CardDescription>
        </CardHeader>
        <CardContent>
          <DocumentViewer
            files={[{ path: "instructions.md", artifact_id: skill.reference, name: skill.reference, version: 0, hash: skill.digest ?? "", media_type: "text/markdown", content: skill.instructions }]}
          />
        </CardContent>
      </Card>
    </div>
  );
}

/** Every file of a skill directory under review (nested tree, previews, hashes, changes). */
function PackageFiles({ skill }: { skill: SkillView }) {
  const q = useSkillFiles(skill.id, skill.version);
  return (
    <Card>
      <CardHeader>
        <CardTitle>
          Files · {skill.package!.file_count} in <code>{skill.package!.dir}/</code>
        </CardTitle>
        <CardDescription>
          Manifest {skill.package!.source_version}, revision {skill.package!.revision}. The approval covers every file (tree hash {shortHash(skill.package!.tree_hash)}).{" "}
          {q.data?.compared_with && `Changes are marked against ${q.data.compared_with}, the version in force.`}{" "}
          <Link className="underline" to={`/skills/${encodeURIComponent(skill.id)}/${encodeURIComponent(skill.version)}`}>
            Open full view
          </Link>
        </CardDescription>
      </CardHeader>
      <CardContent>{q.data ? <SkillFiles files={q.data.files} removed={q.data.removed_files} /> : <span className="text-xs text-muted-foreground">Loading files…</span>}</CardContent>
    </Card>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="break-words">{children}</dd>
    </div>
  );
}
