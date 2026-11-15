import { screen, within, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProjectsPage } from "@/pages/Projects";
import { projectStatus } from "@/lib/status";
import { mockFetch, renderAt, session } from "./helpers";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryRouter, RouterProvider } from "react-router";
import { render } from "@testing-library/react";

const B = "01a1db9e-d2cb-7022-8e64-02c363f9d141";
const project = (over: Record<string, unknown> = {}) => ({
  id: B,
  name: "simple-springboot-api-b",
  restricted: false,
  workflow_counts: { waiting_approval: 2 },
  active_tasks: 0,
  pending_approvals: 2,
  status: "waiting_approval",
  current_workflow: { id: "01a1e022-b374-7690-af21-5fcc94f3ae79", definition_id: "engineering.design", status: "waiting_approval", state: "approve_requirements", blocked: null, updated_at: "2026-11-15T16:58:00Z", created_at: "2026-11-15T16:55:32Z" },
  last_activity: "2026-11-15T16:58:00Z",
  description: "Simple REST API for local development",
  stack: { language: "Java 21", framework: "Spring Boot 3.x", build_tool: "Maven" },
  workspace: { root: "/repo/simple-springboot-api-b", registered_by: "dev", registered_at: "2026-11-15T16:55:00Z" },
  registered_workflows: ["requirements.analysis", "engineering.design"],
  manifest_issue: null,
  ...over,
});
const dashboard = (projects: unknown[]) => ({
  generated_at: new Date().toISOString(),
  execution_mode: "external_ai",
  summary: { projects: projects.length, in_progress: 0, executing_tasks: 0, waiting_external: 0, waiting_approval: 0, pending_approvals: 0, completed: 0, failed_or_blocked: 0 },
  projects,
  active: [],
  pending_approvals: [],
  activity: [],
});

describe("Projects", () => {
  it("lists every registered project with stack, workspace, status, current workflow and approvals", async () => {
    mockFetch({
      "/portal/session": session(),
      "/api/v1/dashboard": dashboard([
        project(),
        project({ id: "p-new", name: "fresh-project", status: "ready", current_workflow: null, pending_approvals: 0, workflow_counts: {}, last_activity: null, workspace: null, manifest_issue: "no workspace registered yet" }),
      ]),
    });
    renderAt(<ProjectsPage />);
    const row = await screen.findByTestId("project-row-simple-springboot-api-b");
    expect(row).toHaveTextContent("Simple REST API for local development");
    expect(row).toHaveTextContent(B);
    expect(row).toHaveTextContent("Java 21 · Spring Boot 3.x · Maven");
    expect(row).toHaveTextContent("/repo/simple-springboot-api-b");
    expect(row).toHaveTextContent("Waiting for approval");
    expect(row).toHaveTextContent("engineering.design · Approve requirements");
    expect(within(row).getByRole("link")).toHaveAttribute("href", "/workflows/01a1e022-b374-7690-af21-5fcc94f3ae79");
    expect(row).toHaveTextContent("2");
    // A project without any workflow is listed too, as Ready.
    const fresh = screen.getByTestId("project-row-fresh-project");
    expect(fresh).toHaveTextContent("Ready");
    expect(fresh).toHaveTextContent("none yet");
    expect(fresh).toHaveTextContent("not registered");
    // The installation is visible, with help for missing projects.
    expect(await screen.findByTestId("installation-panel")).toHaveTextContent("/work/.eaeh-data");
    expect(screen.getByText("Not seeing a project?")).toBeInTheDocument();
  });

  it("labels a DEMO installation prominently", async () => {
    const s = session();
    mockFetch({
      "/portal/session": { ...s, installation: { ...s.installation, label: "DEMO – /x/target/demo-portal" } },
      "/api/v1/dashboard": dashboard([]),
    });
    renderAt(<ProjectsPage />);
    expect(await screen.findByTestId("installation-label")).toHaveTextContent("DEMO");
    expect(screen.getByText("No projects visible in this installation")).toBeInTheDocument();
  });

  it("shows a project registered later after the next refresh (no stale cache)", async () => {
    const routes: Record<string, unknown> = { "/portal/session": session(), "/api/v1/dashboard": dashboard([project()]) };
    mockFetch(routes);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false, refetchInterval: false } } });
    const router = createMemoryRouter([{ path: "/", element: <ProjectsPage /> }], { initialEntries: ["/"] });
    render(
      <QueryClientProvider client={qc}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );
    await screen.findByTestId("project-row-simple-springboot-api-b");
    expect(screen.queryByTestId("project-row-registered-later")).toBeNull();
    // The CLI registers a new project; the next poll (or any invalidation) shows it.
    routes["/api/v1/dashboard"] = dashboard([project(), project({ id: "p2", name: "registered-later", status: "ready", current_workflow: null })]);
    await qc.invalidateQueries();
    await waitFor(() => expect(screen.getByTestId("project-row-registered-later")).toHaveTextContent("Ready"));
  });

  it("maps every backend project status", () => {
    expect(projectStatus("ready").label).toBe("Ready");
    expect(projectStatus("in_progress").label).toBe("In progress");
    expect(projectStatus("waiting_approval").label).toBe("Waiting for approval");
    expect(projectStatus("blocked").tone).toBe("failed");
    expect(projectStatus("completed").label).toBe("Completed");
    expect(projectStatus("failed").label).toBe("Failed");
    expect(projectStatus("completed", true).label).toBe("Restricted");
  });
});
