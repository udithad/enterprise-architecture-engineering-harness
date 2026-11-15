import { Bot, Cpu, FileCog, PlayCircle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import type { Generation } from "@/lib/types";

const INFO = {
  external_ai: { label: "AI client", cls: "border-violet-200 bg-violet-50 text-violet-800", icon: <Bot /> },
  deterministic_replay: { label: "Recorded fixture – not AI", cls: "border-amber-300 bg-amber-50 text-amber-900", icon: <PlayCircle /> },
  direct_provider: { label: "Model provider", cls: "border-sky-200 bg-sky-50 text-sky-800", icon: <Cpu /> },
  platform: { label: "Platform step (no model)", cls: "border-slate-200 bg-slate-50 text-slate-700", icon: <FileCog /> },
} as const;

/** Where an artifact's content came from, from its recorded provenance. */
export function GenerationBadge({ g }: { g: Generation }) {
  const i = INFO[g.source];
  const client = g.models.find((m) => m.startsWith("external:"))?.slice("external:".length);
  const model = g.source === "direct_provider" ? g.models[0] : undefined;
  return (
    <Badge className={i.cls} title={g.models.join(", ") || g.produced_by} data-source={g.source}>
      {i.icon}
      {i.label}
      {client ? `: ${client}` : model ? `: ${model}` : ""}
    </Badge>
  );
}

/** Banner when a run's documents were produced by recorded fixtures instead of AI. */
export function ReplayNotice({ provenance, className }: { provenance: Record<string, Generation> | undefined; className?: string }) {
  const replay = Object.values(provenance ?? {}).some((g) => g.source === "deterministic_replay");
  if (!replay) return null;
  return (
    <div role="note" className={`${className ?? ""} rounded-md border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-950`}>
      <b>Deterministic mode:</b> the agent stages of this run replayed recorded, hand-authored responses (<code>replay/…</code> in provenance). They are{" "}
      <b>not AI-generated</b> and complete in well under a second. In <code>external_ai</code> mode the same stages wait for your AI client (Claude Code / Copilot via MCP) and show
      "Waiting for AI" until it submits a validated result.
    </div>
  );
}
