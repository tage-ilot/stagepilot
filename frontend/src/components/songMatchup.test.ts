import { describe, expect, it } from "vitest";

import { libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import type { LibraryPlan, LibraryPlanSong, LibrarySong } from "../types";
import {
  classicRows, clock, countStamps, joinScan, keyDifference, lengthWords, libraryMatchesPlan, libraryRows, longestRising, sameKey, setlistSongs, STAMP_WORDS,
  type MatchupInput,
} from "./songMatchup";

const planOf = (titles: string[]) => titles.map((title, i) => ({ id: `p${i + 1}`, title, duration_seconds: 389 }));
const song = (title: string, position: number, over: Partial<LibrarySong> = {}) => librarySong({ title, position, setlist_song_id: 2000000 + position, countdown_length_s: 389, ...over });
const paired = (title: string, planPosition: number, partner: LibrarySong, over: Partial<LibraryPlanSong> = {}) =>
  librarySongOf(title, planPosition, { library: partner, ...over });
const input = (titles: string[], over: Partial<MatchupInput> = {}): MatchupInput => ({
  plan: planOf(titles), order: [], lengths: [], planSkips: new Set(), playbackSkips: new Set(), ...over,
});

describe("keys", () => {
  it("treats the same note spelt two ways as the same key, and a minor key as another", () => {
    expect(sameKey("A#", "Bb")).toBe(true);
    expect(sameKey("G♯m", "Abm")).toBe(true);
    expect(sameKey("Db", "C#")).toBe(true);
    expect(sameKey("A", "Am")).toBe(false);
    expect(sameKey("A", "Ab")).toBe(false);
    expect(sameKey("bb minor", "A#m")).toBe(true);
    expect(sameKey("Dsus", "dsus")).toBe(true);
  });

  it("says the difference in plain words, only when both keys are known and differ", () => {
    expect(keyDifference("A", "Ab")).toBe("Plan says key A, Playback plays Ab.");
    expect(keyDifference("A#", "Bb")).toBeNull();
    expect(keyDifference(null, "Ab")).toBeNull();
    expect(keyDifference("A", null)).toBeNull();
  });
});

describe("length words", () => {
  it("uses the existing wording over 15 s and nothing within it", () => {
    const far = paired("Song", 1, song("Song", 1, { countdown_length_s: 442 }), { plan_seconds: 408 });
    expect(lengthWords(far)).toBe("Playback counts this song down to 7:22; the plan says 6:48.");
    expect(lengthWords(paired("Song", 1, song("Song", 1, { countdown_length_s: 400 }), { plan_seconds: 389 }))).toBeNull();
    expect(lengthWords(paired("Song", 1, song("Song", 1, { countdown_length_s: 404 }), { plan_seconds: 389 }))).toBeNull();
    expect(lengthWords(paired("Song", 1, song("Song", 1, { countdown_length_s: 405 }), { plan_seconds: 389 }))).toContain("Playback counts this song down to 6:45");
    expect(clock(null)).toBe("Unknown");
    expect(clock(442)).toBe("7:22");
  });
});

describe("the scan and the library", () => {
  const songs = [song("A", 1), song("B", 2), song("C", 3)];
  it("joins by Playback's own song numbers when they are the same set, in the scan's order", () => {
    const join = joinScan([2000003, 2000001, 2000002], songs)!;
    expect(join.by).toBe("id");
    expect(join.of.map((s) => s?.title)).toEqual(["C", "A", "B"]);
  });

  it("joins by place only when the scan shares no number with the library and has as many songs", () => {
    const join = joinScan([17, 42, 55], songs)!;
    expect(join.by).toBe("position");
    expect(join.of.map((s) => s?.title)).toEqual(["A", "B", "C"]);
  });

  it("does not join a scan of another setlist: some numbers shared, a different count, or none to join", () => {
    expect(joinScan([2000001, 42, 55], songs)).toBeNull();
    expect(joinScan([2000001, 2000002], songs)).toBeNull();
    expect(joinScan([17, 42, 55, 70], songs)).toBeNull();
    expect(joinScan([], songs)).toBeNull();
    expect(joinScan([2000001, 2000001, 2000002], songs)).toBeNull();
  });

  it("uses the library only when it is about the plan on screen", () => {
    const plan = planOf(["A", "B"]);
    const ready = libraryPlan([librarySongOf("A", 1), librarySongOf("B", 2)]);
    expect(libraryMatchesPlan(ready, plan)).toBe(true);
    expect(libraryMatchesPlan(ready, planOf(["A", "Other"]))).toBe(false);
    expect(libraryMatchesPlan(ready, planOf(["A"]))).toBe(false);
    expect(libraryMatchesPlan({ ...ready, enabled: false }, plan)).toBe(false);
    expect(libraryMatchesPlan({ ...ready, state: "no_setlist" }, plan)).toBe(false);
    expect(libraryMatchesPlan({ ...ready, setlist: null }, plan)).toBe(false);
    expect(libraryMatchesPlan(null, plan)).toBe(false);
  });

  it("finds one longest run of songs in the same order on both sides", () => {
    expect([...longestRising([2, 1, 3, 4])].sort()).toEqual([0, 2, 3]);
    expect([...longestRising([1, 2, 3])].sort()).toEqual([0, 1, 2]);
    expect([...longestRising([3, 2, 1])].length).toBe(1);
    expect([...longestRising([])]).toEqual([]);
  });
});

describe("the scan-only rows (no library)", () => {
  const ORDER = [900, 17, 42, 55, 70];
  const TITLES = ["One Thing Remains", "So Much", "Easy To Worship", "I Sing Praises", "Great Are You Lord"];
  const plan = TITLES.map((title, i) => ({ id: `p${i + 1}`, title, duration_seconds: [443, 309, 539, 150, 295][i]! }));
  const base = (over: Partial<MatchupInput> = {}): MatchupInput => ({ plan, order: ORDER, lengths: [438, 312, 535, 298, 606], planSkips: new Set(), playbackSkips: new Set(), ...over });

  it("pairs the Nth with the Nth and says how far apart the lengths are, with a stamp per row", () => {
    const rows = classicRows(base());
    expect(rows.map((r) => r.kind)).toEqual(["pair", "pair", "pair", "pair", "pair"]);
    expect(rows.map((r) => r.stamp)).toEqual(["matched", "matched", "matched", "check-length", "check-length"]);
    expect(rows[0]!.notes).toEqual(["5 s apart"]);
    expect(rows[3]!.notes).toEqual(["148 s apart. Probably a different song."]);
    expect(rows[0]!.playback).toMatchObject({ number: 1, title: null, skipId: 900 });
  });

  it("the owner's example: skipping plan song 4 and Playback song 5 lines everything up", () => {
    const rows = classicRows(base({ planSkips: new Set(["p4"]), playbackSkips: new Set([70]) }));
    expect(rows.map((r) => `${r.kind}:${r.stamp}`)).toEqual(["pair:matched", "pair:matched", "pair:matched", "plan-only:skipped", "pair:matched", "playback-only:skipped"]);
    expect(rows[4]!.plan!.title).toBe("Great Are You Lord");
    expect(rows[4]!.playback!.number).toBe(4);
    expect(rows[5]!.sentence).toBe("Playback song 5 has no plan song. Skipped.");
    expect(countStamps(rows).skipped).toBe(2);
  });

  it("a song with nothing left to pair with says so", () => {
    const rows = classicRows(base({ planSkips: new Set(["p1"]) }));
    expect(rows[0]!.stamp).toBe("skipped");
    expect(rows.at(-1)).toMatchObject({ kind: "playback-only", stamp: "only-playback", notes: ["No service song left to pair with."] });
    expect(classicRows(base({ playbackSkips: new Set([900]) })).at(-1)).toMatchObject({ kind: "plan-only", stamp: "only-plan", notes: ["No Playback song left to pair with."] });
  });

  it("reads each row as one full sentence", () => {
    const rows = classicRows(base());
    expect(rows[0]!.sentence).toBe("Plan song 1, One Thing Remains goes with Playback song 1. Matched. 5 s apart");
    expect(rows[3]!.sentence).toContain("Check length.");
    expect(Object.values(STAMP_WORDS)).toEqual(["Matched", "Check key", "Check length", "Check title", "Check order", "Only in plan", "Only in Playback", "Skipped"]);
  });
});

describe("the library rows", () => {
  // Plan: four songs. Playback: So Much first, then One Thing Remains, Easy To Worship (a key lower), Waymaker, and a song only it has.
  const TITLES = ["One Thing Remains", "So Much", "Easy To Worship", "Way Maker"];
  const soMuch = song("So Much", 1);
  const remains = song("One Thing Remains", 2);
  const easy = song("Easy To Worship", 3, { key: "Ab" });
  const waymaker = song("Waymaker", 4);
  const background = song("Background Music", 5, { setlist_song_id: 2000005 });
  const library = (): LibraryPlan => libraryPlan([
    paired("One Thing Remains", 1, remains),
    paired("So Much (Live)", 2, soMuch, { confidence: "close" }),
    paired("Easy To Worship", 3, easy, { plan_key: "A" }),
    librarySongOf("Way Maker", 4, { status: "uncertain", confidence: "uncertain", library: null, candidate: waymaker }),
  ], { playback_only: [background] });
  const ids = [1, 2, 3, 4, 5].map((n) => 2000000 + n);

  it("is one row per plan song and per Playback song, plan order first, Playback-only songs where Playback has them", () => {
    const view = libraryRows(input(TITLES), library(), false)!;
    expect(view.live).toBe(false);
    expect(view.rows.map((r) => r.kind)).toEqual(["pair", "pair", "pair", "pair", "playback-only"]);
    expect(view.rows.map((r) => r.id)).toEqual(["pair-0-2", "pair-1-1", "pair-2-3", "pair-3-4", "playback-only-5"]);
    expect(setlistSongs(library()).map((s) => s.position)).toEqual([1, 2, 3, 4, 5]);
  });

  it("puts a Playback-only song among the pairs where Playback has it", () => {
    const lib = libraryPlan([paired("A", 1, song("A", 1)), paired("B", 2, song("B", 3))], { playback_only: [song("Extra", 2)] });
    const view = libraryRows(input(["A", "B"]), lib, false)!;
    expect(view.rows.map((r) => r.playback!.title)).toEqual(["A", "Extra", "B"]);
    expect(view.rows.map((r) => r.kind)).toEqual(["pair", "playback-only", "pair"]);
  });

  it("states the confidence and never silently accepts an uncertain pair", () => {
    const rows = libraryRows(input(TITLES), library(), false)!.rows;
    expect(rows.map((r) => r.confidence)).toEqual(["exact", "close", "exact", "uncertain", null]);
    expect(rows[1]!.notes[0]).toBe("Close match: the plan says “So Much”, Playback has “So Much”.");
    expect(rows[3]).toMatchObject({ stamp: "check-title", playback: { title: "Waymaker", cover: false } });
    expect(rows[3]!.notes[0]).toContain("The titles are alike but not the same: the plan says “Way Maker”, Playback has “Waymaker”.");
    expect(rows[0]!.playback!.cover).toBe(true);
  });

  it("says a difference in key and in order in plain words, and only the song that moved", () => {
    const rows = libraryRows(input(TITLES), library(), false)!.rows;
    expect(rows[2]).toMatchObject({ stamp: "check-key", notes: ["Plan says key A, Playback plays Ab."] });
    expect(rows[1]!.stamp).toBe("check-order");
    expect(rows[1]!.notes).toContain("This is song 2 in the plan but song 1 in Playback.");
    expect(rows[0]!.stamp).toBe("matched");
    expect(rows[4]).toMatchObject({ stamp: "only-playback", notes: ["Playback has this song; the plan does not."] });
  });

  it("says a length difference with the existing wording", () => {
    const lib = libraryPlan([paired("Song", 1, song("Song", 1, { countdown_length_s: 442 }), { plan_seconds: 408 })]);
    const row = libraryRows(input(["Song"]), lib, false)!.rows[0]!;
    expect(row.stamp).toBe("check-length");
    expect(row.notes).toEqual(["Playback counts this song down to 7:22; the plan says 6:48."]);
    expect(row.sentence).toBe("Plan song 1, Song goes with Playback song 1, Song. Check length. Playback counts this song down to 7:22; the plan says 6:48.");
  });

  it("a song only in the plan, or only in Playback, is its own row kind, with the reason", () => {
    const lib = libraryPlan([
      librarySongOf("Gone", 1, { status: "missing", confidence: null, library: null }),
      librarySongOf("Twice", 2, { status: "duplicate_in_plan", confidence: null, library: null }),
      librarySongOf("Doubled", 3, { status: "duplicate_in_playback", confidence: null, library: null }),
    ], { playback_only: [song("Extra", 1)] });
    const rows = libraryRows(input(["Gone", "Twice", "Doubled"]), lib, false)!.rows;
    expect(rows.map((r) => `${r.kind}:${r.stamp}`)).toEqual(["plan-only:only-plan", "plan-only:only-plan", "plan-only:only-plan", "playback-only:only-playback"]);
    expect(rows[0]!.notes).toEqual(["Playback's setlist has no song with this title."]);
    expect(rows[1]!.notes[0]).toContain("in the plan more than once");
    expect(rows[2]!.notes[0]).toContain("Playback's setlist has this title more than once");
  });

  it("with the scan joined, says which Playback song will really start each timer and lets a skip fix it", () => {
    const plan = ["A", "B", "C"];
    const a = song("A", 1);
    const extra = song("Extra", 2);
    const b = song("B", 3);
    const c = song("C", 4);
    const lib = libraryPlan([paired("A", 1, a), paired("B", 2, b), paired("C", 3, c)], { playback_only: [extra] });
    const order = [2000001, 2000002, 2000003, 2000004];
    const without = libraryRows(input(plan, { order }), lib, true)!;
    expect(without.live).toBe(true);
    expect(without.join).toBe("id");
    expect(without.rows.map((r) => r.stamp)).toEqual(["matched", "only-playback", "check-order", "check-order"]);
    expect(without.rows[2]!.notes[0]).toBe("With the skips as they are, Playback song 2, Extra, will start this plan song's timer, not B.");
    expect(without.rows[1]!.notes).toContain("Playback song 2 will start the timer of plan song 2, B, unless you skip it.");
    expect(without.rows[1]!.playback).toMatchObject({ skipId: 2000002, skipped: false });

    const fixed = libraryRows(input(plan, { order, playbackSkips: new Set([2000002]) }), lib, true)!;
    expect(fixed.rows.map((r) => r.stamp)).toEqual(["matched", "skipped", "matched", "matched"]);
    expect(fixed.rows[1]!.playback!.skipped).toBe(true);
  });

  it("a skipped plan song is a skipped row, and the Playback song that was its partner then lines up with the next one", () => {
    const lib = libraryPlan([paired("A", 1, song("A", 1)), paired("B", 2, song("B", 2))]);
    const rows = libraryRows(input(["A", "B"], { order: [2000001, 2000002], planSkips: new Set(["p1"]) }), lib, true)!.rows;
    expect(rows[0]).toMatchObject({ stamp: "skipped", plan: { skipped: true } });
    expect(rows[1]!.stamp).toBe("check-order");
    expect(rows[1]!.notes[0]).toBe("With the skips as they are, Playback song 1, A, will start this plan song's timer, not B.");
  });

  it("gives no skip ids and no runtime claims without a scan, and ties by place when the numbers cannot be checked", () => {
    const lib = libraryPlan([paired("A", 1, song("A", 1))]);
    const readonly = libraryRows(input(["A"]), lib, false)!;
    expect(readonly.join).toBeNull();
    expect(readonly.rows[0]!.playback!.skipId).toBeNull();
    const byPlace = libraryRows(input(["A"], { order: [17] }), lib, true)!;
    expect(byPlace).toMatchObject({ live: true, join: "position" });
    expect(byPlace.rows[0]!.playback!.skipId).toBe(17);
  });

  it("returns nothing for a scan that is not of this setlist, so the caller shows the plain scan view", () => {
    const lib = libraryPlan([paired("A", 1, song("A", 1))]);
    expect(libraryRows(input(["A"], { order: [2000001, 5, 6] }), lib, true)).toBeNull();
  });

  it("counts the stamps for the summary", () => {
    const counts = countStamps(libraryRows(input(TITLES), library(), false)!.rows);
    expect(counts).toMatchObject({ matched: 1, "check-order": 1, "check-key": 1, "check-title": 1, "only-playback": 1 });
    expect(ids.length).toBe(5);
  });
});
