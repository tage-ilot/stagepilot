import { describe, expect, it } from "vitest";

import { SECTION_ABBREVIATIONS, SECTION_COLORS, sectionColor, UNKNOWN_SECTION_COLOR } from "./sectionColors";

describe("Playback's section color table", () => {
  it("holds all 72 names and ten distinct colors", () => {
    expect(Object.keys(SECTION_COLORS)).toHaveLength(72);
    expect(new Set(Object.values(SECTION_COLORS))).toEqual(new Set([
      "#43A873", "#E55E73", "#DB4439", "#FC620F", "#A0A0A0", "#2AB5BD", "#78CB1F", "#FEBA00", "#7566FF", "#0096FF",
    ]));
    expect(Object.values(SECTION_COLORS).every((hex) => /^#[0-9A-F]{6}$/.test(hex))).toBe(true);
  });

  it("looks each family up by name", () => {
    for (const [hex, names] of Object.entries({
      "#43A873": ["Refrain", "Post Chorus", "Breakdown", "Acapella", "Post-Chorus 3", "Refrain 4"],
      "#E55E73": ["Bridge", "Bridge 1", "Bridge 8", "Rap"],
      "#DB4439": ["Channel", "Interlude", "Vamp"],
      "#FC620F": ["Chorus", "Chorus 8", "Tag", "Tag 4"],
      "#A0A0A0": ["Count Off", "Pad", "Click"],
      "#2AB5BD": ["Intro", "Ending", "Outro", "Solo"],
      "#78CB1F": ["Instrumental", "Turnaround"],
      "#FEBA00": ["Pre Chorus", "Pre Chorus 4"],
      "#7566FF": ["Verse", "Verse 8", "Exhortation"],
      "#0096FF": ["Vamp on 1", "Vamp on 1 and 1sus", "Vamp on 4", "Vamp Chorus", "Vamp Bridge", "Vamp Ending on 1", "Vamp Ending on 4", "Vamp Verse", "Vamp Pre Chorus"],
    })) {
      for (const name of names) expect(sectionColor(name), name).toBe(hex);
    }
  });

  it("does not guess by first word: plain Vamp is red, every Vamp on ... is blue", () => {
    expect(sectionColor("Vamp")).toBe("#DB4439");
    expect(sectionColor("Vamp on 1")).toBe("#0096FF");
    expect(sectionColor("Vamp Chorus")).toBe("#0096FF");
    expect(sectionColor("Vamp on 2")).toBe(UNKNOWN_SECTION_COLOR);
  });

  it("is exact and case-sensitive: any other name is grey", () => {
    for (const name of ["", "verse", "VERSE", "Verse 9", " Verse", "Verse ", "Guitar Feature", "constructor", "__proto__", "toString", "hasOwnProperty"]) {
      expect(sectionColor(name), JSON.stringify(name)).toBe("#A0A0A0");
    }
    expect(UNKNOWN_SECTION_COLOR).toBe("#A0A0A0");
  });

  it("keeps Playback's short forms for the next task, one for every colored name", () => {
    expect(Object.keys(SECTION_ABBREVIATIONS).sort()).toEqual(Object.keys(SECTION_COLORS).sort());
    expect(SECTION_ABBREVIATIONS).toMatchObject({
      "Count Off": "Co", Intro: "I", "Verse 1": "V1", Chorus: "C", "Pre Chorus": "Pc", "Pre Chorus 2": "Pr", Bridge: "B", Vamp: "Vp", "Vamp on 1": "V1",
      "Vamp on 1 and 1sus": "VS", "Vamp Ending on 4": "E4", "Post-Chorus 2": "P", "Tag 3": "T", Ending: "E", Turnaround: "Ta",
    });
    expect(Object.values(SECTION_ABBREVIATIONS).every((short) => short.length >= 1 && short.length <= 2)).toBe(true);
  });

  it("cannot be changed from outside", () => {
    expect(Object.isFrozen(SECTION_COLORS)).toBe(true);
    expect(Object.isFrozen(SECTION_ABBREVIATIONS)).toBe(true);
  });
});
