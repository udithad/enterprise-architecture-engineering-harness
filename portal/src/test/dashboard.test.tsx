import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { DashboardPage } from "@/pages/Dashboard";
import { approval, mockFetch, renderAt, wf } from "./helpers";

const dashboard = (over: Record<string, unknown> = {}) => ({
  generated_at: new Date().toISOString(),
  execution_mode: "external_ai",
  summary: { projects: 2, in_progress: 2, executing_tasks: 1, waiting_external: 1, waiting_approval: 1, pending_approvals: 1, completed: 3, failed_or_blocked: 1 },
  projects: [{ id: "p1", name: "orders", restricted: false }],
  active: [
    { project_id: "p1", project_name: "orders", workflow: wf(), tasks: [] },
    {
      project_id: "p1",
      project_name: "orders",
      workflow: wf({ id: "0190-wf-3333", state: "hld", status: "waiting_external", awaiting_external: "hld" }),
      tasks: [{ id: "t1", workflow_instance_id: "0190-wf-3333", state: "hld", kind: "agent", name: "hld.design", status: "awaiting_external", attempt: 1, agent: "solution-architect@1.0.0", error: null, created_at: "2026-11-14T10:00:00Z", updated_at: "2026-11-14T10:01:00Z" }],
    },
  ],
  pending_approvals: [approval()],
  activity: [{ kind: "approval_requested", occurred_at: "2026-11-14T10:05:00Z", workflow_instance_id: "0190-wf-1111", definition_id: "engineering.design", state: "approve_requirements", to_state: null, actor_id: "workflow-engine", project_name: "orders" }],
  ...over,
});

describe("Dashboard", () => {
  it("shows real summary figures, in-progress work and pending approvals", async () => {
    mockFetch({ "/api/v1/dashboard": dashboard(), "/api/v1/workflow-definitions": { items: [] } });
    renderAt(<DashboardPage />);
    expect(await screen.findByTestId("stat-Workflows in progress")).toHaveTextContent("2");
    expect(screen.getByTestId("stat-Pending approvals")).toHaveTextContent("1");
    expect(screen.getByTestId("stat-Failed or blocked")).toHaveTextContent("1");
    const table = screen.getByRole("table", { name: "In-progress engineering work" });
    // Waiting-for-AI and waiting-for-approval are visibly different.
    expect(within(table).getByText("Waiting for approval")).toBeInTheDocument();
    expect(within(table).getAllByText("Waiting for AI").length).toBeGreaterThan(0);
    expect(within(table).getByText("External AI")).toBeInTheDocument();
    expect(within(table).getByText("Human approval")).toBeInTheDocument();
    expect(within(table).getByText("solution-architect@1.0.0")).toBeInTheDocument();
    const pending = screen.getByRole("table", { name: "Pending approvals" });
    expect(within(pending).getByText("Requirements")).toBeInTheDocument();
    expect(within(pending).getByRole("link", { name: /Review/ })).toHaveAttribute("href", "/approvals/0190-ap-2222");
    expect(screen.getByText("Approval requested")).toBeInTheDocument();
  });

  it("renders empty states when nothing is running", async () => {
    mockFetch({
      "/api/v1/dashboard": dashboard({ summary: { projects: 1, in_progress: 0, executing_tasks: 0, waiting_external: 0, waiting_approval: 0, pending_approvals: 0, completed: 0, failed_or_blocked: 0 }, active: [], pending_approvals: [], activity: [] }),
      "/api/v1/workflow-definitions": { items: [] },
    });
    renderAt(<DashboardPage />);
    expect(await screen.findByText("No engineering work in progress")).toBeInTheDocument();
    expect(screen.getByText("No approvals waiting")).toBeInTheDocument();
    expect(screen.getByText("No workflow activity yet.")).toBeInTheDocument();
  });

  it("explains a failed request with the server's message and a hint", async () => {
    mockFetch({ "/api/v1/dashboard": [401, { error: { code: "unauthenticated", message: "invalid or expired token", request_id: "r-1" } }] });
    renderAt(<DashboardPage />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("invalid or expired token");
    expect(alert).toHaveTextContent("request r-1");
    expect(alert).toHaveTextContent("eaeh auth renew");
  });
});
