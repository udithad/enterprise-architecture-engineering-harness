import { useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { ChevronDown, ChevronRight, FileCode2, FileJson, FileText, FileType, Folder, Lock } from "lucide-react";
import { CopyText } from "@/components/CopyText";
import { EmptyState } from "@/components/States";
import { shortHash } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { SkillFileEntry } from "@/lib/types";

/** Directory tree node built from `/`-separated paths. */
interface Node {
  name: string;
  path: string;
  file?: SkillFileEntry;
  children: Node[];
}

export function buildTree(files: SkillFileEntry[]): Node {
  const root: Node = { name: "", path: "", children: [] };
  for (const f of files) {
    const parts = f.path.split("/");
    let cur = root;
    parts.forEach((part, i) => {
      const path = parts.slice(0, i + 1).join("/");
      let next = cur.children.find((c) => c.name === part && !!c.file === (i === parts.length - 1));
      if (!next) {
        next = { name: part, path, children: [], file: i === parts.length - 1 ? f : undefined };
        cur.children.push(next);
      }
      cur = next;
    });
  }
  const sort = (n: Node) => {
    // Folders first, then SKILL.md, then alphabetical.
    n.children.sort((a, b) => Number(!!a.file) - Number(!!b.file) || Number(b.name === "SKILL.md") - Number(a.name === "SKILL.md") || a.name.localeCompare(b.name));
    n.children.forEach(sort);
  };
  sort(root);
  return root;
}

const ICON: Record<string, React.ReactNode> = {
  markdown: <FileText className="size-3.5" />,
  json: <FileJson className="size-3.5" />,
  yaml: <FileType className="size-3.5" />,
  toml: <FileType className="size-3.5" />,
  code: <FileCode2 className="size-3.5" />,
  text: <FileText className="size-3.5" />,
  binary: <Lock className="size-3.5" />,
};

const CHANGE: Record<string, string> = { added: "bg-emerald-100 text-emerald-900", modified: "bg-amber-100 text-amber-900" };

function TreeNode({ node, depth, selected, onSelect, open, toggle }: { node: Node; depth: number; selected: string; onSelect: (p: string) => void; open: Set<string>; toggle: (p: string) => void }) {
  if (node.file) {
    const f = node.file;
    return (
      <li>
        <button
          type="button"
          className={cn("flex w-full items-center gap-1.5 rounded px-1.5 py-0.5 text-left text-xs hover:bg-muted", selected === f.path && "bg-muted font-medium")}
          style={{ paddingLeft: 6 + depth * 14 }}
          onClick={() => onSelect(f.path)}
          aria-current={selected === f.path}
          data-testid={`tree-file-${f.path}`}
        >
          {ICON[f.kind] ?? ICON.text}
          <span className="truncate">{node.name}</span>
          {f.change && f.change !== "unchanged" && <span className={cn("ml-auto rounded px-1 text-[10px]", CHANGE[f.change])}>{f.change}</span>}
        </button>
      </li>
    );
  }
  const isOpen = open.has(node.path);
  return (
    <li>
      <button type="button" className="flex w-full items-center gap-1 rounded px-1.5 py-0.5 text-left text-xs font-medium hover:bg-muted" style={{ paddingLeft: 6 + depth * 14 }} onClick={() => toggle(node.path)} aria-expanded={isOpen}>
        {isOpen ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        <Folder className="size-3.5" /> {node.name}/
      </button>
      {isOpen && (
        <ul>
          {node.children.map((c) => (
            <TreeNode key={c.path + (c.file ? ":f" : ":d")} node={c} depth={depth + 1} selected={selected} onSelect={onSelect} open={open} toggle={toggle} />
          ))}
        </ul>
      )}
    </li>
  );
}

/** Preview by kind: Markdown rendered (no raw HTML), JSON pretty-printed, YAML/TOML/code/text
 * as numbered monospace lines, binaries by hash and size only. */
export function FilePreview({ file, raw }: { file: SkillFileEntry; raw?: boolean }) {
  if (file.content == null) return <EmptyState title="Binary file">Shown by hash and size only. It is never sent to agents.</EmptyState>;
  if (file.kind === "markdown" && !raw)
    return (
      <article className="doc-prose max-w-none" data-testid="preview-markdown">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{file.content}</ReactMarkdown>
      </article>
    );
  let text = file.content;
  if (file.kind === "json" && !raw) {
    try {
      text = JSON.stringify(JSON.parse(file.content), null, 2);
    } catch {
      /* show as stored */
    }
  }
  const lines = text.replace(/\n$/, "").split("\n");
  return (
    <div className="overflow-x-auto rounded-md bg-muted/50 py-2 font-mono text-xs leading-relaxed" data-testid={`preview-${file.kind}`}>
      <table className="w-full border-collapse">
        <tbody>
          {lines.map((l, i) => (
            <tr key={i}>
              <td className="select-none px-3 text-right align-top text-muted-foreground">{i + 1}</td>
              <td className="whitespace-pre pr-4">{l}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function SkillFiles({ files, removed = [] }: { files: SkillFileEntry[]; removed?: string[] }) {
  const tree = useMemo(() => buildTree(files), [files]);
  const allDirs = useMemo(() => {
    const s = new Set<string>();
    const walk = (n: Node) => n.children.forEach((c) => (c.file ? null : (s.add(c.path), walk(c))));
    walk(tree);
    return s;
  }, [tree]);
  const [open, setOpen] = useState<Set<string>>(allDirs);
  const [selected, setSelected] = useState(files.find((f) => f.path === "SKILL.md")?.path ?? files[0]?.path ?? "");
  const [raw, setRaw] = useState(false);
  const file = files.find((f) => f.path === selected);
  const toggle = (p: string) =>
    setOpen((o) => {
      const n = new Set(o);
      if (n.has(p)) n.delete(p);
      else n.add(p);
      return n;
    });
  if (!files.length) return <EmptyState title="No files" />;
  return (
    <div className="grid gap-4 lg:grid-cols-[260px_1fr]" data-testid="skill-files">
      <nav aria-label="Skill files" className="rounded-md border p-2">
        <div className="mb-1 px-1.5 text-[11px] uppercase tracking-wide text-muted-foreground">{files.length} files</div>
        <ul>
          {tree.children.map((c) => (
            <TreeNode key={c.path + (c.file ? ":f" : ":d")} node={c} depth={0} selected={selected} onSelect={setSelected} open={open} toggle={toggle} />
          ))}
        </ul>
        {removed.length > 0 && (
          <div className="mt-2 border-t pt-2 text-[11px] text-red-800">
            Removed since the version in force: {removed.join(", ")}
          </div>
        )}
      </nav>
      {file && (
        <section className="min-w-0">
          <div className="mb-2 flex flex-wrap items-center gap-3 text-xs">
            <code className="font-semibold">{file.path}</code>
            <span className="text-muted-foreground">
              {file.kind} · {file.size.toLocaleString()} bytes
            </span>
            <span className="text-muted-foreground">
              sha256 <CopyText value={file.sha256} display={shortHash(file.sha256)} />
            </span>
            {file.change && file.change !== "unchanged" && <span className={cn("rounded px-1.5", CHANGE[file.change])}>{file.change} since the version in force</span>}
            {(file.kind === "markdown" || file.kind === "json") && file.content != null && (
              <button type="button" className="ml-auto text-xs underline" onClick={() => setRaw((r) => !r)}>
                {raw ? "Rendered" : "Raw"}
              </button>
            )}
          </div>
          <FilePreview file={file} raw={raw} />
        </section>
      )}
    </div>
  );
}
