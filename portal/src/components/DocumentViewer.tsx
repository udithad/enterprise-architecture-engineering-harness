import { useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { FileText } from "lucide-react";
import { Button } from "@/components/ui/button";
import { CopyText } from "@/components/CopyText";
import { EmptyState } from "@/components/States";
import { shortHash } from "@/lib/format";
import type { DocumentFile } from "@/lib/types";
import { cn } from "@/lib/utils";

function kindOf(f: DocumentFile): "markdown" | "json" | "text" {
  const p = f.path.toLowerCase();
  if (p.endsWith(".md") || p.endsWith(".markdown") || f.media_type.includes("markdown")) return "markdown";
  if (p.endsWith(".json") || f.media_type.includes("json")) return "json";
  return "text";
}

function Rendered({ file, raw }: { file: DocumentFile; raw: boolean }) {
  if (file.content == null) return <EmptyState title="Binary content">This file is not text and cannot be shown in the browser.</EmptyState>;
  const kind = kindOf(file);
  if (raw || kind === "text") return <pre className="overflow-x-auto whitespace-pre-wrap break-words rounded-md bg-muted/50 p-4 font-mono text-xs leading-relaxed">{file.content}</pre>;
  if (kind === "json") {
    let pretty = file.content;
    try {
      pretty = JSON.stringify(JSON.parse(file.content), null, 2);
    } catch {
      /* show as stored */
    }
    return <pre className="overflow-x-auto rounded-md bg-muted/50 p-4 font-mono text-xs leading-relaxed">{pretty}</pre>;
  }
  // Markdown is rendered without raw HTML (react-markdown default): untrusted content.
  return (
    <article className="doc-prose max-w-none">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{file.content}</ReactMarkdown>
    </article>
  );
}

export function DocumentViewer({ files }: { files: DocumentFile[] }) {
  const ordered = useMemo(() => {
    // Primary documents (Markdown) first, then the rest in path order.
    return [...files].sort((a, b) => Number(kindOf(b) === "markdown") - Number(kindOf(a) === "markdown") || a.path.localeCompare(b.path));
  }, [files]);
  const [sel, setSel] = useState(0);
  const [raw, setRaw] = useState(false);
  const file = ordered[Math.min(sel, ordered.length - 1)];
  if (!file) return <EmptyState title="The approval has no document attached" />;
  return (
    <div className="flex flex-col gap-3 lg:flex-row">
      {ordered.length > 1 && (
        <nav aria-label="Document files" className="shrink-0 lg:w-56">
          <ul className="flex flex-row gap-1 overflow-x-auto lg:flex-col">
            {ordered.map((f, i) => (
              <li key={f.path}>
                <button
                  type="button"
                  onClick={() => setSel(i)}
                  aria-current={i === sel}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs whitespace-nowrap",
                    i === sel ? "bg-primary text-primary-foreground" : "hover:bg-muted",
                  )}
                >
                  <FileText className="size-3.5 shrink-0" aria-hidden />
                  <span className="truncate">{f.path}</span>
                </button>
              </li>
            ))}
          </ul>
        </nav>
      )}
      <div className="min-w-0 flex-1 rounded-md border">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b bg-muted/30 px-4 py-2 text-xs">
          <span className="font-medium">{file.path}</span>
          <span className="text-muted-foreground">v{file.version}</span>
          <CopyText value={file.hash} display={shortHash(file.hash)} className="text-muted-foreground" />
          {kindOf(file) !== "text" && (
            <Button size="sm" variant="ghost" className="ml-auto h-7" onClick={() => setRaw((r) => !r)} aria-pressed={raw}>
              {raw ? "Rendered" : "Source"}
            </Button>
          )}
        </div>
        <div className="max-h-[70vh] overflow-y-auto px-5 py-4" data-testid="document-content">
          <Rendered file={file} raw={raw} />
        </div>
      </div>
    </div>
  );
}
