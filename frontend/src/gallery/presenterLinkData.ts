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

// ---- Playback's setlist: the same songs, with number, key and order ----------------------------------------------------------
// `id` is the MultiTracks Song Master ID (the number ProPresenter shows as MTID: 4 to 6 digits, `content_id`). Playback's own song number
// (`setlist_song_id`, a long number) is different on purpose: the cards must show the first and never the second.
const SET: { title: string; key: string; arrangement: string; id: number | null; sections: string[] }[] = [
  { title: "Holy Forever", key: "G", arrangement: "Radio", id: 43302, sections: ["Count Off", "Intro", "Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge", "Chorus"] },
  { title: "Battle Belongs", key: "Ab", arrangement: "Long ending", id: 23126, sections: ["Count Off", "Intro", "Verse 1", "Chorus", "Interlude", "Verse 2", "Chorus", "Outro"] },
  { title: "Gratitude", key: "A", arrangement: "Default", id: 31877, sections: ["Count Off", "Verse 1", "Chorus", "Verse 2", "Chorus"] },
  { title: "Firm Foundation", key: "E", arrangement: "Default", id: 8412, sections: ["Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge"] },
  // The songs only the all-statuses preview uses.
  { title: "Living Hope", key: "D", arrangement: "Default", id: 21874, sections: ["Count Off", "Verse 1", "Chorus", "Verse 2", "Chorus"] },
  { title: "Graves Into Gardens", key: "B", arrangement: "Default", id: 24017, sections: ["Count Off", "Verse 1", "Chorus", "Bridge 2", "Chorus"] },
  { title: "Goodness of God", key: "Bb", arrangement: "Default", id: null, sections: ["Count Off", "Verse 1", "Chorus", "Verse 2", "Chorus"] },
  { title: "What a Beautiful Name", key: "D", arrangement: "Default", id: 27363, sections: ["Count Off", "Verse 1", "Chorus", "Interlude", "Interlude", "Interlude", "Chorus"] },
  { title: "Great Are You Lord", key: "C", arrangement: "Default", id: 12988, sections: ["Count Off", "Verse 1", "Chorus", "Verse 2", "Chorus"] },
  { title: "Build My Life", key: "E", arrangement: "Default", id: 19405, sections: ["Count Off", "Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge"] },
];
/** Playback's own song number for the song at `index`: long, and never the number a card shows. */
const SETLIST_ID = (index: number) => 30_389_715 + index;
const REF = (index: number) => String(SETLIST_ID(index));
const librarySong = (item: (typeof SET)[number], index: number): LibrarySong => ({
  setlist_song_id: SETLIST_ID(index), content_id: item.id, position: index + 1, title: item.title, key: item.key, key_original: item.key,
  // The first song's key was changed in Playback an hour ago: its card says "(was F)".
  key_was: index === 0 ? "F" : null, key_changed_at: index === 0 ? new Date(Date.now() - 3_600_000).toISOString() : null, bpm: 72,
  scan_length_s: 300, countdown_length_s: 300, length_source: "last_measure", arrangement: item.arrangement, section_count: item.sections.length,
  sections: item.sections.map((name, i) => ({ name, start: i * 20, end: i * 20 + 19 })), artwork: null, rental_expires_utc: null,
});
const planSong = (item: (typeof SET)[number], index: number): LibraryPlanSong => ({
  key: `k${index + 1}`, title: item.title, plan_position: index + 1, plan_seconds: 300, plan_key: null, status: "matched", confidence: "exact", library: librarySong(item, index), candidate: null,
});
/** The setlist with its first `count` songs (four, or all ten for the preview with every status). */
export const setlistOf = (count = 4): LibraryPlan => ({
  enabled: true, state: "ready", message: null, plan_date: "2026-11-22", setlist: { id: 1, title: "Sunday set", date: "2026-11-22", songs: count },
  newest: null, stale: false, checked_at: null, songs: SET.slice(0, count).map(planSong), playback_only: [], chosen: false, setlists: [],
});
export const SETLIST: LibraryPlan = setlistOf();
/** The Library Link with Playback's songs, or without any data (not installed). */
export const withPlayback = (count = 4): PlaybackLibraryController => ({ ...libraryController(libraryOf("ready")), plan: setlistOf(count) });
export const withoutPlayback = (): PlaybackLibraryController => libraryController(libraryOf("not_installed"));

// ---- what the routes answer --------------------------------------------------------------------------------------------------
const row = (over: Partial<AddSongsSong> & Pick<AddSongsSong, "ref_id" | "title">): AddSongsSong => ({
  position: 1, content_id: null, action: "copy", file: `${over.title} (1)`, source: `Songs / ${over.title}.pro`, import_as: null, reason: null, choices: [], chosen_by: null, chosen_file: null, notes: [], mapping: [], guesses: [], sections: 7, key: null, already_in_playlist: false, ...over,
});
const planOf = (songs: AddSongsSong[]): AddSongsPlan => {
  const stuck = songs.filter((s) => s.action === "blocked" || s.action === "needs_choice").length;
  return {
    plan_id: "0123456789abcdef0123456789abcdef",
    playlist: { id: "PL-SUNDAY", name: "Sunday Morning Service", folder: null, place: "at the end of the section under the header \"Worship\"", insert_at: 4, total_items: 9, before: "Welcome song", after: "Message", untouched: 9, blocked: null, items_hash: "abc" },
    songs, to_add: songs.length - stuck, stuck, can_apply: songs.length - stuck > 0, propresenter: "ProPresenter 19.0.1 (API v1)", can_import: true, warnings: [],
  };
};
const GUESS = { section: "Interlude", position: 3, uses: 3, group_name: "Interlude", group_index: 2, groups: 2, rule: "last_reused", text: "Interlude (3rd use) uses the 2nd \"Interlude\" group: check it in ProPresenter." };

/** Four Playback songs: one needs a guess (still goes in), one cannot be added (the preview stops there). */
export const PREVIEW_PLAYBACK_STOPPED = planOf([
  row({ ref_id: REF(0), title: "Holy Forever", content_id: 43302, key: "G", mapping: [{ section: "Verse 1", group: "Verse 1", how: "full", line: null }] }),
  row({ ref_id: REF(1), title: "Battle Belongs", content_id: 23126, key: "Ab", file: "Battle Belongs (2)", guesses: [GUESS], mapping: [{ section: "Interlude", group: "Interlude", how: "guessed", line: GUESS.text }] }),
  row({ ref_id: REF(2), title: "Gratitude", content_id: 31877, key: "A", action: "reuse", file: "Gratitude (1)" }),
  row({ ref_id: REF(3), title: "Firm Foundation", content_id: 8412, key: "E", action: "blocked", file: null, reason: "These sections have no group with that name in the song: Bridge. Nothing is made for this song." }),
]);
/** The preview with EVERY status (the four of Hunter's screenshot first, then the rest): one chip and at most one short phrase each. */
const MAC = "ProPresenter does not have this song and it cannot be imported from here. Importing a song needs a Mac; this computer is not one.";
export const PREVIEW_ALL_STATUSES = planOf([
  row({ ref_id: REF(0), title: "Holy Forever", content_id: 43302, key: "G", action: "reuse", file: "Holy Forever (1)", already_in_playlist: true }),
  row({ ref_id: REF(1), title: "Battle Belongs", content_id: 23126, key: "Ab", action: "blocked", file: null, reason: MAC }),
  row({ ref_id: REF(2), title: "Gratitude", content_id: 31877, key: "A", action: "reuse", file: "Gratitude (1)", already_in_playlist: true }),
  row({ ref_id: REF(3), title: "Firm Foundation", content_id: 8412, key: "E", action: "needs_choice", file: null, reason: "Two files are this song. Pick one.", choices: ["Songs / Firm Foundation (Live).pro", "Songs / Firm Foundation.pro"] }),
  row({ ref_id: REF(4), title: "Living Hope", content_id: 21874, key: "D", action: "import_then_copy", file: "Living Hope (1)", import_as: "Living Hope" }),
  row({ ref_id: REF(5), title: "Graves Into Gardens", content_id: 24017, key: "B", action: "blocked", file: null, reason: "These sections have no group with that name in the song: Bridge 2. Nothing is made for this song.", mapping: [{ section: "Bridge 2", group: null, how: "missing", line: "No group called Bridge 2." }] }),
  row({ ref_id: REF(6), title: "Goodness of God", content_id: null, key: "Bb", action: "blocked", file: null, reason: "Playback did not give this song's MultiTracks number, so StagePilot cannot find it in ProPresenter." }),
  row({ ref_id: REF(7), title: "What a Beautiful Name", content_id: 27363, key: "D", file: "What a Beautiful Name (1)", guesses: [GUESS], mapping: [{ section: "Interlude", group: "Interlude", how: "guessed", line: GUESS.text }] }),
  row({ ref_id: REF(8), title: "Great Are You Lord", content_id: 12988, key: "C", action: "reuse", file: "Great Are You Lord (1)" }),
  row({ ref_id: REF(9), title: "Build My Life", content_id: 19405, key: "E", file: "Build My Life (2)" }),
]);
/** The same four with nothing blocked: the one button goes on and adds them. */
export const PREVIEW_PLAYBACK_CLEAN = planOf(PREVIEW_PLAYBACK_STOPPED.songs.map((s) => (s.action === "blocked" ? { ...s, action: "copy" as const, file: "Firm Foundation (1)", reason: null } : s)));
// ---- "Pick one": the person chooses the song's file on its card ------------------------------------------------------------------
const FIRM_FILES = ["Songs / Firm Foundation (Live).pro", "Songs / Firm Foundation.pro"];
const firm = (over: Partial<AddSongsSong>): AddSongsSong => row({ ref_id: REF(3), title: "Firm Foundation", content_id: 8412, key: "E", ...over });
/** The four songs with Firm Foundation as given and Battle Belongs unable to import, so the preview stops there and the cards stay to be looked at. */
const withFirm = (song: AddSongsSong) => planOf(PREVIEW_PLAYBACK_STOPPED.songs.map((s) => (s.title === "Firm Foundation" ? song : s.title === "Battle Belongs" ? row({ ref_id: REF(1), title: "Battle Belongs", content_id: 23126, key: "Ab", action: "blocked", file: null, reason: MAC }) : s)));
/** Two files are this song and the name rule cannot tell: the card asks. */
export const PREVIEW_PICK_ONE = withFirm(firm({ action: "needs_choice", file: null, source: null, reason: "Two files are this song. Pick one.", choices: FIRM_FILES }));
/** The person pressed the first file. */
export const PREVIEW_PICKED = withFirm(firm({
  file: "Firm Foundation (Live) (1)", source: FIRM_FILES[0]!, choices: FIRM_FILES, chosen_by: "pick", chosen_file: FIRM_FILES[0]!,
  notes: ["Used \"Firm Foundation (Live)\": the file you picked."], mapping: [{ section: "Verse 1", group: "Verse 1", how: "full", line: null }],
}));
/** The name rule decided: only one file is named like the Playback title, so nobody was asked. */
export const PREVIEW_PICKED_BY_NAME = withFirm(firm({
  file: "Firm Foundation (1)", source: FIRM_FILES[1]!, choices: FIRM_FILES, chosen_by: "title", chosen_file: FIRM_FILES[1]!,
  notes: ["Used \"Firm Foundation\": its name matches the Playback title."], mapping: [{ section: "Verse 1", group: "Verse 1", how: "full", line: null }],
}));

/** Planning Center names: the three ProPresenter has go in as they are; the fourth cannot be ticked so it is not asked for. */
export const PREVIEW_NAMES = planOf(["Holy Forever", "Battle Belongs", "Gratitude"].map((title, i) => row({ ref_id: `t${i + 1}`, title, action: "as_is", file: title, source: `Songs / ${title}.pro`, sections: 0 })));

const done = (over: Partial<AddSongsReport> = {}): AddSongsReport => ({
  outcome: "done", message: "Added 4 song(s) to the playlist \"Sunday Morning Service\" and read it back: all as planned.", version: "ProPresenter 19.0.1 (API v1)",
  backup: "/home/church/.config/StagePilot/propresenter-playlist-backups/Sunday Morning Service-20261122-081500.json", songs: [], differences: [], playlist_changed: true, ...over,
});
const resultOf = (ref_id: string, title: string, status: AddSongsReport["songs"][number]["status"], text = "Added to the playlist.", guesses: AddSongsReport["songs"][number]["guesses"] = []) => ({ ref_id, title, status, file: `${title} (1)`, text, code: null, guesses });
const job = (over: Partial<AddSongsJob>): AddSongsJob => ({ id: "fedcba9876543210fedcba9876543210", plan_id: "0123456789abcdef0123456789abcdef", state: "running", started_at: "2026-11-22T08:15:00Z", steps: [], songs: [], events: [], report: null, error: null, ...over });

export const JOB_RUNNING = job({
  steps: [{ step: "check", state: "done", text: "ProPresenter says: ProPresenter 19.0.1.", notes: [] }, { step: "backup", state: "done", text: "Saved the playlist.", notes: [] }, { step: "library", state: "done", text: "The Playback library is there.", notes: [] }, { step: "playlist", state: "running", text: "Writing the playlist.", notes: [] }],
  songs: [
    { ref_id: REF(0), title: "Holy Forever", steps: [{ step: "copy", state: "done", text: "Made \"Holy Forever (1)\" in the Playback library.", notes: [] }], result: null },
    { ref_id: REF(1), title: "Battle Belongs", steps: [{ step: "copy", state: "running", text: "Making the copy from \"Songs / Battle Belongs.pro\".", notes: [] }], result: null },
    { ref_id: REF(2), title: "Gratitude", steps: [], result: null },
    { ref_id: REF(3), title: "Firm Foundation", steps: [], result: null },
  ],
  events: [{ song: REF(1), step: "copy", state: "running", text: "Making the copy from \"Songs / Battle Belongs.pro\"." }],
});
const finished = (report: AddSongsReport): AddSongsJob => job({ state: "done", report });
export const JOB_DONE = finished(done({ songs: ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"].map((t, i) => resultOf(REF(i), t, "added", "Added to the playlist.", i === 1 ? [GUESS] : [])) }));
export const JOB_DONE_NAMES = finished(done({ message: "Added 3 song(s) to the playlist \"Sunday Morning Service\" and read it back: all as planned.", songs: ["Holy Forever", "Battle Belongs", "Gratitude"].map((t, i) => resultOf(`t${i + 1}`, t, "added", "Found in the Songs library; it goes in as it is.")) }));
/** Three songs went in and the read-back found one thing different. */
export const JOB_DIFFERS = finished(done({
  outcome: "done_with_differences", message: "Added 3 song(s) to the playlist \"Sunday Morning Service\", but ProPresenter shows 1 difference(s) from the plan.",
  songs: ["Holy Forever", "Battle Belongs", "Gratitude"].map((t, i) => resultOf(REF(i), t, "added")), differences: ["Item 7 \"Message\": its duration reads 0 now, it was 80."],
}));
/** The job stopped at a song it could not import; the others were not tried. */
export const JOB_STOPPED = finished(done({
  outcome: "stopped", message: "Stopped at \"Battle Belongs\": ProPresenter does not have this song and it cannot be imported from here. Nothing was added to the playlist.", playlist_changed: false,
  songs: [resultOf(REF(0), "Holy Forever", "skipped", "Left out: the job stopped first."), resultOf(REF(1), "Battle Belongs", "failed", "ProPresenter does not have this song and it cannot be imported from here."), resultOf(REF(2), "Gratitude", "not_tried", "Not reached.")],
}));
export const JOB_LIVE = finished(done({ outcome: "live_on_screen", message: "ProPresenter is showing something on screen (slide). Clear it or wait, then press Try again. Nothing was changed.", playlist_changed: false, backup: null, songs: [resultOf(REF(3), "Firm Foundation", "failed", "ProPresenter is showing something on screen (slide). Clear it or wait, then press Try again.")] }));
export const JOB_LIBRARY = finished(done({ outcome: "needs_library_open", message: "ProPresenter has not loaded the new songs yet. Nothing was added to the playlist.", playlist_changed: false, songs: [resultOf(REF(0), "Holy Forever", "not_loaded", "ProPresenter has not loaded this copy yet.")] }));

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
