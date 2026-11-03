import { describe, expect, it } from "vitest";

import { clearAllRequest, clearLoudnessRequest, clearSongRequest, countCueMap } from "./lightingCueConfirmations";

const look = { source: "captured", messages: [{ address: "/a" }] };
const hand = { source: "manual", address: "/b" };

describe("clear request wording and counts", () => {
  it("counts captured looks and hand-made cues per method", () => {
    const map = { cues: [{}, {}], osc_cues: [look, look, hand], loudness: { values: [1] } } as never;
    expect(countCueMap(map, "osc")).toEqual({ looks: 2, handmade: 1, loudness: 1 });
    expect(countCueMap(map, "midi")).toEqual({ looks: 0, handmade: 2, loudness: 1 });
    expect(countCueMap(undefined, "osc")).toEqual({ looks: 0, handmade: 0, loudness: 0 });
  });

  it("song request names the song; all request names the plan or the single song", () => {
    const onErase = () => undefined;
    const counts = { looks: 1, handmade: 2, loudness: 0 };
    expect(clearSongRequest("Holy Forever", counts, onErase)).toMatchObject({ kind: "song", subject: "Holy Forever", looks: 1, handmade: 2, loudness: 0 });
    expect(clearAllRequest(["A", "B"], "Sunday", counts, onErase).subject).toBe('every song in "Sunday"');
    expect(clearAllRequest(["A", "B"], null, counts, onErase).subject).toBe('every song in "the current service plan"');
    expect(clearAllRequest(["Holy Forever"], "Sunday", counts, onErase).subject).toBe("Holy Forever");
    expect(clearLoudnessRequest("Holy Forever", onErase)).toMatchObject({ kind: "loudness", looks: 0, handmade: 0, loudness: 1 });
  });
});
