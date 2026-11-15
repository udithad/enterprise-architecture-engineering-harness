import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { buildStages, mainPath, progress } from "@/lib/lifecycle";
import type { Transition, WorkflowDefinition } from "@/lib/types";
import { approval, wf } from "./helpers";

// The shipped definition, not a hand-written copy.
const def = JSON.parse(readFileSync(resolve(__dirname, "../../../workflows/engineering.design.json"), "utf8")) as WorkflowDefinition;

const t = (seq: number, from: string, to: string, trigger = "succeeded"): Transition => ({ seq, from_state: from, to_state: to, trigger, actor_id: "x", occurred_at: `2026-11-14T10:0${seq}:00Z` });

describe("lifecycle from the workflow definition", () => {
  it("follows success transitions from the initial state", () => {
    expect(mainPath(def).map((s) => s.name)).toEqual([
      "brd", "analyse", "refine", "render_requirements", "approve_requirements",
      "hld", "render_hld", "approve_hld", "lld", "render_lld", "approve_lld",
    ]);
  });
  it("marks completed stages, the pending gate and future stages", () => {
    const w = wf();
    const history = [t(1, "brd", "analyse"), t(2, "analyse", "refine"), t(3, "refine", "render_requirements"), t(4, "render_requirements", "approve_requirements")];
    const stages = buildStages(def, w as never, history, [], [approval() as never]);
    const by = Object.fromEntries(stages.map((s) => [s.def.name, s.status]));
    expect(by.brd).toBe("completed");
    expect(by.refine).toBe("completed");
    expect(by.approve_requirements).toBe("awaiting_approval");
    expect(by.hld).toBe("not_started");
    expect(stages.find((s) => s.def.name === "approve_requirements")?.approval?.id).toBe("0190-ap-2222");
    expect(progress(def, w as never)).toEqual({ index: 5, total: 11 });
  });
  it("shows a stage handed off to the external AI client", () => {
    const stages = buildStages(def, wf({ state: "hld", status: "waiting_external", awaiting_external: "hld" }) as never, [t(1, "brd", "analyse")], [], []);
    expect(stages.find((s) => s.def.name === "hld")?.status).toBe("awaiting_ai");
  });
  it("appends a reached failure terminal and marks the failed stage", () => {
    const w = wf({ state: "failed", status: "failed" });
    const stages = buildStages(def, w as never, [t(1, "brd", "analyse"), t(2, "analyse", "failed", "failed")], [], []);
    expect(stages.find((s) => s.def.name === "analyse")?.status).toBe("failed");
    expect(stages.at(-1)?.def.name).toBe("failed");
    expect(stages.at(-1)?.status).toBe("failed");
  });
});
