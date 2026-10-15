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
  MARKER_LABEL_MAX_PX,
  MARKER_LABEL_ROWS,
  markerLabelWidth,
  placeMarkerLabels,
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

describe("marker label placement", () => {
  const m = (id: string, label: string, at: number) => ({ id, label, at });

  it("sizes a label by its text and caps it", () => {
    expect(markerLabelWidth("Hi")).toBeLessThan(markerLabelWidth("Chorus wash"));
    expect(markerLabelWidth("A very long cue name that goes on")).toBe(MARKER_LABEL_MAX_PX);
  });

  it("keeps far-apart labels on the top row", () => {
    const placed = placeMarkerLabels([m("a", "Chorus wash", 10), m("b", "Blackout", 200)], 1200, 280);
    expect(placed.get("a")?.row).toBe(0);
    expect(placed.get("b")?.row).toBe(0);
  });

  it("drops a label to the next row when it would run into the one before it (cues 4 s apart on a 4:41 song)", () => {
    const placed = placeMarkerLabels([m("a", "Chorus wash", 8.2), m("b", "Blackout", 12.2)], 1285, 281);
    expect(placed.get("a")?.row).toBe(0);
    expect(placed.get("b")?.row).toBe(1);
  });

  it("reuses the top row once the earlier label has ended", () => {
    const placed = placeMarkerLabels([m("a", "Chorus wash", 8), m("b", "Blackout", 12), m("c", "Strobe", 100)], 1285, 281);
    expect([placed.get("a")?.row, placed.get("b")?.row, placed.get("c")?.row]).toEqual([0, 1, 0]);
  });

  it("when every row is busy, the last label shares a row and the earlier one is cut short at it", () => {
    const cues = Array.from({ length: MARKER_LABEL_ROWS + 1 }, (_, i) => m(`c${i}`, "Chorus wash", 10 + i));
    const placed = placeMarkerLabels(cues, 1285, 281);
    const rows = cues.map((cue) => placed.get(cue.id)!.row);
    expect(Math.max(...rows)).toBe(MARKER_LABEL_ROWS - 1);
    expect(new Set(rows).size).toBe(MARKER_LABEL_ROWS);
    const shortened = cues.filter((cue) => placed.get(cue.id)!.width < markerLabelWidth("Chorus wash"));
    expect(shortened.length).toBeGreaterThan(0);
    for (const cue of cues) expect(placed.get(cue.id)!.width).toBeGreaterThanOrEqual(0);
  });

  it("stops a label short of a later marker line that would cross it, but not of one that starts on a row below", () => {
    // Two cues 13 px apart: Blackout is on the top row and the next marker line would run through its text.
    const crossing = placeMarkerLabels([m("a", "Blackout", 100), m("b", "Chorus wash", 100 + (13 / 1285) * 281)], 1285, 281);
    expect(crossing.get("a")?.row).toBe(0);
    expect(crossing.get("b")?.row).toBe(1);
    // b's line starts on row 1, below a's label on row 0, so a's label is NOT cut for b.
    expect(crossing.get("a")?.width).toBe(markerLabelWidth("Blackout"));
    // A later marker that lands on a row at or above a label does cut it: A (row 0), B (row 1, long name), then C back on row 0.
    const px = (pixels: number) => (pixels / 1285) * 281;
    const cut = placeMarkerLabels([m("a", "Chorus wash", px(0)), m("b", "Chorus wash", px(20)), m("c", "X", px(95))], 1285, 281);
    expect([cut.get("a")?.row, cut.get("b")?.row, cut.get("c")?.row]).toEqual([0, 1, 0]);
    expect(cut.get("b")?.width).toBeCloseTo(95 - 20 - 10, 5);
    expect(cut.get("b")?.width).toBeLessThan(markerLabelWidth("Chorus wash"));
  });

  it("does not depend on the order the cues are given in", () => {
    const a = placeMarkerLabels([m("a", "One", 8), m("b", "Two", 12)], 1285, 281);
    const b = placeMarkerLabels([m("b", "Two", 12), m("a", "One", 8)], 1285, 281);
    expect([...a.entries()]).toEqual([...b.entries()].sort(([x], [y]) => x.localeCompare(y)));
  });

  it("is safe before the track has been measured", () => {
    const placed = placeMarkerLabels([m("a", "One", 8)], 0, 281);
    expect(placed.get("a")).toEqual({ row: 0, width: markerLabelWidth("One") });
  });
});
