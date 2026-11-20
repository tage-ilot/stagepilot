// Made-up data for the Presenter Link states of the section gallery: no real computer, playlist or song. No backend is asked: the section
// is given a `PresenterApi` that answers from here, so every state is reached by pressing the real buttons.
import type { PresenterApi } from "../hooks/presenterApi";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { AddSongsJob, AddSongsPlan, AddSongsReport, AddSongsSong, ProPresenterPlaylist, TitleFound } from "../propresenterLink";
import type { LibraryPlan, LibraryPlanSong, LibrarySong, Song } from "../types";
import { libraryController, libraryOf } from "./fixtures";

export const PLAYLISTS: ProPresenterPlaylist[] = [
  { id: "PL-SUNDAY", name: "Sunday Morning Service", folder: null, headers: ["Welcome", "Worship", "Message", "Closing"] },
  { id: "PL-TWICE", name: "Easter (two Worship headers)", folder: null, headers: ["Welcome", "Worship", "Message", "Worship"] },
  { id: "PL-WED", name: "Wednesday Night", folder: null, headers: [] },
  { id: "PL-YOUTH", name: "Youth Night", folder: "Special services", headers: ["Games", "Worship"] },
];

// ---- the loaded plan: the Planning Center names ------------------------------------------------------------------------------
const NAMES = ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"];
const song = (title: string, order: number): Song => ({ id: `pc-${order}`, title, duration_seconds: 300, order, is_generic: false, source_song_id: null });
export const PLAN_NAMES = { songs: NAMES.map(song) };
/** A plan whose names ProPresenter has, except the last one. */
export const NAMES_IN_PROPRESENTER: Record<string, TitleFound> = { "Holy Forever": "found", "Battle Belongs": "found", "Gratitude": "found", "Firm Foundation": "missing" };

// ---- Playback's setlist: the same songs, with id, key and arrangement --------------------------------------------------------
const SET: { title: string; key: string; arrangement: string; id: number; sections: string[] }[] = [
  { title: "Holy Forever", key: "G", arrangement: "Radio", id: 5_000_101, sections: ["Count Off", "Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge", "Chorus"] },
  { title: "Battle Belongs", key: "Ab", arrangement: "Long ending", id: 5_000_102, sections: ["Count Off", "Intro", "Verse 1", "Chorus", "Interlude", "Verse 2", "Chorus", "Outro"] },
  { title: "Gratitude", key: "A", arrangement: "Default", id: 5_000_103, sections: ["Count Off", "Verse 1", "Chorus", "Verse 2", "Chorus"] },
  { title: "Firm Foundation", key: "E", arrangement: "Default", id: 5_000_104, sections: ["Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge"] },
];
const librarySong = (item: (typeof SET)[number], index: number): LibrarySong => ({
  setlist_song_id: 2_000_001 + index, content_id: item.id, position: index + 1, title: item.title, key: item.key, key_original: item.key,
  // The first song's key was changed in Playback an hour ago: its card says "(was F)".
  key_was: index === 0 ? "F" : null, key_changed_at: index === 0 ? new Date(Date.now() - 3_600_000).toISOString() : null, bpm: 72,
  scan_length_s: 300, countdown_length_s: 300, length_source: "last_measure", arrangement: item.arrangement, section_count: item.sections.length,
  sections: item.sections.map((name, i) => ({ name, start: i * 20, end: i * 20 + 19 })), artwork: null, rental_expires_utc: null,
});
const planSong = (item: (typeof SET)[number], index: number): LibraryPlanSong => ({
  key: `k${index + 1}`, title: item.title, plan_position: index + 1, plan_seconds: 300, plan_key: null, status: "matched", confidence: "exact", library: librarySong(item, index), candidate: null,
});
export const SETLIST: LibraryPlan = {
  enabled: true, state: "ready", message: null, plan_date: "2026-11-22", setlist: { id: 1, title: "Sunday set", date: "2026-11-22", songs: SET.length },
  newest: null, stale: false, checked_at: null, songs: SET.map(planSong), playback_only: [], chosen: false, setlists: [],
};
/** The Library Link with Playback's songs, or without any data (not installed). */
export const withPlayback = (): PlaybackLibraryController => ({ ...libraryController(libraryOf("ready")), plan: SETLIST });
export const withoutPlayback = (): PlaybackLibraryController => libraryController(libraryOf("not_installed"));

// ---- what the routes answer --------------------------------------------------------------------------------------------------
const row = (over: Partial<AddSongsSong> & Pick<AddSongsSong, "ref_id" | "title">): AddSongsSong => ({
  position: 1, content_id: null, action: "copy", file: `${over.title} (1)`, source: `Songs / ${over.title}.pro`, import_as: null, reason: null, choices: [], notes: [], mapping: [], guesses: [], sections: 7, key: null, already_in_playlist: false, ...over,
});
const planOf = (songs: AddSongsSong[]): AddSongsPlan => {
  const stuck = songs.filter((s) => s.action === "blocked" || s.action === "needs_choice").length;
  return {
    plan_id: "0123456789abcdef0123456789abcdef",
    playlist: { id: "PL-SUNDAY", name: "Sunday Morning Service", folder: null, place: "at the end of the section under the header \"Worship\"", insert_at: 4, total_items: 9, before: "Welcome song", after: "Message", untouched: 9, blocked: null, items_hash: "abc" },
    songs, to_add: songs.length - stuck, stuck, can_apply: songs.length - stuck > 0, propresenter: "ProPresenter 19.0.1 (API v1)", can_import: true, warnings: [],
  };
};
const GUESS = { section: "Interlude", position: 1, uses: 1, group_name: "Interlude", group_index: 2, groups: 2, rule: "in_order", text: "Interlude (1st use) uses the 2nd \"Interlude\" group: check it in ProPresenter." };

/** Four Playback songs: one needs a guess (still goes in), one cannot be added (the preview stops there). */
export const PREVIEW_PLAYBACK_STOPPED = planOf([
  row({ ref_id: "2000001", title: "Holy Forever", content_id: 5_000_101, key: "G", mapping: [{ section: "Verse 1", group: "Verse 1", how: "full", line: null }] }),
  row({ ref_id: "2000002", title: "Battle Belongs", content_id: 5_000_102, key: "Ab", file: "Battle Belongs (2)", guesses: [GUESS], mapping: [{ section: "Interlude", group: "Interlude", how: "guessed", line: GUESS.text }] }),
  row({ ref_id: "2000003", title: "Gratitude", content_id: 5_000_103, key: "A", action: "reuse", file: "Gratitude (1)" }),
  row({ ref_id: "2000004", title: "Firm Foundation", content_id: 5_000_104, key: "E", action: "blocked", file: null, reason: "These sections have no group with that name in the song: Bridge. Nothing is made for this song." }),
]);
/** The same four with nothing blocked: the one button goes on and adds them. */
export const PREVIEW_PLAYBACK_CLEAN = planOf(PREVIEW_PLAYBACK_STOPPED.songs.map((s) => (s.action === "blocked" ? { ...s, action: "copy" as const, file: "Firm Foundation (1)", reason: null } : s)));
/** Planning Center names: the three ProPresenter has go in as they are; the fourth cannot be ticked so it is not asked for. */
export const PREVIEW_NAMES = planOf(["Holy Forever", "Battle Belongs", "Gratitude"].map((title, i) => row({ ref_id: `t${i + 1}`, title, action: "as_is", file: title, source: `Songs / ${title}.pro`, sections: 0 })));

const done = (over: Partial<AddSongsReport> = {}): AddSongsReport => ({
  outcome: "done", message: "Added 4 song(s) to the playlist \"Sunday Morning Service\" and read it back: all as planned.", version: "ProPresenter 19.0.1 (API v1)",
  backup: "/home/church/.config/StagePilot/propresenter-playlist-backups/Sunday Morning Service-20261122-081500.json", songs: [], differences: [], playlist_changed: true, ...over,
});
const resultOf = (ref_id: string, title: string, status: AddSongsReport["songs"][number]["status"], text = "Added to the playlist.", guesses: AddSongsReport["songs"][number]["guesses"] = []) => ({ ref_id, title, status, file: `${title} (1)`, text, code: null, guesses });
const job = (over: Partial<AddSongsJob>): AddSongsJob => ({ id: "fedcba9876543210fedcba9876543210", plan_id: "0123456789abcdef0123456789abcdef", state: "running", started_at: "2026-11-22T08:15:00Z", steps: [], songs: [], events: [], report: null, error: null, ...over });

export const JOB_RUNNING = job({
  steps: [{ step: "check", state: "done", text: "ProPresenter says: ProPresenter 19.0.1.", notes: [] }, { step: "backup", state: "done", text: "Saved the playlist.", notes: [] }, { step: "playlist", state: "running", text: "Writing the playlist.", notes: [] }],
  songs: [
    { ref_id: "2000001", title: "Holy Forever", steps: [{ step: "copy", state: "done", text: "Made \"Holy Forever (1)\" in the Playback library.", notes: [] }], result: null },
    { ref_id: "2000002", title: "Battle Belongs", steps: [{ step: "copy", state: "running", text: "Making the copy from \"Songs / Battle Belongs.pro\".", notes: [] }], result: null },
    { ref_id: "2000003", title: "Gratitude", steps: [], result: null },
    { ref_id: "2000004", title: "Firm Foundation", steps: [], result: null },
  ],
  events: [{ song: "2000002", step: "copy", state: "running", text: "Making the copy from \"Songs / Battle Belongs.pro\"." }],
});
const finished = (report: AddSongsReport): AddSongsJob => job({ state: "done", report });
export const JOB_DONE = finished(done({ songs: ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"].map((t, i) => resultOf(String(2_000_001 + i), t, "added", "Added to the playlist.", i === 1 ? [GUESS] : [])) }));
export const JOB_DONE_NAMES = finished(done({ message: "Added 3 song(s) to the playlist \"Sunday Morning Service\" and read it back: all as planned.", songs: ["Holy Forever", "Battle Belongs", "Gratitude"].map((t, i) => resultOf(`t${i + 1}`, t, "added", "Found in the Songs library; it goes in as it is.")) }));
export const JOB_LIVE = finished(done({ outcome: "live_on_screen", message: "ProPresenter is showing something on screen (slide). Clear it or wait, then press Try again. Nothing was changed.", playlist_changed: false, backup: null, songs: [resultOf("2000004", "Firm Foundation", "failed", "ProPresenter is showing something on screen (slide). Clear it or wait, then press Try again.")] }));
export const JOB_LIBRARY = finished(done({ outcome: "needs_library_open", message: "ProPresenter has not loaded the new songs yet. Nothing was added to the playlist.", playlist_changed: false, songs: [resultOf("2000001", "Holy Forever", "not_loaded", "ProPresenter has not loaded this copy yet.")] }));

// ---- the answers one state gives --------------------------------------------------------------------------------------------
export interface Scenario {
  playlists?: ProPresenterPlaylist[];
  /** What ProPresenter's Songs library has, by title (a title left out is `found`). */
  titles?: Record<string, TitleFound>;
  /** The preview, per press (the last one repeats). `"hang"`: it never answers. */
  previews?: (AddSongsPlan | "hang")[];
  /** The job each apply starts, per apply (the last one repeats). */
  jobs?: AddSongsJob[];
}
const nth = <T,>(list: T[], n: number): T => list[Math.min(n, list.length - 1)]!;

/** An in-memory StagePilot for the gallery: it answers like the routes would, and counts presses so Try again gets a second answer. */
export function madeUpPresenterApi(scenario: Scenario = {}): PresenterApi {
  let previews = 0;
  let applies = 0;
  return {
    playlists: async () => ({ playlists: scenario.playlists ?? PLAYLISTS }),
    titles: async (titles) => ({ titles: titles.map((title) => ({ title, state: scenario.titles?.[title] ?? "found" })) }),
    preview: async () => {
      const next = nth(scenario.previews ?? [PREVIEW_PLAYBACK_CLEAN], previews);
      previews += 1;
      return next === "hang" ? new Promise<AddSongsPlan>(() => undefined) : next;
    },
    apply: async () => { applies += 1; return { job_id: "fedcba9876543210fedcba9876543210", plan_id: "0123456789abcdef0123456789abcdef", state: "running" as const }; },
    job: async () => nth(scenario.jobs ?? [JOB_RUNNING], Math.max(0, applies - 1)),
  };
}
