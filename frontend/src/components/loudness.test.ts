import { describe, expect, it } from "vitest";

import type { SongLoudness } from "../types";
import { loudnessAltText, loudnessDb, loudnessPath, loudnessPeak } from "./loudness";

const l = (values: number[], step = 0.25): SongLoudness => ({ step_seconds: step, values, takes: 1 });

describe("loudness helpers", () => {
  it("maps the stored byte back to dBFS, 0 meaning no data", () => {
    expect(loudnessDb(0)).toBeNull();
    expect(loudnessDb(1)).toBeCloseTo(-60, 5);
    expect(loudnessDb(255)).toBeCloseTo(0, 5);
  });

  it("finds the loudest slot (the first one on a tie) and says it in words", () => {
    expect(loudnessPeak(l([0, 5, 9, 9, 3]))).toEqual({ at: 0.5, byte: 9 });
    expect(loudnessPeak(l([0, 0]))).toBeNull();
    expect(loudnessAltText(l(Array.from({ length: 600 }, (_, i) => (i === 520 ? 200 : 10))))).toBe("Loudness: peak at 02:10");
    expect(loudnessAltText(l([0]))).toBe("Loudness: nothing recorded");
  });

  it("scales to the song's own maximum, so a quiet song fills the height as a loud one does", () => {
    const quiet = loudnessPath(l([10, 20]), 0.5)!;
    const loud = loudnessPath(l([100, 200]), 0.5)!;
    expect(quiet).toBe(loud);
    // Peak slot reaches the top (y = 0), the half-level slot half way.
    expect(quiet).toContain("0,100");
    expect(quiet).toMatch(/500,50|0,50/);
    expect(quiet).toContain(",0");
  });

  it("lines slots up with the song length (zoom/scroll stay in sync because x is a share of the length)", () => {
    // 4 slots of 0.25 s in a 4 s song: the data covers the first quarter of the width.
    const path = loudnessPath(l([5, 5, 5, 5]), 4)!;
    expect(path).toContain("250,");
    expect(path).not.toContain("251,");
    expect(path).not.toMatch(/[ ,]1000[ ,]/);
  });

  it("leaves a gap where there is no data and clips to the song", () => {
    const path = loudnessPath(l([5, 0, 5]), 0.75)!;
    expect(path.match(/M/g)).toHaveLength(2);
    const long = loudnessPath(l(Array(40).fill(5)), 2)!;
    expect(long).toContain("1000,");
    expect(long).not.toMatch(/\b[1-9]\d{3,},/);
  });

  it("draws nothing for empty data", () => {
    expect(loudnessPath(l([0, 0]), 10)).toBeNull();
    expect(loudnessPath(l([]), 10)).toBeNull();
    expect(loudnessPath(l([3]), 0)).toBeNull();
  });
});
