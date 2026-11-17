import { useState } from "react";
import { Link } from "react-router";
import { ChevronUp, Folder, FolderGit2, Search } from "lucide-react";
import { useBrowse, useInspect, useRegisterProject, useSession } from "@/hooks/queries";
import { describeError } from "@/lib/api";
import { PageHeader } from "@/components/PageHeader";
import { ErrorState, LoadingRows } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Inspection, RegisterInput } from "@/lib/types";

const list = (s: string) =>
  s
    .split(/[\n,]/)
    .map((x) => x.trim())
    .filter(Boolean);

function Browser({ onPick }: { onPick: (path: string) => void }) {
  const [path, setPath] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const q = useBrowse(path);
  return (
    <Card>
      <CardHeader>
        <CardTitle>1 · Choose the project folder</CardTitle>
        <CardDescription>Browsing is limited to the portal's workspace roots. Nothing is copied or moved: EAEH registers the folder in place.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (typed.trim()) onPick(typed.trim());
          }}
        >
          <Input aria-label="Project folder path" placeholder="/path/to/your/project (inside a workspace root)" value={typed} onChange={(e) => setTyped(e.target.value)} />
          <Button type="submit" variant="outline">
            <Search /> Inspect
          </Button>
        </form>
        {q.isPending && <LoadingRows rows={3} />}
        {q.isError && <ErrorState error={q.error} onRetry={() => setPath(null)} />}
        {q.data && (
          <div className="flex flex-col gap-1 text-sm">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span className="font-mono">{q.data.path || "Workspace roots"}</span>
              {(q.data.parent || q.data.path) && (
                <Button size="sm" variant="ghost" onClick={() => setPath(q.data.parent)}>
                  <ChevronUp /> Up
                </Button>
              )}
            </div>
            <ul className="max-h-72 overflow-y-auto rounded-md border" aria-label="Folders">
              {q.data.entries.map((e) => (
                <li key={e.path} className="flex items-center gap-2 border-b px-3 py-1.5 last:border-0">
                  <button type="button" className="flex flex-1 items-center gap-2 text-left hover:underline" onClick={() => setPath(e.path)}>
                    {e.project_hint ? <FolderGit2 className="size-4 text-emerald-700" aria-hidden /> : <Folder className="size-4 text-muted-foreground" aria-hidden />}
                    {e.name}
                  </button>
                  {e.project_hint && (
                    <Button size="sm" variant="outline" onClick={() => onPick(e.path)}>
                      Select
                    </Button>
                  )}
                </li>
              ))}
              {!q.data.entries.length && <li className="px-3 py-2 text-xs text-muted-foreground">No sub-folders.</li>}
            </ul>
            {q.data.path && (
              <Button size="sm" className="self-start" onClick={() => onPick(q.data.path)}>
                Select this folder
              </Button>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function Field({ id, label, value, onChange, hint, multiline }: { id: string; label: string; value: string; onChange: (v: string) => void; hint?: string; multiline?: boolean }) {
  return (
    <div className="flex flex-col gap-1">
      <Label htmlFor={id}>{label}</Label>
      {multiline ? (
        <textarea id={id} className="min-h-16 rounded-md border bg-background px-2 py-1 text-sm" value={value} onChange={(e) => onChange(e.target.value)} />
      ) : (
        <Input id={id} value={value} onChange={(e) => onChange(e.target.value)} />
      )}
      {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
    </div>
  );
}

interface Form {
  name: string;
  description: string;
  language: string;
  framework: string;
  build_tool: string;
  constraints: string;
  brd: string;
  knowledge: string;
  standards: string;
  developer: string;
  steward: string;
  approvers: string;
}

function initial(i: Inspection): Form {
  const d = i.detection;
  return {
    name: d.suggested_name,
    description: d.description,
    language: d.language,
    framework: d.framework,
    build_tool: d.build_tool,
    constraints: "",
    brd: d.documents.brd_candidates[0] ?? "",
    knowledge: [d.documents.readme, ...d.documents.architecture, ...d.documents.adrs].filter(Boolean).join("\n"),
    standards: "",
    developer: "developer",
    steward: "steward",
    approvers: "approver",
  };
}

export function RegisterProjectPage() {
  const session = useSession();
  const inspect = useInspect();
  const register = useRegisterProject();
  const [form, setForm] = useState<Form | null>(null);
  const [confirmUpdate, setConfirmUpdate] = useState(false);
  const insp = inspect.data;
  const set = (k: keyof Form) => (v: string) => setForm((f) => (f ? { ...f, [k]: v } : f));

  if (session.data && !session.data.can_register_projects) {
    return (
      <>
        <PageHeader title="Register existing project" />
        <Card>
          <CardContent className="pt-5 text-sm">
            Registering projects needs an administrator identity and workspace roots. Restart the portal with{" "}
            <code>eaeh portal --admin &lt;admin-profile&gt; [--workspace-root &lt;folder&gt;]</code>, or use <code>eaeh project init</code> in the project folder.
          </CardContent>
        </Card>
      </>
    );
  }

  if (register.data) {
    const r = register.data;
    return (
      <>
        <PageHeader title="Project registered" description={r.workspace} />
        <Card>
          <CardContent className="flex flex-col gap-2 pt-5 text-sm">
            <ul className="list-disc pl-5 text-xs">{r.actions.map((a, i) => <li key={i}>{a}</li>)}</ul>
            {!!r.handover.length && <ul className="list-disc pl-5 text-xs text-amber-800">{r.handover.map((a, i) => <li key={i}>{a}</li>)}</ul>}
            <p>
              Readiness: <b>{r.readiness.ready ? "ready" : `${r.readiness.blocking} blocking item(s)`}</b>
            </p>
            <ul className="text-xs">
              {r.readiness.checks.filter((c) => c.status !== "ok").map((c, i) => (
                <li key={i}>
                  {c.area}: {c.message} {c.fix && <code>{c.fix}</code>}
                </li>
              ))}
            </ul>
            <Button asChild className="self-start">
              <Link to={`/projects/${r.project}`}>Open project</Link>
            </Button>
          </CardContent>
        </Card>
      </>
    );
  }

  return (
    <>
      <PageHeader
        eyebrow={<Link className="hover:underline" to="/projects">Projects</Link>}
        title="Register existing project"
        description="Register a local project folder with EAEH: detected information, your review, then registration through the same service as `eaeh project init`."
      />
      <div className="flex flex-col gap-6">
        <Browser
          onPick={(p) =>
            inspect.mutate(p, {
              onSuccess: (i) => {
                setForm(initial(i));
                setConfirmUpdate(false);
              },
            })
          }
        />
        {inspect.isError && (
          <p role="alert" className="text-sm text-red-700">
            {describeError(inspect.error).detail}
          </p>
        )}
        {insp && form && (
          <Card>
            <CardHeader>
              <CardTitle>2 · Review what was detected</CardTitle>
              <CardDescription className="font-mono">{insp.detection.path}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4 text-sm">
              <dl className="grid grid-cols-2 gap-2 text-xs md:grid-cols-4">
                <div>
                  <dt className="text-muted-foreground">Build files</dt>
                  <dd>{insp.detection.build_files.join(", ") || "none"}</dd>
                </div>
                <div>
                  <dt className="text-muted-foreground">Sources</dt>
                  <dd>{insp.detection.source_dirs.join(", ") || "—"}</dd>
                </div>
                <div>
                  <dt className="text-muted-foreground">Git</dt>
                  <dd>{insp.detection.git.present ? `${insp.detection.git.head ?? ""} ${insp.detection.git.remote ?? ""}` : "no repository"}</dd>
                </div>
                <div>
                  <dt className="text-muted-foreground">Documents found</dt>
                  <dd>
                    {insp.detection.documents.brd_candidates.length} BRD · {insp.detection.documents.architecture.length} architecture · {insp.detection.documents.adrs.length} ADR ·{" "}
                    {insp.detection.documents.standards.length} standards · {insp.detection.documents.knowledge.length} other
                  </dd>
                </div>
              </dl>
              {insp.detection.warnings.map((w, i) => (
                <p key={i} className="rounded border border-amber-200 bg-amber-50 px-2 py-1 text-xs text-amber-900">
                  {w}
                </p>
              ))}
              {insp.registration.state === "registered" && (
                <div className="rounded border border-sky-200 bg-sky-50 px-2 py-1 text-xs">
                  Already registered as <Link className="underline" to={`/projects/${insp.registration.project_id}`}>{insp.registration.name}</Link>.{" "}
                  <label className="ml-2 inline-flex items-center gap-1">
                    <input type="checkbox" checked={confirmUpdate} onChange={(e) => setConfirmUpdate(e.target.checked)} /> update its registration
                  </label>
                </div>
              )}
              {insp.registration.state === "foreign" && (
                <p className="rounded border border-red-200 bg-red-50 px-2 py-1 text-xs text-red-900">
                  This folder carries the manifest of project {insp.registration.project_id} from another EAEH installation.
                </p>
              )}
              <div className="grid gap-3 md:grid-cols-2">
                <Field id="r-name" label="Project name" value={form.name} onChange={set("name")} />
                <Field id="r-desc" label="Description" value={form.description} onChange={set("description")} />
                <Field id="r-lang" label="Language" value={form.language} onChange={set("language")} />
                <Field id="r-fw" label="Framework" value={form.framework} onChange={set("framework")} />
                <Field id="r-build" label="Build tool" value={form.build_tool} onChange={set("build_tool")} />
                <Field id="r-constraints" label="Technical constraints (one per line)" value={form.constraints} onChange={set("constraints")} multiline />
                <Field id="r-brd" label="BRD (relative path)" value={form.brd} onChange={set("brd")} hint={insp.detection.documents.brd_candidates.length ? `Found: ${insp.detection.documents.brd_candidates.join(", ")}` : "No BRD found; add one before starting a design workflow."} />
                <Field id="r-knowledge" label="Knowledge documents (one per line)" value={form.knowledge} onChange={set("knowledge")} multiline />
                <Field id="r-standards" label="Project standards (one per line; need approval)" value={form.standards} onChange={set("standards")} multiline hint={insp.detection.documents.standards.length ? `Found: ${insp.detection.documents.standards.join(", ")}` : undefined} />
                <Field id="r-dev" label="Developer (principal id)" value={form.developer} onChange={set("developer")} />
                <Field id="r-steward" label="Steward (principal id)" value={form.steward} onChange={set("steward")} />
                <Field id="r-approvers" label="Approvers (principal ids)" value={form.approvers} onChange={set("approvers")} hint="Different people from the developer (separation of duties)." />
              </div>
              <p className="text-xs text-muted-foreground">
                Registration writes <code>.eaeh/project.toml</code>, <code>CLAUDE.md</code> and <code>.mcp.json</code> in the folder (no secrets), ingests the chosen documents as project knowledge, and creates or grants the people. New people's tokens go to this computer's credential store, never to the browser. Source code is not copied or moved.
              </p>
              {register.isError && (
                <p role="alert" className="text-sm text-red-700">
                  {describeError(register.error).detail}
                </p>
              )}
              <Button
                className="self-start"
                disabled={register.isPending || insp.registration.state === "foreign" || (insp.registration.state === "registered" && !confirmUpdate)}
                onClick={() => {
                  const input: RegisterInput = {
                    path: insp.detection.path,
                    name: form.name.trim(),
                    description: form.description.trim(),
                    language: form.language.trim(),
                    framework: form.framework.trim(),
                    build_tool: form.build_tool.trim(),
                    constraints: list(form.constraints),
                    brd: form.brd.trim() || null,
                    knowledge: list(form.knowledge),
                    standards: list(form.standards),
                    developer: form.developer.trim(),
                    steward: form.steward.trim(),
                    approvers: list(form.approvers),
                    update: confirmUpdate,
                  };
                  register.mutate(input);
                }}
              >
                3 · Register project
              </Button>
            </CardContent>
          </Card>
        )}
      </div>
    </>
  );
}
