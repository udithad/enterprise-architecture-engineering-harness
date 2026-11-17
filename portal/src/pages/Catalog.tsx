import { useSearchParams } from "react-router";
import { useCatalogList } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState, ErrorState, LoadingRows } from "@/components/States";
import { Card } from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatDateTime, shortHash } from "@/lib/format";

interface Source { id: string; name: string; kind: string; description: string; owner_id: string; created_at: string }
interface Doc { id: string; source: string; subject: string; version: number; title: string; format: string; content_hash: string; scope: unknown; classification: string; mandatory: boolean; status: string }
interface Agent { definition: { id: string; version: string; purpose: string; capabilities: string[]; skills: string[]; allowed_tools: string[]; autonomy: string }; status: string }
interface Tool { id: string; version: string; description: string; risk: string; needs_execution: boolean; needs_workspace: boolean }

function Shell<T>({ path, children, empty }: { path: string; empty: string; children: (items: T[]) => React.ReactNode }) {
  const q = useCatalogList<T>(path);
  if (q.isPending) return <div className="p-4"><LoadingRows /></div>;
  if (q.isError) return <div className="p-4"><ErrorState error={q.error} onRetry={() => void q.refetch()} /></div>;
  if (!q.data.items.length) return <div className="p-4"><EmptyState title={empty} /></div>;
  return <>{children(q.data.items)}</>;
}

const scopeText = (s: unknown) => (typeof s === "string" ? s : s && typeof s === "object" ? Object.entries(s).map(([k, v]) => `${k}:${String(v)}`).join(" ") : "—");

/** Read-only inventory of registered knowledge, agents and tools (management stays with the CLI). */
export function CatalogPage() {
  const [params, setParams] = useSearchParams();
  const tab = params.get("tab") ?? "knowledge";
  return (
    <>
      <PageHeader title="Knowledge, agents and tools" description="Read-only. Knowledge ingestion, retirement and deletion, and agent and tool registration, stay with the eaeh CLI (administrative operations)." />
      <Tabs value={tab} onValueChange={(v) => setParams({ tab: v }, { replace: true })} className="mb-4">
        <TabsList>
          <TabsTrigger value="knowledge">Knowledge documents</TabsTrigger>
          <TabsTrigger value="sources">Knowledge sources</TabsTrigger>
          <TabsTrigger value="agents">Agents</TabsTrigger>
          <TabsTrigger value="tools">Tools</TabsTrigger>
        </TabsList>
      </Tabs>
      <Card className="overflow-hidden">
        {tab === "sources" && (
          <Shell<Source> path="/v1/knowledge/sources" empty="No knowledge sources">
            {(items) => (
              <Table aria-label="Knowledge sources">
                <TableHeader><TableRow><TableHead>Name</TableHead><TableHead>Kind</TableHead><TableHead>Description</TableHead><TableHead>Owner</TableHead><TableHead>Created</TableHead></TableRow></TableHeader>
                <TableBody>
                  {items.map((s) => (
                    <TableRow key={s.id}><TableCell className="font-medium">{s.name}</TableCell><TableCell className="text-xs">{s.kind}</TableCell><TableCell className="text-xs">{s.description}</TableCell><TableCell className="text-xs">{s.owner_id}</TableCell><TableCell className="text-xs">{formatDateTime(s.created_at)}</TableCell></TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Shell>
        )}
        {tab === "knowledge" && (
          <Shell<Doc> path="/v1/knowledge/documents?limit=100" empty="No knowledge documents you can read">
            {(items) => (
              <Table aria-label="Knowledge documents">
                <TableHeader><TableRow><TableHead>Document</TableHead><TableHead>Citation</TableHead><TableHead>Scope</TableHead><TableHead>Classification</TableHead><TableHead>Status</TableHead><TableHead>Hash</TableHead></TableRow></TableHeader>
                <TableBody>
                  {items.map((d) => (
                    <TableRow key={d.id}>
                      <TableCell className="font-medium">{d.title}{d.mandatory && <span className="ml-1 text-xs text-amber-800">(mandatory)</span>}</TableCell>
                      <TableCell className="font-mono text-xs">{d.source}/{d.subject}@v{d.version}</TableCell>
                      <TableCell className="text-xs">{scopeText(d.scope)}</TableCell>
                      <TableCell className="text-xs">{d.classification}</TableCell>
                      <TableCell className="text-xs">{d.status}</TableCell>
                      <TableCell className="font-mono text-xs">{shortHash(d.content_hash)}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Shell>
        )}
        {tab === "agents" && (
          <Shell<Agent> path="/v1/agents" empty="No agents registered">
            {(items) => (
              <Table aria-label="Agents">
                <TableHeader><TableRow><TableHead>Agent</TableHead><TableHead>Status</TableHead><TableHead>Purpose</TableHead><TableHead>Capabilities</TableHead><TableHead>Skills</TableHead><TableHead>Tools</TableHead></TableRow></TableHeader>
                <TableBody>
                  {items.map(({ definition: a, status }) => (
                    <TableRow key={`${a.id}@${a.version}`}>
                      <TableCell className="font-medium">{a.id}@{a.version}</TableCell>
                      <TableCell className="text-xs">{status}</TableCell>
                      <TableCell className="max-w-sm text-xs">{a.purpose}</TableCell>
                      <TableCell className="text-xs">{a.capabilities.join(", ")}</TableCell>
                      <TableCell className="text-xs">{a.skills.join(", ") || "—"}</TableCell>
                      <TableCell className="text-xs">{a.allowed_tools.join(", ") || "—"}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Shell>
        )}
        {tab === "tools" && (
          <Shell<Tool> path="/v1/tools" empty="No tools registered">
            {(items) => (
              <Table aria-label="Tools">
                <TableHeader><TableRow><TableHead>Tool</TableHead><TableHead>Risk</TableHead><TableHead>Description</TableHead><TableHead>Runs processes</TableHead><TableHead>Workspace</TableHead></TableRow></TableHeader>
                <TableBody>
                  {items.map((t) => (
                    <TableRow key={`${t.id}@${t.version}`}><TableCell className="font-medium">{t.id}@{t.version}</TableCell><TableCell className="text-xs">{t.risk}</TableCell><TableCell className="text-xs">{t.description}</TableCell><TableCell className="text-xs">{t.needs_execution ? "yes" : "no"}</TableCell><TableCell className="text-xs">{t.needs_workspace ? "yes" : "no"}</TableCell></TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Shell>
        )}
      </Card>
    </>
  );
}
