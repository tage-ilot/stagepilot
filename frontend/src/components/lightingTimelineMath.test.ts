import { describe, expect, it } from "vitest";

import type { LightingPad } from "../types";
import { formatElapsed, parseElapsed } from "./lightingOsc";
import {
  clampCueTime,
  elapsedSince,
  movePad,
  nudgeCueTime,
  padForShortcut,
  padIsActive,
  padIsDimmed,
  snapTenths,
  timeFromTrackOffset,
} from "./lightingTimelineMath";

const pad = (id: string, method: "osc" | "midi"): LightingPad => ({
  id, label: id, color: null, method, address: method === "osc" ? "/a" : null, args: [], note: method === "midi" ? 1 : null, velocity: 127,
});

describe("timeline time maths", () => {
  it("snaps to 0.1 s and clamps to the song length", () => {
    expect(snapTenths(72.04)).toBe(72);
    expect(snapTenths(12.26)).toBe(12.3);
    expect(clampCueTime(-4, 200)).toBe(0);
    expect(clampCueTime(250.3, 200)).toBe(200);
    expect(clampCueTime(99.949, 200)).toBe(99.9);
    expect(clampCueTime(500, null)).toBe(500);
  });

  it("maps a pointer offset on the track to a snapped time, clamped at both ends", () => {
    expect(timeFromTrackOffset(250, 1000, 200)).toBe(50);
    expect(timeFromTrackOffset(333, 1000, 200)).toBe(66.6);
    expect(timeFromTrackOffset(-40, 1000, 200)).toBe(0);
    expect(timeFromTrackOffset(1200, 1000, 200)).toBe(200);
    expect(timeFromTrackOffset(10, 0, 200)).toBe(0);
  });

  it("nudges 0.1 s, or 1 s with Shift, and stays inside the song", () => {
    expect(nudgeCueTime(10, 1, false, 200)).toBe(10.1);
    expect(nudgeCueTime(10, -1, true, 200)).toBe(9);
    expect(nudgeCueTime(0, -1, false, 200)).toBe(0);
    expect(nudgeCueTime(199.95, 1, true, 200)).toBe(200);
    expect(nudgeCueTime(0.3, 1, false, 200) - 0.4).toBeCloseTo(0, 10);
  });

  it("formats and parses tenths without changing whole-second times", () => {
    expect(formatElapsed(72)).toBe("01:12");
    expect(formatElapsed(72.3)).toBe("01:12.3");
    expect(parseElapsed("01:12.3")).toBe(72.3);
    expect(parseElapsed("01:12")).toBe(72);
    expect(parseElapsed("1:60")).toBeNull();
  });

  it("derives the playhead from the start time and the resume position", () => {
    const start = new Date(1_000_000).toISOString();
    expect(elapsedSince(start, 0, 1_072_000)).toBe(72);
    expect(elapsedSince(start, 30, 1_010_000)).toBe(40);
    expect(elapsedSince(null, 0, 5)).toBeNull();
    expect(elapsedSince(start, 0, 900_000)).toBe(0);
  });
});

describe("pad rules", () => {
  it("keeps a pad of the other method dimmed and unusable", () => {
    const osc = pad("a", "osc");
    const midi = pad("b", "midi");
    expect(padIsDimmed(midi, "osc")).toBe(true);
    expect(padIsDimmed(osc, "osc")).toBe(false);
    expect(padIsActive(osc, "osc", "osc")).toBe(true);
    expect(padIsActive(osc, "osc", "midi")).toBe(false); // viewing OSC but MIDI is saved
    expect(padIsActive(midi, "osc", "osc")).toBe(false);
  });

  it("reorders by one place and ignores out-of-range moves", () => {
    const list = ["a", "b", "c"];
    expect(movePad(list, 1, -1)).toEqual(["b", "a", "c"]);
    expect(movePad(list, 1, 1)).toEqual(["a", "c", "b"]);
    expect(movePad(list, 0, -1)).toBe(list);
    expect(movePad(list, 2, 1)).toBe(list);
  });

  it("maps keys 1-9 to the Nth pad", () => {
    const list = [pad("a", "osc"), pad("b", "osc")];
    expect(padForShortcut(list, "1")?.id).toBe("a");
    expect(padForShortcut(list, "2")?.id).toBe("b");
    expect(padForShortcut(list, "3")).toBeNull();
    expect(padForShortcut(list, "0")).toBeNull();
    expect(padForShortcut(list, "x")).toBeNull();
  });
});
