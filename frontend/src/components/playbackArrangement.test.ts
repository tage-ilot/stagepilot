import { describe, expect, it } from "vitest";

import { arrangeSongs, secondsApart } from "./playbackArrangement";

const PLAN = ["a", "b", "c", "d", "e"];
const ORDER = [900, 17, 42, 55, 70];
const run = (planSkips: string[] = [], playbackSkips: number[] = [], plan = PLAN, order = ORDER) =>
  arrangeSongs(plan, order.length, new Set(planSkips), new Set(playbackSkips), order);

describe("arrangeSongs", () => {
  it("pairs everything one-to-one when nothing is skipped", () => {
    expect(run().map((l) => l.kind)).toEqual(["pair", "pair", "pair", "pair", "pair"]);
  });

  it("handles the owner's example: plan song 4 has no track and Playback song 5 is background music", () => {
    expect(run(["d"], [70])).toEqual([
      { kind: "pair", plan: 0, playback: 0 },
      { kind: "pair", plan: 1, playback: 1 },
      { kind: "pair", plan: 2, playback: 2 },
      { kind: "plan-skip", plan: 3 },
      { kind: "pair", plan: 4, playback: 3 },
      { kind: "playback-skip", playback: 4 },
    ]);
  });

  it("a skipped song on one side shifts the other side's pairing and leaves a song alone at the end", () => {
    const lines = run(["a"], []);
    expect(lines[0]).toEqual({ kind: "plan-skip", plan: 0 });
    expect(lines.at(-1)).toEqual({ kind: "playback-alone", playback: 4 });
    expect(run([], [900]).at(-1)).toEqual({ kind: "plan-alone", plan: 4 });
  });

  it("never reorders or crosses a pair, for every combination of skips", () => {
    for (const planSkips of [[], ["a"], ["b", "d"], ["c"], ["a", "e"]]) {
      for (const playbackSkips of [[], [900], [17, 55], [70], [900, 70]]) {
        const pairs = run(planSkips, playbackSkips).filter((l) => l.kind === "pair") as { plan: number; playback: number }[];
        expect(pairs.map((p) => p.plan)).toEqual([...pairs.map((p) => p.plan)].sort((x, y) => x - y));
        expect(pairs.map((p) => p.playback)).toEqual([...pairs.map((p) => p.playback)].sort((x, y) => x - y));
        expect(new Set(pairs.map((p) => p.plan)).size).toBe(pairs.length);
      }
    }
  });

  it("every song on both sides appears exactly once", () => {
    const lines = run(["b"], [17, 70]);
    const plan = lines.flatMap((l) => ("plan" in l ? [l.plan] : [])).sort();
    const playback = lines.flatMap((l) => ("playback" in l ? [l.playback] : [])).sort();
    expect(plan).toEqual([0, 1, 2, 3, 4]);
    expect(playback).toEqual([0, 1, 2, 3, 4]);
  });

  it("copes with empty sides", () => {
    expect(run([], [], [], [])).toEqual([]);
    expect(run([], [], [], [1, 2]).map((l) => l.kind)).toEqual(["playback-alone", "playback-alone"]);
  });
});

describe("secondsApart", () => {
  it.each([[180, 170, 10], [180, 169.5, 10], [180, 180, 0], [100, 130.4, 30]])("%s vs %s", (plan, playback, expected) => {
    expect(secondsApart(plan, playback)).toBe(expected);
  });
  it("is null when either length is unknown", () => {
    expect(secondsApart(null, 10)).toBeNull();
    expect(secondsApart(10, null)).toBeNull();
    expect(secondsApart(10, undefined)).toBeNull();
  });
});
