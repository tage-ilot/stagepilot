import { describe, expect, it } from "vitest";

import { AUTO_MAX_PX, AUTO_MIN_PX, autoTimelineHeight, heightFor, labelRowsFor, readHeightMode } from "./timelineHeight";

describe("autoTimelineHeight", () => {
  it.each([[0, 200], [1, 200], [2, 220], [4, 264], [6, 308], [8, 352], [12, 420], [30, 420]])("%i label rows -> %i px", (rows, px) => {
    expect(autoTimelineHeight(rows)).toBe(px);
  });
  it("grows with rows and stays inside the limits", () => {
    let last = 0;
    for (let rows = 0; rows <= 40; rows += 1) {
      const height = autoTimelineHeight(rows);
      expect(height).toBeGreaterThanOrEqual(AUTO_MIN_PX);
      expect(height).toBeLessThanOrEqual(AUTO_MAX_PX);
      expect(height).toBeGreaterThanOrEqual(last);
      last = height;
    }
  });
});

describe("heightFor / labelRowsFor", () => {
  it("S and M are fixed, Auto follows the rows", () => {
    expect(heightFor("s", 9)).toBe(200);
    expect(heightFor("m", 1)).toBe(320);
    expect(heightFor("auto", 8)).toBe(352);
  });
  it("a fixed height leaves room for the waveform under the labels", () => {
    expect(labelRowsFor("s")).toBe(5);
    expect(labelRowsFor("m")).toBe(11);
    expect(labelRowsFor("auto")).toBe(12);
  });
});

describe("readHeightMode", () => {
  it.each([["s", "s"], ["m", "m"], ["auto", "auto"], ["tall", "auto"], [null, "auto"]])("%s", (saved, mode) => {
    expect(readHeightMode(saved)).toBe(mode);
  });
});
