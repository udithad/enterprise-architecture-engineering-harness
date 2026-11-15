import { describe, expect, it } from "vitest";
import { approvalStatus, isActive, taskStatus, workflowStatus } from "@/lib/status";
import { formatDuration, humanize } from "@/lib/format";

describe("status mapping", () => {
  it("distinguishes waiting for AI from waiting for approval", () => {
    const ai = workflowStatus("waiting_external");
    const ap = workflowStatus("waiting_approval");
    expect(ai.label).toBe("Waiting for AI");
    expect(ai.waitingOn).toBe("External AI");
    expect(ap.label).toBe("Waiting for approval");
    expect(ap.waitingOn).toBe("Human approval");
    expect(ai.tone).not.toBe(ap.tone);
    expect(taskStatus("awaiting_external").label).toBe("Waiting for AI");
  });
  it("maps every backend workflow status", () => {
    expect(workflowStatus("running").label).toBe("Running");
    expect(workflowStatus("succeeded").label).toBe("Completed");
    expect(workflowStatus("failed").label).toBe("Failed");
    expect(workflowStatus("cancelled").label).toBe("Cancelled");
    expect(workflowStatus("escalated").tone).toBe("failed");
    expect(isActive("waiting_external")).toBe(true);
    expect(isActive("succeeded")).toBe(false);
    expect(approvalStatus("changes_requested").label).toBe("Changes requested");
  });
  it("formats names and durations", () => {
    expect(humanize("approve_requirements")).toBe("Approve requirements");
    expect(formatDuration("2026-01-01T00:00:00Z", "2026-01-01T01:02:03Z")).toBe("1h 2m");
    expect(formatDuration("2026-01-01T00:00:00Z", "2026-01-01T00:00:09Z")).toBe("9s");
  });
});
