import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WorkspaceDocuments, WorkspaceState } from "@/components/WorkspaceDocuments";
import { workflowStatus } from "@/lib/status";
import type { GateDocument, MaterializationStatus } from "@/lib/types";

const H = "d".repeat(64);
const status = (over: Partial<MaterializationStatus>): MaterializationStatus => ({
  state: "verified", explanation: "All 3 file(s) re-hashed and equal to the approved version.", root: "/repo", target_dir: "docs/architecture/hld",
  files: [], materialized_bundle: "b1", last_success_at: "2026-11-15T10:00:00Z", last_attempt: null, checked_at: "2026-11-15T10:01:00Z", ...over,
});
const doc = (over: Partial<GateDocument>): GateDocument => ({
  gate: "approve_hld", artifact_from: "render_hld", artifact: { id: "b1", name: "design/hld", version: 2, hash: H },
  approval: { gate: "approve_hld", artifact_from: "render_hld", approval_id: "ap1", approver_id: "approver", approver_role: "approver", decided_at: "2026-11-15T09:59:00Z", approved_hash: H, current_hash: H, issues: [] },
  approved: true, target_dir: "docs/architecture/hld", materialization: status({}), blocked: null, ...over,
});

describe("workspace documents", () => {
  it("shows 'verified' only for the backend's verified state", () => {
    const { rerender } = render(<WorkspaceState doc={doc({})} />);
    expect(screen.getByTestId("workspace-state-approve_hld")).toHaveTextContent("In workspace · verified");
    rerender(<WorkspaceState doc={doc({ materialization: status({ state: "modified", explanation: "1 file(s) differ", files: [{ path: "docs/architecture/hld/hld.md", expected_hash: H, actual_hash: "e".repeat(64), state: "modified" }] }) })} />);
    const el = screen.getByTestId("workspace-state-approve_hld");
    expect(el).toHaveTextContent("Modified locally");
    expect(el).not.toHaveTextContent("verified");
    expect(el).toHaveTextContent("modified: docs/architecture/hld/hld.md");
  });

  it("explains a missing materialization and a blocked run", () => {
    render(
      <WorkspaceState
        doc={doc({ materialization: status({ state: "conflict", explanation: "not written – existing files would be overwritten: docs/architecture/hld/hld.md", files: [] }), blocked: "conflict" })}
      />,
    );
    const el = screen.getByTestId("workspace-state-approve_hld");
    expect(el).toHaveTextContent("Not written – conflict");
    expect(el).toHaveTextContent("existing files would be overwritten");
    expect(el).toHaveTextContent("Workflow held at this gate");
  });

  it("lists documents with path and asks for sync when an approved one is not verified", () => {
    render(
      <WorkspaceDocuments
        docs={[doc({}), doc({ gate: "approve_lld", target_dir: "docs/architecture/lld", materialization: status({ state: "missing", explanation: "2 file(s) are missing" }) })]}
        workspace={{ root: "/repo", registered_by: "dev", registered_at: "2026-11-15T09:00:00Z" }}
      />,
    );
    expect(screen.getByText("docs/architecture/hld/")).toBeInTheDocument();
    expect(screen.getByText("docs/architecture/lld/")).toBeInTheDocument();
    expect(screen.getByText("Missing from workspace")).toBeInTheDocument();
    expect(screen.getByText(/Run/)).toHaveTextContent("eaeh artifact sync");
  });

  it("flags an unregistered workspace and a blocked status", () => {
    render(<WorkspaceDocuments docs={[doc({ materialization: status({ state: "no_workspace", explanation: "No application workspace is registered" }) })]} workspace={null} />);
    expect(screen.getByText(/not registered/)).toBeInTheDocument();
    expect(workflowStatus("running", { gate: "approve_hld", reason: "x", since: "" }).label).toBe("Blocked");
    expect(workflowStatus("running", null).label).toBe("Running");
  });
});
