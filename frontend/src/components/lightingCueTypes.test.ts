import { describe, expect, it } from "vitest";

import type { LightingCue, OscLightingCue } from "../types";
import {
  CUE_TYPES, classifyMidiCue, classifyOscCue, midiNoteName, typeAccessibleName, OSC_TYPE_IDS, MIDI_TYPE_IDS,
} from "./lightingCueTypes";

const osc = (over: Partial<OscLightingCue>): OscLightingCue => ({ id: "x", at_seconds: 1, address: "", args: [], label: "", ...over });
const msgs = [{ address: "/fixture/MH1/overrides/dimmer", args: [] }];

describe("classifyOscCue", () => {
  const table: [string, Partial<OscLightingCue>, string][] = [
    ["captured look", { source: "captured", messages: msgs }, "look-captured"],
    ["captured look, edited false", { source: "captured", edited: false, messages: msgs }, "look-captured"],
    ["captured look that was edited", { source: "captured", edited: true, messages: msgs }, "look-edited"],
    ["manual look (source manual)", { source: "manual", messages: msgs }, "look-manual"],
    ["manual look (no source)", { messages: msgs }, "look-manual"],
    ["edited flag on a manual look is ignored", { source: "manual", edited: true, messages: msgs }, "look-manual"],
    ["cue on", { address: "/live/Main/cue/spots/activate" }, "cue-on"],
    ["cue off", { address: "/live/Main/cue/spots/deactivate" }, "cue-off"],
    ["cue name with spaces", { address: "/live/Main/cue/Stage Spread/activate" }, "cue-on"],
    ["cue name with spaces, off", { address: "/live/Main/cue/Stage Spread/deactivate" }, "cue-off"],
    ["cue in a nested path name", { address: "/live/Main/cue/a/b/activate" }, "cue-on"],
    ["fixture message", { address: "/fixture/MH1/overrides/dimmer" }, "fixture"],
    ["fixture name with spaces", { address: "/fixture/Moving Head 1/overrides/panAngle" }, "fixture"],
    ["empty messages array with an address", { messages: [], address: "/fixture/MH1/overrides/dimmer" }, "fixture"],
    ["empty messages array with a cue address", { messages: [], address: "/live/Main/cue/x/activate" }, "cue-on"],
    ["empty messages, captured source, fixture address", { source: "captured", messages: [], address: "/fixture/A/overrides/zoom" }, "fixture"],
    ["fixture address without overrides", { address: "/fixture/MH1/dimmer" }, "other"],
    ["fixture address with nothing after overrides", { address: "/fixture/MH1/overrides/" }, "other"],
    ["fixture address with empty name", { address: "/fixture//overrides/dimmer" }, "other"],
    ["cue address with empty name", { address: "/live/Main/cue//activate" }, "other"],
    ["cue address missing the verb", { address: "/live/Main/cue/spots" }, "other"],
    ["cue address with another verb", { address: "/live/Main/cue/spots/toggle" }, "other"],
    ["verb with trailing path", { address: "/live/Main/cue/spots/activate/now" }, "other"],
    ["not an absolute path", { address: "live/Main/cue/spots/activate" }, "other"],
    ["prefix lookalike", { address: "/liveX/Main/cue/spots/activate" }, "other"],
    ["plain address", { address: "/haze/burst" }, "other"],
    ["empty address and no messages", { address: "" }, "other"],
    ["garbage", { address: "///" }, "other"],
  ];
  it.each(table)("%s", (_name, over, expected) => {
    expect(classifyOscCue(osc(over))).toBe(expected);
  });
});

describe("classifyMidiCue and note names", () => {
  const cue = (note: number): LightingCue => ({ id: "n", at_seconds: 0, note, velocity: 100, label: "" });
  const letters = ["C", "C", "D", "D", "E", "F", "F", "G", "G", "A", "A", "B"];
  const sharps = [false, true, false, true, false, false, true, false, true, false, true, false];
  const spelled = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

  it("maps every pitch class to its letter colour, sharps flagged", () => {
    for (let pc = 0; pc < 12; pc += 1) {
      const result = classifyMidiCue(cue(60 + pc));
      expect(result.type).toBe(`note-${letters[pc]}`);
      expect(result.sharp).toBe(sharps[pc]);
      expect(result.letter).toBe(letters[pc]);
    }
  });

  it("covers octaves 0-9 and notes 0-127 and is octave independent", () => {
    for (let note = 0; note <= 127; note += 1) {
      const result = classifyMidiCue(cue(note));
      expect(result.type).toBe(`note-${letters[note % 12]}`);
      expect(result.sharp).toBe(sharps[note % 12]);
      expect(midiNoteName(note)).toBe(`${spelled[note % 12]}${Math.floor(note / 12) - 2}`);
    }
    for (const octaveBase of [0, 12, 24, 36, 48, 60, 72, 84, 96, 108]) {
      expect(classifyMidiCue(cue(octaveBase)).type).toBe("note-C");
    }
    expect(midiNoteName(0)).toBe("C-2");
    expect(midiNoteName(127)).toBe("G8");
  });

  it("is a pure function (same input, same output, input untouched)", () => {
    const input = cue(61);
    const copy = JSON.stringify(input);
    expect(classifyMidiCue(input)).toEqual(classifyMidiCue(input));
    expect(JSON.stringify(input)).toBe(copy);
  });

  it("does not throw on an out-of-range note", () => {
    expect(() => classifyMidiCue(cue(-1))).not.toThrow();
    expect(classifyMidiCue(cue(-1)).type).toBe("note-B");
    expect(classifyMidiCue(cue(130)).type).toBe("note-A");
  });
});

describe("type table", () => {
  it("has the glyphs from the spec", () => {
    const glyph = (id: string) => CUE_TYPES[id as keyof typeof CUE_TYPES].glyph;
    expect([glyph("look-captured"), glyph("look-edited"), glyph("look-manual"), glyph("cue-on"), glyph("cue-off"), glyph("fixture"), glyph("other")])
      .toEqual(["◆", "◆✎", "◇", "▶", "■", "◐", "○"]);
    expect(MIDI_TYPE_IDS).toEqual(["note-C", "note-D", "note-E", "note-F", "note-G", "note-A", "note-B"]);
    expect(OSC_TYPE_IDS).toHaveLength(7);
  });

  it("builds an accessible name that never relies on colour", () => {
    expect(typeAccessibleName("look-captured", "Look - 12 fixtures", "00:14")).toBe("Captured look, Look - 12 fixtures, at 00:14");
  });
});
