import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WorkflowOperations } from "@/components/WorkflowOperations";
import { TaskDetailPage } from "@/pages/TaskDetail";
import { TasksPage } from "@/pages/Tasks";
import { RegisterProjectPage } from "@/pages/RegisterProject";
import { StartWorkflowPage } from "@/pages/StartWorkflow";
import { mockFetch, renderAt, session, wf } from "./helpers";
import type { WorkflowView } from "@/lib/types";

const operatorSession = (over: Record<string, unknown> = {}) => ({
  ...session(),
  operator: { profile: "developer", principal: "olivia", kind: "human", roles: ["operator"], projects: "all" },
  can_operate: true,
  ...over,
});

const W = "0190-wf-1111";
const plan = {
  source: W,
  source_status: "waiting_approval",
  source_state: "approve_lld",
  definition_id: "engineering.design",
  definition_version: "1.1.0",
  stage: "hld",
  stages: [
    { name: "analyse", kind: "agent_task", disposition: "carried", artifact: null, approval: null, approval_id: null, allowed: true, reason: null },
    { name: "approve_requirements", kind: "approval_gate", disposition: "carried", artifact: null, approval: "carried", approval_id: "ap1", allowed: true, reason: null },
    { name: "hld", kind: "agent_task", disposition: "restart_point", artifact: null, approval: null, approval_id: null, allowed: true, reason: null },
    { name: "render_hld", kind: "agent_task", disposition: "rerun", artifact: { id: "a", name: "design/hld", version: 1, hash: "b".repeat(64) }, approval: null, approval_id: null, allowed: true, reason: null },
    { name: "approve_hld", kind: "approval_gate", disposition: "rerun", artifact: null, approval: "repeat", approval_id: "ap2", allowed: true, reason: null },
  ],
  allowed_stages: ["analyse", "hld", "approve_hld"],
  carried_states: ["analyse", "approve_requirements"],
  carried_approvals: ["approve_requirements"],
  repeat_approvals: ["approve_hld"],
  input_changed: false,
  source_will_be_cancelled: true,
  workspace_effect: "approved documents stay; re-approved versions are written as new versions",
  fingerprint: "fp-123",
};

describe("Workflow operations", () => {
  it("are hidden with a hint when the portal has no operator identity", async () => {
    mockFetch({ "/portal/session": session() });
    renderAt(<WorkflowOperations w={wf() as unknown as WorkflowView} ops={{ resume: true, cancel: true, restart: true }} />);
    expect(await screen.findByTestId("ops-hidden")).toHaveTextContent("--operator");
    expect(screen.queryByRole("button", { name: /resume/i })).toBeNull();
  });

  it("restart shows the engine's preview and confirms with its fingerprint and a reason", async () => {
    const { calls } = mockFetch({
      "/portal/session": operatorSession(),
      [`/api/v1/workflows/${W}/restart/preview`]: plan,
      [`/api/v1/workflows/${W}/restart`]: [202, { id: "0190-wf-2222", source: W, stage: "hld", source_cancelled: true }],
    });
    renderAt(<WorkflowOperations w={wf({ state: "approve_lld" }) as unknown as WorkflowView} ops={{ resume: true, cancel: true, restart: true }} />);
    fireEvent.click(await screen.findByRole("button", { name: /restart from stage/i }));
    const preview = await screen.findByTestId("restart-preview");
    expect(preview).toHaveTextContent("Restart point");
    expect(preview).toHaveTextContent("Re-executed · new version");
    expect(preview).toHaveTextContent("Approval must be repeated");
    expect(preview).toHaveTextContent("Approval stays valid");
    expect(preview).toHaveTextContent("superseded");
    const confirm = screen.getByRole("button", { name: /confirm restart from hld/i });
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByLabelText(/reason/i), { target: { value: "architecture changed" } });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);
    await waitFor(() => expect(calls.some((c) => c.url === `/api/v1/workflows/${W}/restart` && c.init?.method === "POST")).toBe(true));
    const body = JSON.parse(String(calls.find((c) => c.url === `/api/v1/workflows/${W}/restart`)?.init?.body));
    expect(body).toEqual({ stage: "hld", fingerprint: "fp-123", reason: "architecture changed" });
    expect(await screen.findByText("other page")).toBeInTheDocument();
  });
});

const task = (over: Record<string, unknown> = {}) => ({
  task: { id: "t-1", workflow_instance_id: W, state: "hld", kind: "agent", name: "hld", status: "awaiting_external", attempt: 1, agent: "solution-architect@1.0.0", error: null, created_at: "2026-11-14T10:00:00Z", updated_at: "2026-11-14T10:01:00Z" },
  input: { requirements: { $output: "refine" } },
  output: null,
  stage: { name: "hld" },
  depends_on: ["refine"],
  requires_approval_of: ["approve_requirements"],
  artifact: null,
  quality: null,
  attempts: [],
  workflow: { id: W, definition: "engineering.design", status: "waiting_external", state: "hld" },
  project: "p1",
  agent_definition: { reference: "solution-architect@1.0.0", status: "approved", skills: ["architecture-diagrams@1.0.0"], purpose: "Design", capabilities: ["design"], allowed_tools: ["knowledge.read"], autonomy: "supervised" },
  handoff: { task_id: "t-1", workflow_instance: W, workflow_status: "waiting_external", state: "hld", task_kind: "hld", agent: "solution-architect@1.0.0", status: "awaiting", submissions: 0, feedback: [], created_at: "x", updated_at: "x", dispatch_state: "blocked", dispatch_attempts: 1, last_failure: "auth_required" },
  executions: [{ id: "e1", task_id: "t-1", workflow_instance: W, stage: "hld", agent: "a", worker: "w1", provider: "claude-code", client_version: null, execution_mode: "external_ai", auth_status: null, attempt: 1, outcome: "blocked", failure_class: "auth_required", detail: "sign in again", task_status_after: null, num_turns: null, duration_ms: null, notional_cost_usd: null, started_at: "2026-11-14T10:00:30Z", ended_at: null }],
  operations: { retry_handoff: true, reexecute_stage: "hld" },
  ...over,
});

describe("Task page", () => {
  it("shows agent, skills, dependencies, hand-off and attempts, and retries a stopped hand-off", async () => {
    const { calls } = mockFetch({ "/portal/session": operatorSession(), "/api/v1/tasks/t-1/retry": { task: "t-1" }, "/api/v1/tasks/t-1": task() });
    renderAt(<TaskDetailPage />, "/tasks/t-1", "/tasks/:id");
    expect(await screen.findByText(/architecture-diagrams@1.0.0/)).toBeInTheDocument();
    expect(screen.getByText(/Consumes outputs of: Refine/)).toBeInTheDocument();
    expect(screen.getByText("External AI hand-off").closest(".rounded-lg")).toHaveTextContent("blocked");
    expect(screen.getByRole("table", { name: "Execution attempts" })).toHaveTextContent("auth_required");
    fireEvent.click(screen.getByRole("button", { name: /retry ai hand-off/i }));
    await waitFor(() => expect(calls.some((c) => c.url === "/api/v1/tasks/t-1/retry" && c.init?.method === "POST")).toBe(true));
  });

  it("offers no retry button without an operator", async () => {
    mockFetch({ "/portal/session": session(), "/api/v1/tasks/t-1": task() });
    renderAt(<TaskDetailPage />, "/tasks/t-1", "/tasks/:id");
    expect(await screen.findByText(/start the portal with/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry ai hand-off/i })).toBeNull();
  });
});

describe("Tasks page", () => {
  it("filters by stage and status through the backend", async () => {
    const { calls } = mockFetch({
      "/portal/session": session(),
      "/api/v1/projects": { items: [] },
      "/api/v1/workflow-definitions": { items: [{ id: "engineering.design", version: "1.1.0", description: "d", initial: "brd", states: [{ name: "hld", type: "agent_task" }, { name: "lld", type: "agent_task" }] }] },
      "/api/v1/task-queue": { items: [{ project_id: "p1", project_name: "orders", task: task().task }], next_cursor: null },
    });
    renderAt(<TasksPage />, "/tasks", "/tasks");
    expect(await screen.findByRole("link", { name: "Hld" })).toHaveAttribute("href", "/tasks/t-1");
    fireEvent.change(await screen.findByLabelText("Stage"), { target: { value: "hld" } });
    fireEvent.change(screen.getByLabelText("Task status"), { target: { value: "failed" } });
    await waitFor(() => expect(calls.some((c) => c.url.includes("stage=hld") && c.url.includes("status=failed"))).toBe(true));
  });
});

describe("Register existing project", () => {
  it("explains how to enable registration without an admin identity", async () => {
    mockFetch({ "/portal/session": { ...session(), can_register_projects: false } });
    renderAt(<RegisterProjectPage />);
    expect(await screen.findByText(/--admin/)).toBeInTheDocument();
  });

  it("browses, inspects, lets the user amend and registers with the confirmed values", async () => {
    const { calls } = mockFetch({
      "/portal/session": { ...session(), can_register_projects: true, workspace_roots: ["/ws"] },
      "/portal/fs": { path: "", parent: null, roots: ["/ws"], entries: [{ name: "orders-api", path: "/ws/orders-api", project_hint: true }], truncated: false },
      "/portal/projects/inspect": {
        detection: {
          path: "/ws/orders-api", suggested_name: "orders-api", description: "Order API", language: "Java", framework: "Spring Boot", build_tool: "Maven",
          build_files: ["pom.xml"], source_dirs: ["src/main/java"], git: { present: true, remote: null, head: "refs/heads/main" },
          documents: { brd_candidates: ["docs/BRD.md"], readme: "README.md", architecture: [], adrs: [], standards: [], knowledge: [] },
          manifest: null, manifest_issue: null, warnings: [], recognised: true,
        },
        registration: { state: "new" },
      },
      "/portal/projects/register": [201, { project: "p-new", name: "orders", workspace: "/ws/orders-api", actions: ["project 'orders' created"], handover: [], readiness: { project: "p-new", ready: true, blocking: 0, warnings: 0, checks: [] } }],
    });
    renderAt(<RegisterProjectPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Select" }));
    expect(await screen.findByDisplayValue("Spring Boot")).toBeInTheDocument();
    expect(screen.getByDisplayValue("docs/BRD.md")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Project name"), { target: { value: "orders" } });
    fireEvent.click(screen.getByRole("button", { name: /register project/i }));
    expect(await screen.findByText("Project registered")).toBeInTheDocument();
    const body = JSON.parse(String(calls.find((c) => c.url === "/portal/projects/register")?.init?.body));
    expect(body).toMatchObject({ path: "/ws/orders-api", name: "orders", framework: "Spring Boot", brd: "docs/BRD.md", knowledge: ["README.md"], approvers: ["approver"], update: false });
    expect(screen.getByRole("link", { name: "Open project" })).toHaveAttribute("href", "/projects/p-new");
  });
});

describe("Start workflow", () => {
  const overview = { project: { id: "p1", name: "orders", domain: null }, manifest: { description: "", stack: { language: "Java", framework: "", build_tool: "" }, workflows: ["engineering.design"], brd: null, knowledge: [], execution_mode: null, roles: { developer: "d", steward: "s", approvers: ["a"] } } };
  const projectPlan = (ready: boolean) => ({
    project: "p1", workspace: "/ws/orders", configured_workflows: ["engineering.design"],
    plan: {
      activity: "engineering.design", description: "Requirements, HLD and LLD", workflow: "engineering.design", input: { brd: "orders-brd/brd@v1" },
      stages: [{ state: "analyse", kind: "agent", agent: "business-analyst@1.0.0", skills: ["requirements@1.0.0"], detail: "task requirements" }],
      approvals: ["approve_requirements – human approver"],
      readiness: { project: "p1", ready, blocking: ready ? 0 : 1, warnings: 0, checks: ready ? [] : [{ area: "skills", status: "blocking", message: "skill not approved", fix: "approve it" }] },
    },
  });

  it("uses backend definitions and the plan, and starts with the planned input", async () => {
    const { calls } = mockFetch({
      "/portal/session": operatorSession(),
      "/api/v1/projects/p1/overview": overview,
      "/api/v1/projects/p1/plan": projectPlan(true),
      "/api/v1/workflow-definitions": { items: [{ id: "engineering.design", version: "1.1.0", description: "Design", initial: "brd", states: [] }] },
      "/api/v1/workflows": [202, { id: "0190-wf-9", status: "running" }],
      "/portal/projects/p1/worker": { worker: null },
    });
    renderAt(<StartWorkflowPage />, "/projects/p1/start", "/projects/:id/start");
    fireEvent.change(await screen.findByLabelText("Workflow definition"), { target: { value: "engineering.design" } });
    expect(await screen.findByRole("table", { name: "Execution plan" })).toHaveTextContent("business-analyst@1.0.0");
    fireEvent.click(screen.getByRole("button", { name: /start engineering.design/i }));
    await waitFor(() => expect(calls.some((c) => c.url === "/api/v1/workflows" && c.init?.method === "POST")).toBe(true));
    const body = JSON.parse(String(calls.find((c) => c.url === "/api/v1/workflows" && c.init?.method === "POST")?.init?.body));
    expect(body).toEqual({ definition: "engineering.design", project_id: "p1", input: { brd: "orders-brd/brd@v1" } });
  });

  it("does not start while readiness has blocking items", async () => {
    mockFetch({
      "/portal/session": operatorSession(),
      "/api/v1/projects/p1/overview": overview,
      "/api/v1/projects/p1/plan": projectPlan(false),
      "/api/v1/workflow-definitions": { items: [{ id: "engineering.design", version: "1.1.0", description: "Design", initial: "brd", states: [] }] },
    });
    renderAt(<StartWorkflowPage />, "/projects/p1/start", "/projects/:id/start");
    fireEvent.change(await screen.findByLabelText("Workflow definition"), { target: { value: "engineering.design" } });
    expect(await screen.findByText(/skill not approved/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /start engineering.design/i })).toBeDisabled();
  });
});
