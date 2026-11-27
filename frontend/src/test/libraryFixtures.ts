import { vi } from "vitest";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { HelperState, LibraryHelper, LibraryPlan, LibraryPlanSong, LibrarySong, LibraryStatus } from "../types";

/** The Library Link's state as the backend reports it; the words are the backend's. */
export const helperOf = (state: HelperState, over: Partial<LibraryHelper> = {}): LibraryHelper => ({
  state,
  version: state === "ready" ? "0.3.0" : state === "outdated" ? "0.2.0" : null,
  bundled_version: "0.3.0",
  message: `The Library Link is ${state}.`,
  paired: state === "ready" || state === "outdated" || state === "needs_key",
  ...over,
});

/** A status answer of the library for a helper in `state`. Library features exist only in `ready` and `outdated`. */
export const libraryStatus = (state: HelperState = "ready", over: Partial<LibraryStatus> = {}): LibraryStatus => {
  const active = state === "ready" || state === "outdated";
  return {
    enabled: active || state === "unreachable",
    active,
    helper: helperOf(state),
    address: "http://10.30.0.141",
    state: active ? "ready" : state === "unreachable" ? "problem" : "off",
    message: active ? "Playback library 0.3.0 is ready: 12 songs in 3 setlists." : `The Library Link is ${state}.`,
    version: active ? "0.3.0" : null,
    songs: active ? 12 : null,
    setlists: active ? 3 : null,
    stale: false,
    checked_at: null,
    ...over,
  };
};

export const librarySong = (over: Partial<LibrarySong> = {}): LibrarySong => ({
  setlist_song_id: 2000001, content_id: 23126, position: 1, title: "Song One", key: "Ab", key_original: "C", key_was: null, key_changed_at: null, has_waveform: false, bpm: 65, scan_length_s: 388.64, countdown_length_s: 388.64,
  length_source: "last_measure", arrangement: "Default", section_count: 3,
  sections: [{ name: "Count Off", start: 0, end: 4 }, { name: "Verse 1", start: 4, end: 24 }, { name: "Vamp on 1", start: 24, end: 28 }],
  artwork: { type: "image/jpeg", bytes: 46874, etag: "\"0123456789abcdef\"" }, rental_expires_utc: null, lyric_cue_count: 0, lyric_cue_max_slide: null, ...over,
});

export const librarySongOf = (title: string, position: number, over: Partial<LibraryPlanSong> = {}): LibraryPlanSong => ({
  key: `k${position}`, title, plan_position: position, plan_seconds: 389, plan_key: null, status: "matched", confidence: "exact",
  library: librarySong({ title, position, setlist_song_id: 2000000 + position }), candidate: null, ...over,
});

export const libraryPlan = (songs: LibraryPlanSong[], over: Partial<LibraryPlan> = {}): LibraryPlan => ({
  enabled: true, state: "ready", message: null, plan_date: "2026-11-22", setlist: { id: 1, title: "Set", date: "2026-11-22", songs: songs.length },
  newest: null, stale: false, checked_at: null, songs, playback_only: [], chosen: false, setlists: [], ...over,
});

/** A library controller whose helper is in `state` (its plan is only there when the helper is active). */
export const libraryController = (state: HelperState = "ready", over: Partial<PlaybackLibraryController> = {}): PlaybackLibraryController => {
  const status = libraryStatus(state);
  return {
    target: "10.30.0.141", address: "", status, plan: status.active ? libraryPlan([librarySongOf("Song One", 1)]) : null, error: null, pending: null,
    saveAddress: vi.fn().mockResolvedValue(true), chooseSetlist: vi.fn().mockResolvedValue(true), check: vi.fn(), watch: vi.fn(), ...over,
  };
};
