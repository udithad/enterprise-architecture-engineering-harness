import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SkillFiles, buildTree } from "@/components/SkillFiles";
import { SkillsPage } from "@/pages/Skills";
import { SkillViewerPage } from "@/pages/SkillViewer";
import { mockFetch, renderAt, session } from "./helpers";
import type { SkillFileEntry } from "@/lib/types";

const H = (c: string) => c.repeat(64);
const files: SkillFileEntry[] = [
  { path: "SKILL.md", sha256: H("a"), size: 40, kind: "markdown", content: "# API error handling\n\nUse the **catalog**.", change: "unchanged" },
  { path: "references/http/status-mapping.md", sha256: H("b"), size: 30, kind: "markdown", content: "| Status | Code |\n|---|---|\n| 404 | RESOURCE_NOT_FOUND |", change: "modified" },
  { path: "references/extra.md", sha256: H("c"), size: 8, kind: "markdown", content: "# Extra", change: "added" },
  { path: "examples/error-response.json", sha256: H("d"), size: 20, kind: "json", content: '{"error":{"code":"X"}}', change: null },
  { path: "examples/config.yaml", sha256: H("e"), size: 20, kind: "yaml", content: "errors:\n  include_request_id: true\n", change: null },
  { path: "scripts/validate_errors.py", sha256: H("f"), size: 30, kind: "code", content: "def main():\n    return 0\n", change: null },
  { path: "assets/logo.png", sha256: H("0"), size: 7, kind: "binary", content: null, change: null },
];

describe("Skill file browser", () => {
  it("builds a nested tree with folders first and SKILL.md on top", () => {
    const t = buildTree(files);
    expect(t.children.map((c) => c.name)).toEqual(["assets", "examples", "references", "scripts", "SKILL.md"]);
    const refs = t.children.find((c) => c.name === "references")!;
    expect(refs.children.map((c) => c.name)).toEqual(["http", "extra.md"]);
    expect(refs.children[0]?.children[0]?.file?.path).toBe("references/http/status-mapping.md");
  });

  it("previews Markdown, JSON, YAML, code and binary files with hashes and change markers", () => {
    render(<SkillFiles files={files} removed={["references/old.md"]} />);
    // SKILL.md is selected first and rendered as Markdown.
    expect(screen.getByTestId("preview-markdown")).toHaveTextContent("API error handling");
    // Nested file with its change marker.
    const nested = screen.getByTestId("tree-file-references/http/status-mapping.md");
    expect(nested).toHaveTextContent("modified");
    fireEvent.click(nested);
    expect(screen.getByRole("table")).toHaveTextContent("RESOURCE_NOT_FOUND");
    expect(screen.getByText(/modified since the version in force/)).toBeInTheDocument();
    expect(screen.getByTestId("tree-file-references/extra.md")).toHaveTextContent("added");
    fireEvent.click(screen.getByTestId("tree-file-examples/error-response.json"));
    expect(screen.getByTestId("preview-json")).toHaveTextContent('"code": "X"');
    fireEvent.click(screen.getByTestId("tree-file-examples/config.yaml"));
    expect(screen.getByTestId("preview-yaml")).toHaveTextContent("include_request_id: true");
    fireEvent.click(screen.getByTestId("tree-file-scripts/validate_errors.py"));
    const code = screen.getByTestId("preview-code");
    expect(within(code).getAllByRole("row")).toHaveLength(2);
    expect(code).toHaveTextContent("def main():");
    fireEvent.click(screen.getByTestId("tree-file-assets/logo.png"));
    expect(screen.getByText("Binary file")).toBeInTheDocument();
    expect(screen.getByText(/Removed since the version in force: references\/old.md/)).toBeInTheDocument();
  });
});

describe("Skill onboarding pages", () => {
  it("scans for skill directories and shows the outcome", async () => {
    mockFetch({
      "/portal/session": session(),
      "/api/v1/projects": { items: [] },
      "/api/v1/skills/catalog": { skills_dir: "/eaeh/config/skills", file_error: null, items: [], layers: {}, directories: [{ dir: "broken", error: "must start with a `+++` line" }] },
      "/api/v1/skills/discover": { results: [{ dir: "api-error-handling", skill: "api-error-handling", version: "1.0.1", action: "changed", detail: "files changed: registered as 1.0.1", tree_hash: H("1"), approval_id: "ap-2" }] },
    });
    renderAt(<SkillsPage />);
    expect(await screen.findByText(/is not a valid skill/)).toHaveTextContent("broken/");
    fireEvent.click(screen.getByRole("button", { name: /Scan now/ }));
    expect(await screen.findByTestId("discovery-results")).toHaveTextContent("api-error-handling/: changed (1.0.1)");
  });

  it("shows a version's files, version in force and approval history", async () => {
    mockFetch({
      "/api/v1/skills/api-error-handling/1.0.1/files": {
        id: "api-error-handling", version: "1.0.1", reference: "api-error-handling@1.0.1", description: "Errors", task_kinds: ["lld-design"], capabilities: [],
        layers: ["engineering.design/lld"], required_tools: [], required_knowledge: [], max_risk: "read_only", dependencies: [], evaluation_criteria: [], output_schema: {},
        instructions: "x", package: { dir: "api-error-handling", source_version: "1.0.0", revision: 2, tree_hash: H("9"), file_count: files.length, size: 155 },
        digest: H("8"), status: "pending_approval", approval_id: "ap-2", registered_by: "system:skill-discovery",
        in_force_version: "1.0.0", compared_with: "1.0.0", files, removed_files: [],
        history: [
          { version: "1.0.1", approval_id: "ap-2", status: "pending", created_at: "2026-11-16T10:00:00Z", decided_at: null, decision: null },
          { version: "1.0.0", approval_id: "ap-1", status: "approved", created_at: "2026-11-15T10:00:00Z", decided_at: "2026-11-15T11:00:00Z", decision: { approver_id: "arthur", decision: "approve", comments: "fine", decided_at: "2026-11-15T11:00:00Z" } },
        ],
      },
    });
    renderAt(<SkillViewerPage />, "/skills/api-error-handling/1.0.1", "/skills/:id/:version");
    expect(await screen.findByText("Waiting for approval")).toBeInTheDocument();
    expect(screen.getByText(/manifest 1.0.0 · revision 2/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Review and decide/ })).toHaveAttribute("href", "/approvals/ap-2");
    expect(screen.getByTestId("skill-files")).toBeInTheDocument();
    expect(screen.getByTestId("skill-history")).toHaveTextContent("arthur · approve");
  });
});

describe("Community SKILL.md", () => {
  it("shows the YAML front matter and how the layers were determined", async () => {
    const { SkillReview } = await import("@/components/SkillReview");
    render(
      <SkillReview
        skill={{
          id: "architecture-designer", version: "1.1.1", reference: "architecture-designer@1.1.1", description: "Use when designing…",
          task_kinds: ["hld-design", "lld-design"], capabilities: [], layers: ["engineering.design/hld", "engineering.design/lld"], required_tools: [], required_knowledge: [],
          max_risk: "read_only", dependencies: [], evaluation_criteria: [], output_schema: {}, instructions: "# Architecture Designer",
          layers_from: "inferred from metadata.scope=design",
          frontmatter: { name: "architecture-designer", description: "Use when…", license: "MIT", metadata: { author: "https://github.com/Jeffallan", version: "1.1.1", scope: "design" } },
        }}
      />,
    );
    expect(screen.getByTestId("layers-from")).toHaveTextContent("inferred from metadata.scope=design");
    const fm = screen.getByTestId("frontmatter");
    expect(fm).toHaveTextContent("license");
    expect(fm).toHaveTextContent("MIT");
    expect(fm).toHaveTextContent("https://github.com/Jeffallan");
    expect(fm).not.toHaveTextContent("Use when…");
  });
});
