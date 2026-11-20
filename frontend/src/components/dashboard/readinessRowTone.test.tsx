import { describe, expect, it } from "vitest";

import { READINESS_ROW_MARK, READINESS_ROW_MARK_CLASS, READINESS_ROW_TEXT_CLASS, readinessRowTone, type ReadinessCheck } from "./dashboardReadiness";

const check = (over: Partial<ReadinessCheck>): ReadinessCheck => ({ id: "x", label: "Row", passed: false, required: true, severity: "blocking", status: "disconnected", ...over });

/** D3 (design audit): in the header's check list the red mark is on the row that blocks a service; an optional row that is not ready is grey. */
describe("header check list row looks", () => {
  it.each([
    ["a passed required row", check({ passed: true, status: "connected" }), "ok", "\u2713"],
    ["a passed optional row", check({ passed: true, required: false, severity: "warning", status: "connected" }), "ok", "\u2713"],
    ["a blocking row that is not connected (it used to be a grey dash)", check({ status: "disconnected" }), "blocking", "\u00D7"],
    ["a blocking row in error", check({ status: "error" }), "blocking", "\u00D7"],
    ["an optional row in error (the Playback library; it used to be the only red one)", check({ required: false, severity: "warning", status: "error" }), "optional", "!"],
    ["an optional row that is only off", check({ required: false, severity: "info", status: "disconnected" }), "optional", "!"],
  ])("%s", (_name, row, tone, mark) => {
    expect(readinessRowTone(row)).toBe(tone);
    expect(READINESS_ROW_MARK[readinessRowTone(row)]).toBe(mark);
  });

  it("never rests on the colour alone: each tone has its own mark, and only blocking is red", () => {
    expect(new Set(Object.values(READINESS_ROW_MARK)).size).toBe(3);
    expect(READINESS_ROW_MARK_CLASS.blocking).toMatch(/danger/);
    expect(READINESS_ROW_TEXT_CLASS.blocking).toMatch(/danger/);
    expect(READINESS_ROW_MARK_CLASS.optional).not.toMatch(/danger|warning/);
    expect(READINESS_ROW_TEXT_CLASS.optional).toBe("text-ink-muted");
  });
});
