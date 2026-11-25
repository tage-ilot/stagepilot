import { describe, expect, it } from "vitest";
import { compareEvent } from "../src/cues/compare.js";
import type { MidiEvent } from "../src/multitracks/models.js";

const base = { channel: 1, note: 112, velocity: 1, busId: "aux-1", position: { measure: 1, beat: 2, tick: 0 }, malformed: false, raw: {} };

describe("compareEvent", () => {
  it("returns exact when all fields match", () => {
    const actual: MidiEvent = { ...base };
    const expected: MidiEvent = { ...base };
    expect(compareEvent(actual, expected, "aux-1")).toBe("exact");
  });

  it("returns conflict when velocity differs", () => {
    const actual: MidiEvent = { ...base, velocity: 2 };
    const expected: MidiEvent = { ...base, velocity: 1 };
    expect(compareEvent(actual, expected, "aux-1")).toBe("conflict");
  });

  it("returns other-position when position differs", () => {
    const actual: MidiEvent = { ...base, position: { measure: 2, beat: 1, tick: 0 } };
    const expected: MidiEvent = { ...base };
    expect(compareEvent(actual, expected, "aux-1")).toBe("other-position");
  });

  it("returns unrelated when bus differs", () => {
    const actual: MidiEvent = { ...base, busId: "usb-2" };
    const expected: MidiEvent = { ...base };
    expect(compareEvent(actual, expected, "aux-1")).toBe("unrelated");
  });

  it("returns unrelated when channel differs", () => {
    const actual: MidiEvent = { ...base, channel: 2 };
    const expected: MidiEvent = { ...base };
    expect(compareEvent(actual, expected, "aux-1")).toBe("unrelated");
  });

  it("returns unrelated when note differs", () => {
    const actual: MidiEvent = { ...base, note: 60 };
    const expected: MidiEvent = { ...base };
    expect(compareEvent(actual, expected, "aux-1")).toBe("unrelated");
  });

  it("returns malformed when event is malformed", () => {
    const actual: MidiEvent = { ...base, malformed: true };
    const expected: MidiEvent = { ...base };
    expect(compareEvent(actual, expected, "aux-1")).toBe("malformed");
  });

  it("uses implied bus ID when actual has no busId", () => {
    const actual: MidiEvent = { channel: 1, note: 112, velocity: 1, position: { measure: 1, beat: 2, tick: 0 }, malformed: false, raw: {} };
    const expected: MidiEvent = { ...base };
    expect(compareEvent(actual, expected, "aux-1")).toBe("exact");
  });
});

