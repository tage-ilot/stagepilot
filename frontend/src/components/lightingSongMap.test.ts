import { describe, expect, it } from "vitest";

import type { LibrarySection } from "../types";
import { arrangementBlocks, blockLabel, LABEL_PADDING_PX, MAX_WAVEFORM_COLUMNS, waveformPath } from "./lightingSongMap";
import { sectionColor } from "./sectionColors";

const sectionsOf = (...parts: Array<[string, number, number]>): LibrarySection[] => parts.map(([name, start, end]) => ({ name, start, end }));

describe("arrangementBlocks", () => {
  it("places each section by its own seconds on the timeline's scale (the cue markers' at / length), in playing order", () => {
    const blocks = arrangementBlocks(sectionsOf(["Count Off", 0, 4], ["Verse 1", 4, 24], ["Chorus", 24, 50], ["Verse 1", 50, 100]), 100);
    expect(blocks.map((b) => [b.name, b.leftPct, b.widthPct])).toEqual([["Count Off", 0, 4], ["Verse 1", 4, 20], ["Chorus", 24, 26], ["Verse 1", 50, 50]]);
    // Repeats are separate blocks with their own place.
    expect(blocks.map((b) => b.index)).toEqual([0, 1, 2, 3]);
  });

  it("cuts what runs past the end of the timeline and drops what starts at or after it", () => {
    // The arrangement can run a few seconds past the countdown length; the track never grows for it.
    const blocks = arrangementBlocks(sectionsOf(["Verse", 0, 90], ["Chorus", 90, 104], ["Ending", 100, 110], ["Pad", 120, 130]), 100);
    expect(blocks.map((b) => b.name)).toEqual(["Verse", "Chorus"]);
    expect(blocks[1]!.leftPct + blocks[1]!.widthPct).toBeCloseTo(100, 9);
    for (const block of blocks) expect(block.leftPct + block.widthPct).toBeLessThanOrEqual(100 + 1e-9);
  });

  it("keeps the real times in the hover text even where the block is cut", () => {
    const [block] = arrangementBlocks(sectionsOf(["Chorus", 90, 134.9]), 100);
    expect(block!.title).toBe("Chorus · 1:30–2:14");
  });

  it("gives the section its Playback color and short form (grey and no form for a name the table does not know)", () => {
    const [verse, vamp, unknown] = arrangementBlocks(sectionsOf(["Verse 1", 0, 10], ["Vamp on 1", 10, 20], ["Made Up", 20, 30]), 100);
    expect([verse!.color, verse!.abbreviation]).toEqual([sectionColor("Verse 1"), "V1"]);
    expect([vamp!.color, vamp!.abbreviation]).toEqual([sectionColor("Vamp on 1"), "V1"]);
    expect(vamp!.color).not.toBe(sectionColor("Vamp"));
    expect([unknown!.color, unknown!.abbreviation]).toEqual(["#A0A0A0", null]);
  });

  it("skips sections that cannot be placed (no length, backwards, not numbers) and answers nothing for no usable timeline", () => {
    const odd = sectionsOf(["A", 5, 5], ["B", 9, 3], ["C", Number.NaN, 4], ["D", 1, Number.POSITIVE_INFINITY], ["Verse", -5, 10]);
    expect(arrangementBlocks(odd, 100).map((b) => [b.name, b.leftPct, b.widthPct])).toEqual([["Verse", 0, 10]]);
    expect(arrangementBlocks(sectionsOf(["Verse", 0, 10]), 0)).toEqual([]);
    expect(arrangementBlocks(sectionsOf(["Verse", 0, 10]), Number.NaN)).toEqual([]);
    expect(arrangementBlocks([], 100)).toEqual([]);
  });

  it("a song shorter than its arrangement keeps only the part inside the song", () => {
    const blocks = arrangementBlocks(sectionsOf(["Intro", 0, 20], ["Verse", 20, 60], ["Chorus", 60, 120]), 50);
    expect(blocks.map((b) => [b.name, b.leftPct, b.widthPct])).toEqual([["Intro", 0, 40], ["Verse", 40, 60]]);
  });
});

describe("blockLabel", () => {
  const measure = (text: string) => text.length * 6;
  it("is the short form when its measured width plus 8 px fits the block, else nothing", () => {
    expect(LABEL_PADDING_PX).toBe(8);
    expect(blockLabel("V1", 20, measure)).toBe("V1"); // 12 + 8 = 20: exactly fits
    expect(blockLabel("V1", 19.9, measure)).toBeNull();
    expect(blockLabel("C", 14, measure)).toBe("C");
    expect(blockLabel("C", 13, measure)).toBeNull();
  });
  it("is nothing for a block with no short form or no width", () => {
    expect(blockLabel(null, 500, measure)).toBeNull();
    expect(blockLabel(undefined, 500, measure)).toBeNull();
    expect(blockLabel("", 500, measure)).toBeNull();
    expect(blockLabel("V1", 0, measure)).toBeNull();
    expect(blockLabel("V1", Number.NaN, measure)).toBeNull();
  });
  it("asks the injected measure for the short form only", () => {
    const seen: string[] = [];
    blockLabel("B2", 100, (text) => { seen.push(text); return 1; });
    expect(seen).toEqual(["B2"]);
  });
});

describe("waveformPath", () => {
  it("is one closed, mirrored shape around the center line, in a 2400 x 100 box spanning the timeline's length", () => {
    expect(waveformPath({ step_s: 1, peak: [255, 0] }, 2)).toBe("M0,0 L600,0 L1800,50 L2400,50 L2400,50 L1800,50 L600,100 L0,100 Z");
  });

  it("is normalised to the song's own loudest point, whatever the scale of the numbers", () => {
    const loud = waveformPath({ step_s: 1, peak: [100, 50] }, 2);
    const quiet = waveformPath({ step_s: 1, peak: [10, 5] }, 2);
    expect(loud).toBe(quiet);
    expect(loud).toContain("L1800,25"); // the second point is half as tall as the loudest
  });

  it("ends where its numbers end when the waveform is shorter than the timeline", () => {
    const path = waveformPath({ step_s: 1, peak: [255, 255] }, 4)!;
    expect(path).toBe("M0,0 L300,0 L900,0 L1200,0 L1200,100 L900,100 L300,100 L0,100 Z");
    const xs = [...path.matchAll(/[ML](-?[\d.]+),/g)].map((m) => Number(m[1]));
    expect(Math.max(...xs)).toBe(1200);
  });

  it("is cut at the end of the timeline when the waveform is longer", () => {
    const peak = Array.from({ length: 20 }, (_, i) => 10 + i);
    const path = waveformPath({ step_s: 1, peak }, 4)!;
    const xs = [...path.matchAll(/[ML](-?[\d.]+),/g)].map((m) => Number(m[1]));
    expect(Math.max(...xs)).toBe(2400);
    // Four points are drawn (two edges of four columns, plus the two end caps and the two start points).
    expect(xs).toHaveLength(4 + 1 + 1 + 1 + 4 + 1);
    // Normalised to the loudest of the WHOLE song, not just the part that is drawn.
    expect(path).toContain("L300,"); // first column's center
    const firstTopY = Number(/^M[\d.]+,([\d.]+)/.exec(path)![1]);
    expect(firstTopY).toBeGreaterThan(30); // 10 of 29 is a third of the loudest, not full height
  });

  it("a song shorter than its arrangement draws only the part inside the song", () => {
    // 10 s of numbers, but the countdown ends at 5 s.
    const path = waveformPath({ step_s: 1, peak: Array.from({ length: 10 }, () => 200) }, 5)!;
    const xs = [...path.matchAll(/[ML](-?[\d.]+),/g)].map((m) => Number(m[1]));
    expect(Math.max(...xs)).toBe(2400);
    expect(xs.filter((x) => x === 2400)).toHaveLength(2); // the two end caps; nothing is drawn past the song
  });

  it("never draws more than 2400 columns however many numbers there are, each the loudest of the points it covers", () => {
    const peak = Array.from({ length: 10_000 }, (_, i) => (i === 5_000 ? 255 : 10));
    const path = waveformPath({ step_s: 0.1, peak }, 1000)!;
    const tops = path.split(" L").slice(1, 1 + MAX_WAVEFORM_COLUMNS);
    expect(tops.length).toBeLessThanOrEqual(MAX_WAVEFORM_COLUMNS);
    expect(path.split(" ").length).toBeLessThanOrEqual(MAX_WAVEFORM_COLUMNS * 2 + 8);
    // The one loud point still reaches full height.
    expect(path).toMatch(/,0 /);
  });

  it("is null when there is nothing to draw", () => {
    expect(waveformPath({ step_s: 1, peak: [] }, 10)).toBeNull();
    expect(waveformPath({ step_s: 1, peak: [0, 0, 0] }, 10)).toBeNull();
    expect(waveformPath({ step_s: 0, peak: [1, 2] }, 10)).toBeNull();
    expect(waveformPath({ step_s: Number.NaN, peak: [1, 2] }, 10)).toBeNull();
    expect(waveformPath({ step_s: 1, peak: [1, 2] }, 0)).toBeNull();
    expect(waveformPath({ step_s: 1, peak: [1, 2] }, Number.NaN)).toBeNull();
  });
});
