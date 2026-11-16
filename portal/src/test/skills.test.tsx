import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SkillsPage } from "@/pages/Skills";
import { SkillReview } from "@/components/SkillReview";
import { skillStatus } from "@/lib/status";
import { mockFetch, renderAt, session } from "./helpers";
import { render } from "@testing-library/react";

const md = {
  id: "architecture-diagrams", version: "1.0.0", reference: "architecture-diagrams@1.0.0", description: "Diagram rules",
  task_kinds: ["hld-design", "lld-design"], capabilities: ["hld-design"], layers: ["engineering.design/hld", "engineering.design/lld"],
  required_tools: [], required_knowledge: [], max_risk: "read_only", dependencies: [], evaluation_criteria: ["every component appears"],
  output_schema: { type: "object" }, instructions: "# Architecture diagrams\n\nUse the **component id**.", format: "markdown",
  file: "/eaeh/config/skills/architecture-diagrams.md", file_digest: "d".repeat(64), registration: null, file_changed: false,
};
const catalog = (items: unknown[]) => ({ skills_dir: "/eaeh/config/skills", file_error: null, items, layers: { "hld-design": ["engineering.design/hld"], "lld-design": ["engineering.design/lld"] } });

describe("Skills", () => {
  it("lists skill files per layer and registers them as the steward", async () => {
    const s = session();
    const routes: Record<string, unknown> = {
      "/portal/session": { ...s, steward: { profile: "steward", principal: "sam", kind: "human", roles: ["knowledge_steward"], projects: "all" }, can_register_skills: true },
      "/api/v1/projects": { items: [{ id: "p1", name: "orders" }] },
      "/api/v1/skills/catalog": catalog([md]),
      "/api/v1/skills/sync": { results: [{ reference: "architecture-diagrams@=1.0.0", action: "registered", detail: "awaiting human approval", approval_id: "ap-9" }] },
    };
    const m = mockFetch(routes);
    renderAt(<SkillsPage />);
    const row = await screen.findByTestId("skill-row-architecture-diagrams");
    expect(row).toHaveTextContent("engineering.design/hld, engineering.design/lld");
    expect(row).toHaveTextContent("markdown");
    expect(row).toHaveTextContent("Not registered");
    const button = await screen.findByRole("button", { name: /Register skill files/ });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    expect(await screen.findByTestId("sync-results")).toHaveTextContent("registered");
    const post = m.calls.find((c) => c.url.endsWith("/v1/skills/sync"));
    expect(JSON.parse(String(post?.init?.body))).toEqual({ project_id: "p1" });
  });

  it("explains how to enable registration without a steward", async () => {
    mockFetch({ "/portal/session": session(), "/api/v1/projects": { items: [] }, "/api/v1/skills/catalog": catalog([]) });
    renderAt(<SkillsPage />);
    expect(await screen.findByText(/--steward steward/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Register skill files/ })).toBeNull();
  });

  it("shows the full skill under review and derives statuses", () => {
    render(<SkillReview skill={{ ...md, digest: "e".repeat(64), status: "pending_approval", registered_by: "sam" }} />);
    expect(screen.getByTestId("skill-layers")).toHaveTextContent("engineering.design/lld");
    expect(screen.getByText("Architecture diagrams")).toBeInTheDocument();
    expect(skillStatus({ registration: { status: "pending_approval" }, file_changed: false }).label).toBe("Waiting for approval");
    expect(skillStatus({ registration: { status: "approved" }, file_changed: false }).label).toBe("Approved · applied");
    expect(skillStatus({ registration: { status: "approved" }, file_changed: true }).tone).toBe("failed");
    expect(skillStatus({ registration: null, file_changed: false }).label).toBe("Not registered");
  });
});
