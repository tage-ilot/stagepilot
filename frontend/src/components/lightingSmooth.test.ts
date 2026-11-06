import { describe, expect, it } from "vitest";

import type { LightingEffect, OscLightingCue } from "../types";
import {
  MIN_BAR_PX, assignEffectLanes, clampRampStart, effectCycleSamples, effectGeometry, effectLabel, fadeError, formatFade, mergeBlockReason,
  parseFadeInput, rampDescription, rampGeometry, rowSummary, sparklinePoints,
} from "./lightingSmooth";

const row = (id: string, at: number, address: string, value: number, fade = 0): OscLightingCue => ({
  id, at_seconds: at, address: "", args: [], label: "", color: null, source: "captured",
  messages: [{ address, args: [{ type: "float", value }] }], ...(fade ? { fade_seconds: fade } : {}),
});

const effect = (id: string, start: number, end: number, members = 16, period = 4): LightingEffect => ({
  id, label: "", start_seconds: start, end_seconds: end, period_seconds: period,
  members: Array.from({ length: members }, (_, index) => ({ fixture: `P${index + 1}`, offset_seconds: index * 0.1 })),
  cycle: [
    { at_seconds: 0, property: "dimmer", value: [1], fade_seconds: 2 },
    { at_seconds: 2, property: "dimmer", value: [0], fade_seconds: 2 },
  ],
});

describe("ramp bar geometry", () => {
  it("runs from the start to start + fade on the track", () => {
    // 240 s song on 1200 px: 5 px per second.
    const bar = rampGeometry(10, 3.2, 240, 1200);
    expect(bar.left).toBeCloseTo(50, 6);
    expect(bar.width).toBeCloseTo(16, 6);
  });
  it("is never narrower than 4 px", () => {
    expect(rampGeometry(10, 0.1, 240, 1200).width).toBe(MIN_BAR_PX);
    expect(rampGeometry(10, 0.1, 3600, 800).width).toBe(MIN_BAR_PX);
  });
  it("stays on the track at the very end of the song", () => {
    const bar = rampGeometry(239.9, 5, 240, 1200);
    expect(bar.left + bar.width).toBeLessThanOrEqual(1200);
    expect(bar.width).toBeGreaterThanOrEqual(MIN_BAR_PX);
  });
  it("copes with a track that has no width yet", () => {
    expect(rampGeometry(5, 2, 240, 0)).toEqual({ left: 0, width: MIN_BAR_PX });
  });
});

describe("effect band geometry and lanes", () => {
  it("spans start..end", () => {
    expect(effectGeometry(effect("a", 20, 100), 240, 1200)).toEqual({ left: 100, width: 400 });
  });
  it("puts effects that do not overlap on one lane and overlapping ones on separate lanes", () => {
    const lanes = assignEffectLanes([effect("a", 0, 50), effect("b", 60, 100), effect("c", 40, 80)]);
    expect(lanes.get("a")).toBe(0);
    expect(lanes.get("b")).toBe(0);
    expect(lanes.get("c")).toBe(1);
  });
  it("never uses more than three lanes: further overlapping loops share the lane that frees up first", () => {
    const many = [effect("a", 0, 100), effect("b", 0, 90), effect("c", 0, 80), effect("d", 10, 70), effect("e", 20, 60)];
    const lanes = assignEffectLanes(many);
    expect(Math.max(...lanes.values())).toBe(2);
    expect(lanes.get("d")).toBe(lanes.get("c"));
    expect(assignEffectLanes([])).toEqual(new Map());
  });
  it("labels the loop with its period and fixtures", () => {
    expect(effectLabel(effect("a", 0, 10, 16))).toBe("4.0 s loop x 16 fixtures");
    expect(effectLabel(effect("a", 0, 10, 1, 7.95))).toBe("8.0 s loop x 1 fixture");
  });
});

describe("one cycle of a loop", () => {
  it("starts the first step from the last step's value", () => {
    const samples = effectCycleSamples({ period_seconds: 4, cycle: [
      { at_seconds: 0, property: "dimmer", value: [1], fade_seconds: 2 },
      { at_seconds: 2, property: "dimmer", value: [0], fade_seconds: 2 },
    ] }, "dimmer", 5);
    expect(samples[0]).toBeCloseTo(0, 5); // t=0: just starting to rise from the last value (0)
    expect(samples[1]).toBeCloseTo(0.5, 5); // t=1: halfway up
    expect(samples[2]).toBeCloseTo(1, 5); // t=2: the second step starts from 1
    expect(samples[3]).toBeCloseTo(0.5, 5);
    expect(samples[4]).toBeCloseTo(0, 5);
  });
  it("returns nothing for a property the cycle does not name", () => {
    expect(effectCycleSamples(effect("a", 0, 10), "pan")).toEqual([]);
  });
  it("scales the sparkline into its box", () => {
    expect(sparklinePoints([0, 1], 10, 4)).toBe("0.0,4.0 10.0,0.0");
    expect(sparklinePoints([0.5, 0.5], 10, 4)).toBe("0.0,2.0 10.0,2.0");
  });
});

describe("what a ramp says", () => {
  const before = row("a", 1, "/fixture/MH1/overrides/dimmer", 0.2);
  const ramp = row("b", 5, "/fixture/MH1/overrides/dimmer", 0.9, 3.2);
  it("shows fixture, property, from -> to for each message", () => {
    expect(rowSummary(ramp, [before, ramp])).toEqual([{ fixture: "MH1", property: "dimmer", from: "0.2", to: "0.9", ramps: true }]);
    expect(rampDescription(ramp, [before, ramp])).toBe("MH1: dimmer 0.2 → 0.9 over 3.2 s");
  });
  it("has no from value when nothing set the property earlier", () => {
    expect(rowSummary(ramp, [ramp])[0]!.from).toBeNull();
  });
  it("formats the duration column", () => {
    expect(formatFade(3.2)).toBe("+3.2 s");
    expect(formatFade(0.05)).toBe("+0.1 s");
  });
});

describe("ramp edit validation", () => {
  it("accepts 0 to 60 seconds inside the song", () => {
    expect(fadeError(0, 10, 240)).toBeNull();
    expect(fadeError(60, 10, 240)).toBeNull();
    expect(fadeError(3.2, 236.8, 240)).toBeNull();
  });
  it("refuses a negative, a too long and a past-the-end ramp, and a non-number", () => {
    expect(fadeError(-1, 0, 240)).toMatch(/negative/);
    expect(fadeError(60.1, 0, 240)).toMatch(/60 s/);
    expect(fadeError(5, 238, 240)).toMatch(/past the end/);
    expect(fadeError(null, 0, 240)).toMatch(/number/);
  });
  it("does not check the end when the song length is unknown", () => {
    expect(fadeError(30, 1000, null)).toBeNull();
  });
  it("parses what people type", () => {
    expect(parseFadeInput("3.2")).toBe(3.2);
    expect(parseFadeInput(" 3,2 s ")).toBe(3.2);
    expect(parseFadeInput("")).toBeNull();
    expect(parseFadeInput("soon")).toBeNull();
    expect(parseFadeInput("-2")).toBeNull();
  });
  it("keeps a ramp inside the song when it is moved", () => {
    expect(clampRampStart(239, 5, 240)).toBe(235);
    expect(clampRampStart(-1, 5, 240)).toBe(0);
    expect(clampRampStart(10, 5, null)).toBe(10);
  });
});

describe("merge rules", () => {
  const instantA = row("i1", 1, "/fixture/A/overrides/dimmer", 0.1);
  const instantB = row("i2", 2, "/fixture/B/overrides/dimmer", 0.2);
  const rampA = row("r1", 10, "/fixture/A/overrides/dimmer", 0.9, 3);
  const rampB = row("r2", 10.01, "/fixture/B/overrides/dimmer", 0.9, 3);
  it("allows instant rows together (as before)", () => {
    expect(mergeBlockReason([instantA, instantB], ["i1", "i2"])).toBeNull();
  });
  it("allows ramps of the same duration that start together on different properties", () => {
    expect(mergeBlockReason([rampA, rampB], ["r1", "r2"])).toBeNull();
  });
  it("refuses a ramp with an instant row and says why", () => {
    expect(mergeBlockReason([instantA, rampA], ["i1", "r1"])).toMatch(/instant row/);
  });
  it("refuses ramps of different duration, different start, or the same property", () => {
    expect(mergeBlockReason([rampA, { ...rampB, fade_seconds: 4 }], ["r1", "r2"])).toMatch(/different durations/);
    expect(mergeBlockReason([rampA, { ...rampB, at_seconds: 12 }], ["r1", "r2"])).toMatch(/start at different times/);
    expect(mergeBlockReason([rampA, { ...rampA, id: "r3" }], ["r1", "r3"])).toMatch(/same property/);
  });
  it("has no reason with fewer than two rows", () => {
    expect(mergeBlockReason([rampA], ["r1"])).toBeNull();
  });
});
