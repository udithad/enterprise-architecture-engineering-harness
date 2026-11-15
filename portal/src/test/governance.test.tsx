import { render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, it } from "vitest";
import { CompletionSeal, GovernanceCard, gateState } from "@/components/GovernanceCard";
import { GenerationBadge, ReplayNotice } from "@/components/Generation";
import type { Governance } from "@/lib/types";

const H = "c".repeat(64);
const gate = (name: string, over: Partial<Governance["gates"][number]> = {}) => ({
  gate: name, artifact_from: `render_${name}`, approval_id: null, approver_id: null, approver_role: null, decided_at: null, approved_hash: null, current_hash: H, issues: [], ...over,
});
const approved = (name: string) => gate(name, { approval_id: `ap-${name}`, approver_id: "approver", approver_role: "approver", decided_at: "2026-11-14T17:25:20Z", approved_hash: H });

describe("governance verification", () => {
  it("classifies gates", () => {
    expect(gateState(approved("approve_lld"), null)).toBe("verified");
    expect(gateState(gate("approve_lld", { approval_id: "x", issues: ["bad"] }), null)).toBe("issues");
    expect(gateState(gate("approve_lld"), "approve_lld")).toBe("awaiting");
    expect(gateState(gate("approve_lld"), null)).toBe("not_reached");
  });

  it("marks a completed run verified only when every gate and the engine check pass", () => {
    const ok: Governance = { gates: [approved("approve_requirements"), approved("approve_hld"), approved("approve_lld")], gates_total: 3, gates_granted: 3, gates_with_issues: 0, completion_check: { outcome: "succeeded", occurred_at: "2026-11-14T17:25:21Z", details: {} }, completion_verified: true };
    const { rerender } = render(<CompletionSeal gov={ok} status="succeeded" />);
    expect(screen.getByTestId("completion-seal")).toHaveTextContent("Verified: 3/3 gates approved");
    rerender(<CompletionSeal gov={{ ...ok, completion_verified: false }} status="succeeded" />);
    expect(screen.getByTestId("completion-seal")).toHaveTextContent("Completion not verified");
    rerender(<CompletionSeal gov={ok} status="waiting_approval" />);
    expect(screen.queryByTestId("completion-seal")).toBeNull();
  });

  it("lists each gate with approver, hash binding and issues", () => {
    const gov: Governance = {
      gates: [approved("approve_requirements"), gate("approve_hld", { approval_id: "ap2", approved_hash: "d".repeat(64), issues: ["approved hash differs from the current 'render_hld' artifact"] }), gate("approve_lld")],
      gates_total: 3, gates_granted: 2, gates_with_issues: 1, completion_check: { outcome: "denied", occurred_at: "2026-11-14T17:25:21Z", details: { violations: ["gate 'approve_lld' has no granted approval"] } }, completion_verified: false,
    };
    render(<MemoryRouter><GovernanceCard gov={gov} status="failed" pendingGate={null} /></MemoryRouter>);
    const t = screen.getByRole("table", { name: "Gate verification" });
    expect(within(t).getByText("Verified approval")).toBeInTheDocument();
    expect(within(t).getByText("Approval invalid")).toBeInTheDocument();
    expect(within(t).getByText(/approved hash differs/)).toBeInTheDocument();
    expect(within(t).getByText("Not reached")).toBeInTheDocument();
    expect(screen.getByText(/Engine completion check:/)).toHaveTextContent("denied");
    expect(screen.getByText(/Engine completion check:/)).toHaveTextContent("has no granted approval");
  });
});

describe("generation provenance", () => {
  const g = (source: "external_ai" | "deterministic_replay" | "direct_provider" | "platform", models: string[]) => ({ source, models, agent_definition: "solution-architect@1.0.0", produced_by: "agent:x", created_at: "", artifact: "design/hld-model.json", version: 1 });
  it("labels replay as not AI and names the AI client", () => {
    const { rerender } = render(<GenerationBadge g={g("deterministic_replay", ["replay/replay@cassette"])} />);
    expect(screen.getByText(/Recorded fixture – not AI/)).toBeInTheDocument();
    rerender(<GenerationBadge g={g("external_ai", ["external:claude-code"])} />);
    expect(screen.getByText(/AI client: claude-code/)).toBeInTheDocument();
  });
  it("shows the deterministic-mode notice only for replayed runs", () => {
    const { rerender, container } = render(<ReplayNotice provenance={{ hld: g("external_ai", ["external:claude-code"]) }} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<ReplayNotice provenance={{ hld: g("deterministic_replay", ["replay/replay@c"]) }} />);
    expect(screen.getByRole("note")).toHaveTextContent("not AI-generated");
  });
});
