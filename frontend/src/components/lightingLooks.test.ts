import { describe, expect, it } from "vitest";

import type { OscLightingCue } from "../types";
import {
  clusterMarkers, groupByFixture, hasEditedCaptured, isLook, lookLabel, mergeRows, removeFixtureFromRows,
} from "./lightingLooks";

const f = (value: number) => [{ type: "float" as const, value }];
const look = (id: string, at: number, msgs: [string, number][], over: Partial<OscLightingCue> = {}): OscLightingCue => ({
  id, at_seconds: at, address: "", args: [], label: "", source: "captured",
  messages: msgs.map(([address, value]) => ({ address, args: f(value) })), ...over,
});
const dim = (n: string) => `/fixture/${n}/overrides/dimmer`;

describe("look helpers", () => {
  it("labels a look by its distinct fixtures and groups messages by fixture", () => {
    const row = look("a", 1, [[dim("MH1"), 1], ["/fixture/MH1/overrides/panAngle", 2], [dim("MH2"), 3]]);
    expect(isLook(row)).toBe(true);
    expect(lookLabel(row)).toBe("Look - 2 fixtures");
    expect(lookLabel(look("b", 1, [[dim("MH1"), 1]]))).toBe("Look - 1 fixture");
    expect(groupByFixture(row.messages!).map((g) => [g.fixture, g.items.length])).toEqual([["MH1", 2], ["MH2", 1]]);
  });

  it("removes a fixture from every row, drops rows left empty, and marks touched captured rows edited", () => {
    const rows = [look("a", 1, [[dim("MH1"), 1], [dim("MH2"), 1]]), look("b", 2, [[dim("MH1"), 0]]), look("c", 3, [[dim("MH2"), 0]])];
    const next = removeFixtureFromRows(rows, "MH1");
    expect(next.map((r) => r.id)).toEqual(["a", "c"]);
    expect(next[0]!.messages).toHaveLength(1);
    expect(next[0]!.edited).toBe(true);
    expect(next[1]!.edited).toBeUndefined();
    expect(hasEditedCaptured(next)).toBe(true);
  });
});

describe("mergeRows", () => {
  it("merges at the earliest time, latest value per address wins, and reports changed addresses", () => {
    const rows = [look("a", 5, [[dim("A"), 1], [dim("B"), 1]]), look("b", 2, [[dim("A"), 0.5]]), look("c", 9, [[dim("A"), 0.9]])];
    const result = mergeRows(rows, ["a", "b", "c"])!;
    expect(result.cues).toHaveLength(1);
    expect(result.cues[0]).toMatchObject({ id: "b", at_seconds: 2, edited: true, source: "captured" });
    const values = Object.fromEntries(result.cues[0]!.messages!.map((m) => [m.address, m.args[0]!.value]));
    expect(values).toEqual({ [dim("A")]: 0.9, [dim("B")]: 1 });
    expect(result.changedAddresses).toEqual([dim("A")]);
  });

  it("does not report a change when the values agree, and needs two rows", () => {
    const rows = [look("a", 1, [[dim("A"), 1]]), look("b", 2, [[dim("A"), 1]])];
    expect(mergeRows(rows, ["a", "b"])!.changedAddresses).toEqual([]);
    expect(mergeRows(rows, ["a"])).toBeNull();
  });

  it("a merge that includes a manual row is a manual row", () => {
    const rows = [look("a", 1, [[dim("A"), 1]], { source: "manual" }), look("b", 2, [[dim("B"), 1]])];
    expect(mergeRows(rows, ["a", "b"])!.cues[0]!.source).toBeUndefined();
  });
});

describe("clusterMarkers", () => {
  const markers = Array.from({ length: 400 }, (_, i) => ({ id: `m${i}`, at: i * 0.7 }));
  it("clusters more at low zoom and splits at high zoom, never losing a marker", () => {
    const counts = [1, 6, 24, 120].map((px) => {
      const items = clusterMarkers(markers, px, 14);
      expect(items.reduce((n, it) => n + (it.kind === "single" ? 1 : it.markers.length), 0)).toBe(400);
      return items.length;
    });
    expect(counts[0]).toBeLessThan(counts[1]!);
    expect(counts[1]!).toBeLessThanOrEqual(counts[2]!);
    expect(counts[3]).toBe(400);
    expect(clusterMarkers(markers, 1, 14)[0]).toMatchObject({ kind: "cluster" });
  });
  it("keeps the selected marker single", () => {
    const items = clusterMarkers(markers, 1, 14, new Set(["m3"]));
    expect(items.some((it) => it.kind === "single" && it.marker.id === "m3")).toBe(true);
  });
});
