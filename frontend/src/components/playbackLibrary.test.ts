import { describe, expect, it } from "vitest";

import type { LibraryPlan, LibraryPlanSong, LibrarySong } from "../types";
import { buildReadinessChecks, readinessWarnings } from "./dashboard/dashboardReadiness";
import { chipLibrary, formatLength, libraryNotices, libraryPillLine, songFacts } from "./playbackLibrary";

const TZ = "America/Los_Angeles";
const plan = { date: "2026-11-22", service_times: ["10:00"] };

const playback = (over: Partial<LibrarySong> = {}): LibrarySong => ({
  setlist_song_id: 1, position: 1, title: "Song One", key: "G", key_original: "G", bpm: 72, scan_length_s: 300, countdown_length_s: 300, length_source: "last_measure",
  arrangement: "Default", sections: 6, rental_expires_utc: null, ...over,
});
const song = (title: string, position: number, library: Partial<LibrarySong> | null, over: Partial<LibraryPlanSong> = {}): LibraryPlanSong => ({
  key: `k-${title}`, title, plan_position: position, plan_seconds: 300,
  status: library ? "matched" : "missing",
  library: library ? playback({ title, position, ...library }) : null, ...over,
});
const ready = (songs: LibraryPlanSong[], over: Partial<LibraryPlan> = {}): LibraryPlan => ({
  enabled: true, state: "ready", message: null, plan_date: "2026-11-22",
  setlist: { title: "Set", date: "2026-11-22", songs: songs.length }, newest: null, songs, stale: false, checked_at: null, ...over,
});
const texts = (library: LibraryPlan, p = plan) => libraryNotices(library, p, TZ).map((n) => n.text);

describe("the chip words", () => {
  it("shows key (and was X when it differs), BPM, length m:ss, sections and the arrangement", () => {
    expect(songFacts(playback({ key: "Ab", key_original: "C", bpm: 65, scan_length_s: 297.25, countdown_length_s: 297.25 }))).toEqual(["Ab (was C)", "65 BPM", "4:57", "6 sections", "Default"]);
    expect(songFacts(playback({ key: "G", key_original: "G", bpm: 72.4, sections: 1, arrangement: null }))).toEqual(["G", "72.4 BPM", "5:00", "1 section"]);
    expect(formatLength(0)).toBe("0:00");
    expect(formatLength(61.4)).toBe("1:01");
    expect(formatLength(3599.6)).toBe("60:00");
  });

  it("shows the start of the last measure when there is one, never the end of the song next to it", () => {
    // (a) both known: the helper's scan length (388.64) is the countdown length; the end of the song (392.9) is not in the data this window reads.
    const chip = chipLibrary(song("Song One", 1, { scan_length_s: 388.64, countdown_length_s: 388.64, length_source: "last_measure" }, { plan_seconds: 389 }));
    expect(chip?.text).toContain("6:29");
    expect(chip?.text).not.toContain("6:33");
    expect(chip?.title).not.toContain("no bar lines");
    expect(Object.keys(playback())).not.toContain("length_s");
    expect(libraryNotices(ready([song("Song One", 1, { countdown_length_s: 388.64 }, { plan_seconds: 389 })]), plan, TZ)).toEqual([]);
    const far = libraryNotices(ready([song("Song One", 1, { countdown_length_s: 388.64 }, { plan_seconds: 355 })]), plan, TZ);
    expect(far.map((n) => n.text)).toEqual(["Song One: Playback counts this song down to 6:29; the plan says 5:55."]);
  });

  it("for a song with no beat grid uses the end of the song, and says so in the chip's tooltip only", () => {
    // (b) scan_length_s null, the backend resolved the end of the song (388.64 here, from the helper's length_s).
    const noGrid = song("Song One", 1, { scan_length_s: null, countdown_length_s: 388.64, length_source: "song_end_no_grid" }, { plan_seconds: 389 });
    const chip = chipLibrary(noGrid);
    expect(chip?.text).toContain("6:29");
    expect(chip?.text).not.toContain("bar lines");
    expect(chip?.title).toContain("Playback has no bar lines for this song, so this is where the song ends.");
    expect(songFacts(noGrid.library!)).toEqual(["G", "72 BPM", "6:29", "6 sections", "Default"]);
    expect(libraryNotices(ready([noGrid]), plan, TZ)).toEqual([]);
    // The same 15 s rule and wording as for a measured length.
    const far = song("Song One", 1, { scan_length_s: null, countdown_length_s: 388.64, length_source: "song_end_no_grid" }, { plan_seconds: 355 });
    expect(chipLibrary(far)?.text).toContain("6:29 (plan 5:55)");
    expect(texts(ready([far]))).toEqual(["Song One: Playback counts this song down to 6:29; the plan says 5:55."]);
  });

  it("explains in the chip's tooltip where the length comes from, and says nothing extra for the last measure", () => {
    const tip = (library: Partial<LibrarySong>) => chipLibrary(song("Song One", 1, library, { plan_seconds: 389 }))?.title ?? "";
    const noGrid = "Playback has no bar lines for this song, so this is where the song ends.";
    const longLast = "This song's last measure is longer than 6 seconds, so this is where the song ends.";
    const measure = tip({ scan_length_s: 388.64, countdown_length_s: 388.64, length_source: "last_measure" });
    expect(measure).not.toContain("no bar lines");
    expect(measure).not.toContain("longer than 6 seconds");
    const grid = tip({ scan_length_s: null, countdown_length_s: 392.9, length_source: "song_end_no_grid" });
    expect(grid).toContain(noGrid);
    expect(grid).not.toContain(longLast);
    const long = tip({ scan_length_s: 380, countdown_length_s: 392.9, length_source: "song_end_long_last_measure" });
    expect(long).toContain(longLast);
    expect(long).not.toContain(noGrid);
    // The chip's visible words are the same for all three: only the number differs.
    expect(chipLibrary(song("Song One", 1, { scan_length_s: 380, countdown_length_s: 392.9, length_source: "song_end_long_last_measure" }, { plan_seconds: 393 }))?.text).toBe("G · 72 BPM · 6:33 · 6 sections · Default");
  });

  it("says nothing about a length when the song has neither, and no notice", () => {
    // (c) both null.
    const missing = song("Song One", 1, { scan_length_s: null, countdown_length_s: null, length_source: null });
    expect(chipLibrary(missing)?.text).not.toMatch(/\d:\d\d/);
    expect(chipLibrary(missing)?.title).not.toContain("bar lines");
    expect(songFacts(missing.library!)).toEqual(["G", "72 BPM", "6 sections", "Default"]);
    expect(libraryNotices(ready([missing]), plan, TZ)).toEqual([]);
  });

  it("marks the plan's length next to Playback's when they are more than 15 s apart, and not otherwise", () => {
    expect(chipLibrary(song("Song One", 1, { countdown_length_s: 388.64 }, { plan_seconds: 355 }))?.text).toContain("6:29 (plan 5:55)");
    expect(chipLibrary(song("Song One", 1, { countdown_length_s: 315 }, { plan_seconds: 300 }))?.text).not.toContain("plan");
    expect(chipLibrary(song("Song One", 1, { countdown_length_s: 316 }, { plan_seconds: 300 }))?.text).toContain("(plan 5:00)");
    expect(chipLibrary(song("Song One", 1, null))).toBeNull();
    expect(chipLibrary(undefined)).toBeNull();
  });
});

describe("the notices", () => {
  it("has nothing to say when everything agrees", () => {
    const agree = ready([song("Song One", 1, {}), song("Song Two", 2, {}), song("Song Three", 3, { countdown_length_s: 310 })]);
    expect(libraryNotices(agree, plan, TZ)).toEqual([]);
    expect(libraryPillLine(libraryNotices(agree, plan, TZ))).toBeNull();
  });

  it("is silent while the library is off, not ready or has nothing", () => {
    expect(libraryNotices(null, plan, TZ)).toEqual([]);
    expect(libraryNotices(ready([song("Song One", 1, null)], { enabled: false }), plan, TZ)).toEqual([]);
    for (const state of ["off", "waiting", "problem"] as const) expect(libraryNotices(ready([song("Song One", 1, null)], { state }), plan, TZ)).toEqual([]);
    expect(libraryNotices(ready([]), plan, TZ)).toEqual([]);
  });

  it("names a plan song that is missing from Playback", () => {
    expect(texts(ready([song("Song One", 1, {}), song("Song Two", 2, null)]))).toEqual(["Song Two is not in Playback's setlist."]);
  });

  it("names both positions when the order differs, and does not blame the songs after a missing one", () => {
    const moved = ready([song("Song One", 1, { position: 2 }), song("Song Two", 2, { position: 1 })]);
    expect(texts(moved)).toEqual([
      "Song One is song 1 in the plan but song 2 in Playback.",
      "Song Two is song 2 in the plan but song 1 in Playback.",
    ]);
    // Song One is missing from Playback, so Song Two sits at 2 in the plan and 1 in Playback: not a different order.
    const shifted = ready([song("Song One", 1, null), song("Song Two", 2, { position: 1 }), song("Song Three", 3, { position: 2 })]);
    expect(texts(shifted)).toEqual(["Song One is not in Playback's setlist."]);
  });

  it("says Playback's length when it is more than 15 s from the plan's, as information only", () => {
    const longer = ready([song("Song One", 1, { countdown_length_s: 388.64 }, { plan_seconds: 355 })]);
    const notices = libraryNotices(longer, plan, TZ);
    expect(notices.map((n) => n.text)).toEqual(["Song One: Playback counts this song down to 6:29; the plan says 5:55."]);
    expect(notices[0]).toMatchObject({ kind: "length", actionable: false });
    expect(libraryPillLine(notices)).toBeNull();
    expect(texts(ready([song("Song One", 1, { countdown_length_s: 315 }, { plan_seconds: 300 })]))).toEqual([]);
    expect(texts(ready([song("Song One", 1, { countdown_length_s: 300 }, { plan_seconds: null })]))).toEqual([]);
  });

  it("warns about a rental that ends before the service starts, using the service time when known", () => {
    // 10:00 in Los Angeles on 2026-11-22 is 18:00 UTC.
    const ends = (at: string) => ready([song("Song Four", 1, { rental_expires_utc: at })]);
    expect(texts(ends("2026-11-22T17:59:00Z"))).toEqual(["Song Four is a rental in Playback that ends before the service starts."]);
    expect(texts(ends("2026-11-22T18:01:00Z"))).toEqual([]);
    expect(texts(ends("2026-11-30T00:00:00Z"))).toEqual([]);
    // With no service time known, the end of the plan's day counts (23:59 in Los Angeles is 07:59 UTC the next day).
    const noTime = { date: "2026-11-22", service_times: [] as string[] };
    expect(texts(ends("2026-11-22T20:00:00Z"), noTime)).toEqual(["Song Four is a rental in Playback that ends before the end of 2026-11-22."]);
    expect(texts(ends("2026-11-23T09:00:00Z"), noTime)).toEqual([]);
    // Several services: the last one decides.
    expect(texts(ends("2026-11-22T18:59:00Z"), { date: "2026-11-22", service_times: ["09:00", "11:00"] })).toHaveLength(1);
  });

  it("gives Playback's own sentence when it has no setlist for the plan's date", () => {
    const message = "Playback has no setlist for 2026-11-29; its newest is Newest Set from 2026-11-15.";
    const none = ready([], { state: "no_setlist", message, setlist: null });
    expect(libraryNotices(none, plan, TZ)).toEqual([{ id: "date", kind: "date", actionable: true, text: message }]);
  });
});

describe("the one line in the header pill", () => {
  const pill = (library: LibraryPlan) => libraryPillLine(libraryNotices(library, plan, TZ));

  it("is one line with the actionable notices, and leaves the length difference out", () => {
    const line = pill(ready([song("Song One", 1, { countdown_length_s: 388.64 }, { plan_seconds: 355 }), song("Song Two", 2, null)]));
    expect(line?.label).toBe("Playback library: Song Two is not in Playback's setlist");
    expect(line?.detail).toBe("Song Two is not in Playback's setlist.");
  });

  it("counts them when they do not fit on one line, keeping every sentence in the detail", () => {
    const line = pill(ready([song("Song One", 1, null), song("Song Two", 2, null), song("Song Three", 3, null)]));
    expect(line?.label).toBe("Playback library: 3 things to check");
    expect(line?.detail).toContain("Song Three is not in Playback's setlist.");
  });

  it("is a warning in the readiness list that never blocks Ready", () => {
    const line = pill(ready([song("Song Two", 1, null)]));
    const base = { state: { plan: null, timer: { status: "idle" }, service_load: { status: "idle" }, plugins: {} } as never, settings: null, propresenter: null, live: true, views: {} as never };
    const checks = buildReadinessChecks({ ...base, views: { planningCenter: { status: "connected", detail: "", configured: true, mode: "real" }, midi: { status: "connected", detail: "", configured: true, mode: "real" }, propresenter: { status: "connected", detail: "", configured: true, mode: "real" }, lights: { status: "connected", detail: "", configured: true, mode: "real" } }, libraryLine: line });
    const found = checks.find((check) => check.id === "playback-library");
    expect(found).toMatchObject({ required: false, passed: false, severity: "warning", label: "Playback library: Song Two is not in Playback's setlist" });
    expect(readinessWarnings(checks).map((check) => check.id)).toContain("playback-library");
    expect(buildReadinessChecks({ ...base, views: { planningCenter: { status: "connected", detail: "", configured: true, mode: "real" }, midi: { status: "connected", detail: "", configured: true, mode: "real" }, propresenter: { status: "connected", detail: "", configured: true, mode: "real" }, lights: { status: "connected", detail: "", configured: true, mode: "real" } }, libraryLine: null }).some((check) => check.id === "playback-library")).toBe(false);
  });
});
