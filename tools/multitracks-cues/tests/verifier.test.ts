import { describe, expect, it, vi } from "vitest";
import { verifyCuePlan } from "../src/cues/verifier.js";
import type { CuePlanner } from "../src/cues/planner.js";
import { defaultConfiguration } from "../src/config/schema.js";
import type { CuePlan, PlanOperation } from "../src/cues/models.js";

function mockPlanner(plan: CuePlan): CuePlanner {
  return { buildCuePlan: vi.fn(async () => plan) } as unknown as CuePlanner;
}

const basePlan = (items: CuePlan["items"]): CuePlan => ({
  generatedAt: "2026-01-01T00:00:00Z",
  mode: "dry-run",
  setlist: { id: "set-1", name: "Sunday", items: [], targetDate: "2026-01-04" },
  configuration: { cueProfile: "setlist-ordinal-test", channel: 1, note: 112, busId: "aux-1" },
  items,
});

const createItem = (position: number, operations: PlanOperation[], reason = "safe") => ({
  setlistPosition: position, songOrdinal: position, velocity: position, songTitle: `Song ${position}`,
  targetType: "library" as const, targetId: `t-${position}`, bankId: "default", busId: "aux-1",
  resolvedPosition: { measure: 1, beat: 2, tick: 0 }, operations, reason,
  verificationStrategy: "read back", existingBanks: [], selectedBusEvents: [],
});

describe("verifyCuePlan", () => {
  it("reports verified for SKIP_ALREADY_PRESENT items", async () => {
    const plan = basePlan([createItem(1, ["SKIP_ALREADY_PRESENT"])]);
    const result = await verifyCuePlan(mockPlanner(plan), defaultConfiguration, "set-1", "setlist-ordinal-test");
    expect(result.summary.verified).toBe(1);
    expect(result.summary.success).toBe(true);
  });

  it("reports missing for CREATE_* items", async () => {
    const plan = basePlan([createItem(1, ["CREATE_LIBRARY_EVENT"])]);
    const result = await verifyCuePlan(mockPlanner(plan), defaultConfiguration, "set-1", "setlist-ordinal-test");
    expect(result.summary.missing).toBe(1);
    expect(result.summary.success).toBe(false);
  });

  it("reports conflicting for SKIP_CONFLICT items", async () => {
    const plan = basePlan([createItem(1, ["SKIP_CONFLICT"], "conflict detected")]);
    const result = await verifyCuePlan(mockPlanner(plan), defaultConfiguration, "set-1", "setlist-ordinal-test");
    expect(result.summary.conflicting).toBe(1);
    expect(result.summary.success).toBe(false);
  });

  it("reports skipped for SKIP_NON_SONG items", async () => {
    const plan = basePlan([createItem(1, ["SKIP_NON_SONG"], "not a song")]);
    const result = await verifyCuePlan(mockPlanner(plan), defaultConfiguration, "set-1", "setlist-ordinal-test");
    expect(result.summary.skipped).toBe(1);
    expect(result.summary.success).toBe(true);
  });

  it("reports duplicated for multiple SKIP_ALREADY_PRESENT items", async () => {
    const plan = basePlan([createItem(1, ["SKIP_ALREADY_PRESENT"], "multiple matches")]);
    const result = await verifyCuePlan(mockPlanner(plan), defaultConfiguration, "set-1", "setlist-ordinal-test");
    expect(result.summary.duplicated).toBe(1);
    expect(result.summary.success).toBe(false);
  });

  it("aggregates mixed item types", async () => {
    const plan = basePlan([
      createItem(1, ["SKIP_ALREADY_PRESENT"]),
      createItem(2, ["CREATE_LIBRARY_EVENT"]),
      createItem(3, ["SKIP_NON_SONG"], "not a song"),
      createItem(4, ["SKIP_CONFLICT"], "conflict"),
    ]);
    const result = await verifyCuePlan(mockPlanner(plan), defaultConfiguration, "set-1", "setlist-ordinal-test");
    expect(result.summary.verified).toBe(1);
    expect(result.summary.missing).toBe(1);
    expect(result.summary.skipped).toBe(1);
    expect(result.summary.conflicting).toBe(1);
    expect(result.summary.success).toBe(false);
  });
});

