import { describe, expect, it } from "vitest";

import type { LibraryPlan, LibraryPlanSong, LibrarySong } from "../types";
import { buildReadinessChecks, readinessWarnings } from "./dashboard/dashboardReadiness";
import { anyKeyWas, chipLibrary, formatLength, KEY_CHANGE_WINDOW_MS, keyWas, libraryNotices, libraryPillLine, songFaceFacts, songFacts } from "./playbackLibrary";

const TZ = "America/Los_Angeles";
const plan = { date: "2026-11-22", service_times: ["10:00"] };

const playback = (over: Partial<LibrarySong> = {}): LibrarySong => ({
  setlist_song_id: 1, position: 1, title: "Song One", key: "G", key_original: "G", key_was: null, key_changed_at: null, bpm: 72, scan_length_s: 300, countdown_length_s: 300, length_source: "last_measure",
  arrangement: "Default", section_count: 6, sections: [], artwork: null, rental_expires_utc: null, ...over,
});
const song = (title: string, position: number, library: Partial<LibrarySong> | null, over: Partial<LibraryPlanSong> = {}): LibraryPlanSong => ({
  key: `k-${title}`, title, plan_position: position, plan_seconds: 300, plan_key: null,
  status: library ? "matched" : "missing", confidence: library ? "exact" : null,
  library: library ? playback({ title, position, ...library }) : null, candidate: null, ...over,
});
const ready = (songs: LibraryPlanSong[], over: Partial<LibraryPlan> = {}): LibraryPlan => ({
  enabled: true, state: "ready", message: null, plan_date: "2026-11-22",
  setlist: { id: 1, title: "Set", date: "2026-11-22", songs: songs.length }, newest: null, songs, playback_only: [], chosen: false, setlists: [], stale: false, checked_at: null, ...over,
});
/** `hours` after the key was changed (2026-11-20 03:00 UTC), in ms. */
const CHANGED = Date.parse("2026-11-20T03:00:00Z");
const at = (hours: number) => CHANGED + hours * 3_600_000;
const texts = (library: LibraryPlan, p = plan) => libraryNotices(library, p, TZ).map((n) => n.text);

describe("the chip words", () => {
  it("shows key (and was X when it differs), BPM, length m:ss, sections and the arrangement", () => {
    const changed = { key: "Ab", key_was: "G", key_changed_at: "2026-11-20T03:00:00Z" };
    expect(songFacts(playback({ ...changed, bpm: 65, scan_length_s: 297.25, countdown_length_s: 297.25 }), at(1))).toEqual(["Ab (was G)", "65 BPM", "4:57", "6 sections", "Default"]);
    expect(songFacts(playback({ key: "G", key_original: "G", bpm: 72.4, section_count: 1, arrangement: null }), at(1))).toEqual(["G", "72.4 BPM", "5:00", "1 section"]);
    expect(formatLength(0)).toBe("0:00");
    expect(formatLength(61.4)).toBe("1:01");
    expect(formatLength(3599.6)).toBe("60:00");
  });

  it("shows the start of the last measure when there is one, never the end of the song next to it", () => {
    // (a) both known: the helper's scan length (388.64) is the countdown length; the end of the song (392.9) is not in the data this window reads.
    const chip = chipLibrary(song("Song One", 1, { scan_length_s: 388.64, countdown_length_s: 388.64, length_source: "last_measure" }, { plan_seconds: 389 }), at(1));
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
    const chip = chipLibrary(noGrid, at(1));
    expect(chip?.text).toContain("6:29");
    expect(chip?.text).not.toContain("bar lines");
    expect(chip?.title).toContain("Playback has no bar lines for this song, so this is where the song ends.");
    expect(songFacts(noGrid.library!, at(1))).toEqual(["G", "72 BPM", "6:29", "6 sections", "Default"]);
    expect(libraryNotices(ready([noGrid]), plan, TZ)).toEqual([]);
    // The same 15 s rule and wording as for a measured length.
    const far = song("Song One", 1, { scan_length_s: null, countdown_length_s: 388.64, length_source: "song_end_no_grid" }, { plan_seconds: 355 });
    expect(chipLibrary(far, at(1))?.text).toContain("6:29 (plan 5:55)");
    expect(texts(ready([far]))).toEqual(["Song One: Playback counts this song down to 6:29; the plan says 5:55."]);
  });

  it("explains in the chip's tooltip where the length comes from, and says nothing extra for the last measure", () => {
    const tip = (library: Partial<LibrarySong>) => chipLibrary(song("Song One", 1, library, { plan_seconds: 389 }), at(1))?.title ?? "";
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
    expect(chipLibrary(song("Song One", 1, { scan_length_s: 380, countdown_length_s: 392.9, length_source: "song_end_long_last_measure" }, { plan_seconds: 393 }), at(1))?.text).toBe("G · 72 BPM · 6:33 · 6 sections · Default");
  });

  it("says nothing about a length when the song has neither, and no notice", () => {
    // (c) both null.
    const missing = song("Song One", 1, { scan_length_s: null, countdown_length_s: null, length_source: null });
    expect(chipLibrary(missing, at(1))?.text).not.toMatch(/\d:\d\d/);
    expect(chipLibrary(missing, at(1))?.title).not.toContain("bar lines");
    expect(songFacts(missing.library!, at(1))).toEqual(["G", "72 BPM", "6 sections", "Default"]);
    expect(libraryNotices(ready([missing]), plan, TZ)).toEqual([]);
  });

  it("marks the plan's length next to Playback's when they are more than 15 s apart, and not otherwise", () => {
    expect(chipLibrary(song("Song One", 1, { countdown_length_s: 388.64 }, { plan_seconds: 355 }), at(1))?.text).toContain("6:29 (plan 5:55)");
    expect(chipLibrary(song("Song One", 1, { countdown_length_s: 315 }, { plan_seconds: 300 }), at(1))?.text).not.toContain("plan");
    expect(chipLibrary(song("Song One", 1, { countdown_length_s: 316 }, { plan_seconds: 300 }), at(1))?.text).toContain("(plan 5:00)");
    expect(chipLibrary(song("Song One", 1, null), at(1))).toBeNull();
    expect(chipLibrary(undefined, at(1))).toBeNull();
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

  it("asks for a look at an uncertain pair, naming both titles, and treats a close match as a match", () => {
    const uncertain = song("Way Maker", 1, null, { status: "uncertain", confidence: "uncertain", candidate: playback({ title: "Waymaker" }) });
    expect(texts(ready([uncertain]))).toEqual(["Way Maker may be “Waymaker” in Playback's setlist. Check that they are the same song."]);
    expect(libraryNotices(ready([uncertain]), plan, TZ)[0]).toMatchObject({ kind: "title", actionable: true });
    expect(texts(ready([song("So Much", 1, {}, { confidence: "close" })]))).toEqual([]);
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

describe("a key changed in Playback, said for a day", () => {
  const changed = (over: Partial<LibrarySong> = {}) => playback({ key: "Ab", key_original: "C", key_was: "G", key_changed_at: "2026-11-20T03:00:00Z", ...over });
  const minutes = (m: number) => CHANGED + m * 60_000;

  it("shows just the key when no change was seen, even when it differs from the original recording", () => {
    expect(songFaceFacts(playback({ key: "Ab", key_original: "C" }), at(0))).toEqual(["Ab", "72 BPM", "5:00"]);
    expect(keyWas(playback({ key: "Ab", key_original: "C" }), at(0))).toBeNull();
  });

  it("says was-X for 24 hours and not after: 0 h, 23 h 59 m, 24 h 1 m", () => {
    expect(songFaceFacts(changed(), at(0))[0]).toBe("Ab (was G)");
    expect(songFaceFacts(changed(), minutes(23 * 60 + 59))[0]).toBe("Ab (was G)");
    expect(songFaceFacts(changed(), minutes(24 * 60 + 1))[0]).toBe("Ab");
    expect(songFaceFacts(changed(), CHANGED + KEY_CHANGE_WINDOW_MS)[0]).toBe("Ab");
    expect(songFaceFacts(changed(), CHANGED + KEY_CHANGE_WINDOW_MS - 1)[0]).toBe("Ab (was G)");
  });

  it("says nothing when the old key is the key, when the time is missing or unreadable, or when there is no old key", () => {
    expect(songFaceFacts(changed({ key_was: "Ab" }), at(1))[0]).toBe("Ab");
    expect(songFaceFacts(changed({ key_changed_at: null }), at(1))[0]).toBe("Ab");
    expect(songFaceFacts(changed({ key_changed_at: "yesterday-ish" }), at(1))[0]).toBe("Ab");
    expect(songFaceFacts(changed({ key_was: null }), at(1))[0]).toBe("Ab");
    expect(songFaceFacts(changed({ key: null }), at(1))).toEqual(["72 BPM", "5:00"]);
  });

  it("puts it in words in the hover, only while it shows", () => {
    const one = song("Song One", 1, { key: "Ab", key_was: "G", key_changed_at: "2026-11-20T03:00:00Z" });
    expect(chipLibrary(one, at(5))?.title).toContain("Key changed in Playback from G to Ab in the last day");
    expect(chipLibrary(one, at(25))?.title).not.toContain("Key changed");
    expect(chipLibrary(one, at(25))?.text).toBe("Ab · 72 BPM · 5:00 · 6 sections · Default");
  });

  it("knows whether any card shows it, so the clock runs only then", () => {
    const withChange = ready([song("Song One", 1, { key_was: "F", key_changed_at: "2026-11-20T03:00:00Z" }), song("Song Two", 2, null)]);
    expect(anyKeyWas(withChange, at(1))).toBe(true);
    expect(anyKeyWas(withChange, at(25))).toBe(false);
    expect(anyKeyWas(ready([song("Song One", 1, {})]), at(1))).toBe(false);
    expect(anyKeyWas(null, at(1))).toBe(false);
    expect(anyKeyWas(ready([], { state: "problem" }), at(1))).toBe(false);
  });
});
