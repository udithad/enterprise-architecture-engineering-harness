import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { Play } from "lucide-react";
import { useDefinitions, useEnsureWorker, useProjectOverview, useProjectPlan, useSession, useStartWorkflow } from "@/hooks/queries";
import { describeError } from "@/lib/api";
import { PageHeader } from "@/components/PageHeader";
import { ErrorState, LoadingRows } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { humanize } from "@/lib/format";

/**
 * Start a workflow for a project: the definitions come from the backend, the execution plan
 * and readiness from the same planner as `eaeh workflow run`, and the start from the engine.
 */
export function StartWorkflowPage() {
  const { id = "" } = useParams();
  const session = useSession();
  const overview = useProjectOverview(id);
  const defs = useDefinitions();
  const [workflow, setWorkflow] = useState("");
  const [inputText, setInputText] = useState<string | null>(null);
  const [inputError, setInputError] = useState<string | null>(null);
  const plan = useProjectPlan(id, workflow);
  const start = useStartWorkflow();
  const worker = useEnsureWorker();
  const navigate = useNavigate();

  if (overview.isPending || defs.isPending) return <LoadingRows rows={4} />;
  if (overview.isError) return <ErrorState error={overview.error} onRetry={() => void overview.refetch()} />;
  const configured = overview.data.manifest?.workflows ?? [];
  const available = [...new Set((defs.data?.items ?? []).map((d) => d.id))];
  const options = configured.length ? configured.filter((w) => available.includes(w)) : available;
  const p = plan.data?.plan;
  const input = inputText ?? (p ? JSON.stringify(p.input ?? {}, null, 2) : "");
  const canOperate = !!session.data?.can_operate;

  return (
    <>
      <PageHeader
        eyebrow={<Link className="hover:underline" to={`/projects/${id}`}>{overview.data.project.name}</Link>}
        title="Start workflow"
        description="Review the execution plan and readiness before starting. Nothing runs until you confirm."
      />
      <div className="flex flex-col gap-6">
        <Card>
          <CardHeader>
            <CardTitle>1 · Workflow</CardTitle>
            <CardDescription>{configured.length ? "Workflows enabled for this project (from its manifest)." : "Registered workflow definitions."}</CardDescription>
          </CardHeader>
          <CardContent>
            <Label htmlFor="s-workflow">Workflow definition</Label>
            <select
              id="s-workflow"
              className="mt-1 h-9 w-full max-w-md rounded-md border bg-background px-2 text-sm"
              value={workflow}
              onChange={(e) => {
                setWorkflow(e.target.value);
                setInputText(null);
              }}
            >
              <option value="">Choose…</option>
              {options.map((w) => (
                <option key={w} value={w}>
                  {w} – {defs.data?.items.find((d) => d.id === w)?.description}
                </option>
              ))}
            </select>
          </CardContent>
        </Card>
        {workflow && plan.isPending && <LoadingRows rows={3} label="Planning" />}
        {plan.isError && <ErrorState error={plan.error} onRetry={() => void plan.refetch()} />}
        {p && (
          <>
            <Card>
              <CardHeader>
                <CardTitle>2 · Execution plan</CardTitle>
                <CardDescription>{p.description}</CardDescription>
              </CardHeader>
              <CardContent className="px-0">
                <Table aria-label="Execution plan">
                  <TableHeader>
                    <TableRow>
                      <TableHead>Stage</TableHead>
                      <TableHead>Kind</TableHead>
                      <TableHead>Agent</TableHead>
                      <TableHead>Skills</TableHead>
                      <TableHead>Detail</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {p.stages.map((s) => (
                      <TableRow key={s.state}>
                        <TableCell>{humanize(s.state)}</TableCell>
                        <TableCell className="text-xs">{s.kind}</TableCell>
                        <TableCell className="text-xs">{s.agent ?? "—"}</TableCell>
                        <TableCell className="text-xs">{s.skills.join(", ") || "—"}</TableCell>
                        <TableCell className="text-xs">{s.detail ?? ""}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
                <div className="px-5 pt-3 text-xs">
                  Human approvals: <ul className="list-disc pl-5">{p.approvals.map((a) => <li key={a}>{a}</li>)}</ul>
                </div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader>
                <CardTitle>3 · Inputs and readiness</CardTitle>
              </CardHeader>
              <CardContent className="flex flex-col gap-3 text-sm">
                <Label htmlFor="s-input">Workflow input (the project's governed BRD by default)</Label>
                <textarea id="s-input" className="min-h-20 rounded-md border bg-background p-2 font-mono text-xs" value={input} onChange={(e) => setInputText(e.target.value)} />
                {inputError && <p className="text-xs text-red-700">{inputError}</p>}
                <ul className="flex flex-col gap-1 text-xs" aria-label="Readiness">
                  {p.readiness.checks.map((c, i) => (
                    <li key={i} className={c.status === "blocking" ? "text-red-700" : c.status === "warning" ? "text-amber-800" : "text-emerald-800"}>
                      [{c.status}] {c.area}: {c.message} {c.fix && <code className="text-muted-foreground">{c.fix}</code>}
                    </li>
                  ))}
                </ul>
                {!canOperate && (
                  <p className="text-xs">
                    Starting needs the portal to run with <code>--operator &lt;developer-profile&gt;</code>, or run <code>eaeh workflow run {p.activity} --yes</code> in the project folder.
                  </p>
                )}
                {start.isError && <p role="alert" className="text-sm text-red-700">{describeError(start.error).detail}</p>}
                <Button
                  className="self-start"
                  disabled={!canOperate || !p.readiness.ready || start.isPending}
                  onClick={() => {
                    let parsed: unknown;
                    try {
                      parsed = JSON.parse(input || "{}");
                      setInputError(null);
                    } catch {
                      setInputError("The input must be valid JSON.");
                      return;
                    }
                    start.mutate(
                      { definition: p.workflow, project_id: id, input: parsed },
                      {
                        onSuccess: (r) => {
                          // As `eaeh workflow run`: start the worker when AI work waits.
                          worker.mutate(id);
                          navigate(`/workflows/${r.id}`);
                        },
                      },
                    );
                  }}
                >
                  <Play /> Start {p.workflow}
                </Button>
                {!p.readiness.ready && <p className="text-xs text-red-700">Resolve the blocking items first.</p>}
              </CardContent>
            </Card>
          </>
        )}
      </div>
    </>
  );
}
