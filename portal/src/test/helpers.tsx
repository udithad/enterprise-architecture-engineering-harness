import type { ReactNode } from "react";
import { render } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryRouter, RouterProvider } from "react-router";
import { vi } from "vitest";

/** Route table for fetch mocks: path (without `/api`) → body, or [status, body]. */
export type Routes = Record<string, unknown | [number, unknown]>;

export function mockFetch(routes: Routes) {
  const calls: { url: string; init?: RequestInit }[] = [];
  const fn = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : input.toString();
    calls.push({ url, init });
    const key = Object.keys(routes)
      .sort((a, b) => b.length - a.length)
      .find((k) => url === k || url.startsWith(k));
    if (!key) return new Response(JSON.stringify({ error: { code: "not_found", message: `no mock for ${url}` } }), { status: 404 });
    const v = routes[key];
    const [status, body] = Array.isArray(v) && typeof v[0] === "number" ? (v as [number, unknown]) : [200, v];
    return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  });
  vi.stubGlobal("fetch", fn);
  return { fn, calls };
}

export function renderAt(ui: ReactNode, path = "/", routePath = "/") {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false, refetchInterval: false } } });
  const router = createMemoryRouter([{ path: routePath, element: ui }, { path: "*", element: <div>other page</div> }], { initialEntries: [path] });
  return render(
    <QueryClientProvider client={qc}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

export const session = (approver = true) => ({
  installation: { data_dir: "/work/.eaeh-data", config_dir: "/work/config" },
  viewer: { profile: "dev", principal: "olivia", kind: "human", roles: ["operator"], projects: "all" },
  approver: approver ? { profile: "approver", principal: "arthur", kind: "human", roles: ["approver"], projects: "all" } : null,
  can_decide: approver,
  execution_mode: "deterministic",
  api_version: "v1",
  local_only: true,
  authentication: "none (trusted local development only)",
});

const wf = (over: Record<string, unknown> = {}) => ({
  id: "0190-wf-1111",
  definition_id: "engineering.design",
  definition_version: "1.0.0",
  project_id: "p1",
  state: "approve_requirements",
  status: "waiting_approval",
  version: 7,
  started_by: "olivia",
  correlation_id: "c",
  cancel_requested: false,
  created_at: "2026-11-14T10:00:00Z",
  updated_at: "2026-11-14T10:05:00Z",
  artifacts: {},
  approved_gates: {},
  current_approval: {},
  revisions: {},
  visits: {},
  quality: {},
  feedback: {},
  failure: null,
  awaiting_external: null,
  ...over,
});
export { wf };

export const approval = (over: Record<string, unknown> = {}) => ({
  id: "0190-ap-2222",
  project_id: "p1",
  project_name: "orders",
  subject_type: "workflow_gate",
  subject_ref: "x",
  document_type: "Requirements",
  gate: "approve_requirements",
  status: "pending",
  required_role: "approver",
  authors: ["agent:business-analyst"],
  requested_by: "workflow-engine",
  created_at: "2026-11-14T10:05:00Z",
  decided_at: null,
  decision: null,
  artifact_id: "art-1",
  artifact_hash: "a".repeat(64),
  artifact: { name: "design/requirements", version: 1, hash: "a".repeat(64), media_type: "application/json", size_bytes: 2048, created_at: "2026-11-14T10:05:00Z" },
  workflow: { id: "0190-wf-1111", definition_id: "engineering.design", definition_version: "1.0.0", state: "approve_requirements", status: "waiting_approval" },
  ...over,
});
