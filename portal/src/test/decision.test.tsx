import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { DecisionPanel, decisionSchema } from "@/components/DecisionPanel";
import type { ApprovalDocument } from "@/lib/types";
import { approval, mockFetch, renderAt, session, wf } from "./helpers";

const H = "a".repeat(64);
const doc = (over: Partial<ApprovalDocument["approval"]> = {}, hash = H): ApprovalDocument => ({
  approval: { ...(approval() as unknown as ApprovalDocument["approval"]), ...over },
  document: {
    artifact: { id: "art-1", name: "design/requirements", version: 1, content_hash: hash, size_bytes: 10, media_type: "application/json", created_at: "" },
    bundle: true,
    files: [],
  },
  workflow: { instance: wf() as never, previous_requests: [], trace_links: [], artifact_provenance: {}, execution_mode: "deterministic" },
  decision_endpoint: "/v1/approvals/0190-ap-2222/decision",
});

describe("decision form rules", () => {
  it("requires comments for request changes and rejection only", () => {
    expect(decisionSchema.safeParse({ decision: "approve", comments: "" }).success).toBe(true);
    expect(decisionSchema.safeParse({ decision: "reject", comments: " " }).success).toBe(false);
    expect(decisionSchema.safeParse({ decision: "request_changes", comments: "" }).success).toBe(false);
    expect(decisionSchema.safeParse({ decision: "request_changes", comments: "Add NFRs" }).success).toBe(true);
  });
});

describe("DecisionPanel", () => {
  it("is read-only when the portal has no approver", () => {
    mockFetch({});
    renderAt(<DecisionPanel doc={doc()} session={session(false) as never} />);
    expect(screen.getByText("Read-only portal")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Approve/ })).not.toBeInTheDocument();
  });

  it("blocks rejection without comments, then confirms before submitting with the reviewed hash", async () => {
    const m = mockFetch({
      "/api/v1/approvals/0190-ap-2222/document": doc(),
      "/api/v1/approvals/0190-ap-2222/decision": { ...approval({ status: "rejected" }) },
    });
    const user = userEvent.setup();
    renderAt(<DecisionPanel doc={doc()} session={session() as never} />);
    await user.click(screen.getByRole("button", { name: /Reject/ }));
    expect(await screen.findByText("Explain why the document is rejected.")).toBeInTheDocument();
    expect(m.calls.some((c) => c.url.endsWith("/decision"))).toBe(false);

    await user.type(screen.getByLabelText("Reviewer comments"), "Out of scope items missing");
    await user.click(screen.getByRole("button", { name: /Reject/ }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Reject Requirements v1?");
    expect(dialog).toHaveTextContent(H);
    expect(m.calls.some((c) => c.url.endsWith("/decision"))).toBe(false);
    await user.click(screen.getByRole("button", { name: "Confirm reject" }));
    await waitFor(() => expect(m.calls.some((c) => c.url.endsWith("/decision"))).toBe(true));
    const call = m.calls.find((c) => c.url.endsWith("/decision"))!;
    expect(call.init?.method).toBe("POST");
    expect(JSON.parse(String(call.init?.body))).toEqual({ decision: "reject", comments: "Out of scope items missing", artifact_hash: H });
  });

  it("refuses to submit when the document changed since it was opened", async () => {
    const m = mockFetch({ "/api/v1/approvals/0190-ap-2222/document": doc({}, "b".repeat(64)) });
    const user = userEvent.setup();
    renderAt(<DecisionPanel doc={doc()} session={session() as never} />);
    await user.click(screen.getByRole("button", { name: /Approve/ }));
    await user.click(await screen.findByRole("button", { name: "Confirm approve" }));
    expect(await screen.findByText(/The document changed since you opened it/)).toBeInTheDocument();
    expect(m.calls.some((c) => c.url.endsWith("/decision"))).toBe(false);
  });

  it("shows the backend's refusal (e.g. separation of duties)", async () => {
    mockFetch({
      "/api/v1/approvals/0190-ap-2222/document": doc(),
      "/api/v1/approvals/0190-ap-2222/decision": [403, { error: { code: "forbidden", message: "authors cannot approve their own work" } }],
    });
    const user = userEvent.setup();
    renderAt(<DecisionPanel doc={doc()} session={session() as never} />);
    await user.click(screen.getByRole("button", { name: /Approve/ }));
    await user.click(await screen.findByRole("button", { name: "Confirm approve" }));
    expect(await screen.findByText("authors cannot approve their own work")).toBeInTheDocument();
  });

  it("disables decisions the approval service would refuse", () => {
    mockFetch({});
    renderAt(<DecisionPanel doc={doc({ authors: ["arthur"] })} session={session() as never} />);
    expect(screen.getByText(/cannot approve it \(separation of duties\)/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Approve/ })).toBeDisabled();
  });
});
