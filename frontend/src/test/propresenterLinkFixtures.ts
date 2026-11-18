import { vi } from "vitest";

import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type {
  AddSongsJob,
  AddSongsJobSong,
  AddSongsJobStep,
  AddSongsPlan,
  AddSongsReport,
  AddSongsSong,
  AddSongsSongResult,
  LinkState,
  LinkStatus,
  ProPresenterPlaylist,
} from "../propresenterLink";
import type { ProPresenterStatusResponse } from "../types";

/** The ProPresenter Link's answer for another computer (the helper) in `state`. The words are the backend's. */
export const helperStatus = (state: LinkState = "ready", over: Partial<LinkStatus> = {}): LinkStatus => ({
  mode: "helper",
  state,
  message: `The ProPresenter Link is ${state}.`,
  version: state === "ready" ? "0.1.0" : state === "outdated" ? "0.0.9" : undefined,
  bundled_version: "0.1.0",
  can_import: state === "ready" || state === "outdated",
  libraries: state === "ready" ? ["Songs", "Playback"] : [],
  playback_library_exists: state === "ready",
  computer_address: "192.168.1.60",
  paired: state !== "not_installed",
  updater: state === "ready" || state === "outdated",
  stale: false,
  ...over,
});

/** The answer when ProPresenter runs on this computer: no helper, no key, nothing to set up. */
export const localStatus = (over: Partial<LinkStatus> = {}): LinkStatus => ({
  mode: "local",
  state: "ready",
  message: "ProPresenter is on this computer, so no setup is needed.",
  version: "0.1.0",
  bundled_version: "0.1.0",
  can_import: true,
  libraries: ["Songs", "Playback"],
  playback_library_exists: true,
  paired: false,
  updater: false,
  stale: false,
  ...over,
});

/** No ProPresenter computer is saved yet: nothing is asked. */
export const noComputerStatus = (): LinkStatus => ({
  mode: null,
  state: "not_installed",
  message: "Choose the ProPresenter computer first.",
  bundled_version: "0.1.0",
  can_import: false,
  libraries: [],
  playback_library_exists: false,
  paired: false,
  updater: false,
  stale: false,
});

export const linkController = (status: LinkStatus | null = helperStatus("ready"), over: Partial<ProPresenterLinkController> = {}): ProPresenterLinkController => ({
  status, error: null, pending: null, check: vi.fn(), watch: vi.fn(), ...over,
});

/** The Presentation connection as the panel gets it. */
export const presentation = (over: Partial<ProPresenterStatusResponse> = {}): ProPresenterStatusResponse => ({
  enabled: true, host: "192.168.1.60", port: 1025, timer_name: "Song Countdown", look_id: null, request_timeout_seconds: 3,
  connection_status: "connected", detail: null, timers: [], selected_timer_id: null, timer_found: true, looks: [], current_look_id: null,
  look_found: true, last_checked_at: null, ...over,
});

export const playlists = (): ProPresenterPlaylist[] => [
  { id: "PL-SUNDAY", name: "Sunday Morning Service", folder: null, headers: ["Welcome", "Worship", "Message"] },
  { id: "PL-WED", name: "Wednesday Night", folder: null, headers: [] },
  { id: "PL-YOUTH", name: "Youth Night", folder: "Special services", headers: ["Games"] },
];

export const addSong = (over: Partial<AddSongsSong> & Pick<AddSongsSong, "ref_id" | "title">): AddSongsSong => ({
  position: 1, content_id: 5_000_001, action: "copy", file: `${over.title} (1)`, source: `Songs / ${over.title}.pro`, import_as: null, reason: null,
  choices: [], notes: [], mapping: [], sections: 9, key: "G", already_in_playlist: false, ...over,
});

export const addPlan = (songs: AddSongsSong[], over: Partial<AddSongsPlan> = {}): AddSongsPlan => {
  const stuck = songs.filter((song) => song.action === "blocked" || song.action === "needs_choice").length;
  return {
    plan_id: "0123456789abcdef0123456789abcdef",
    playlist: {
      id: "PL-SUNDAY", name: "Sunday Morning Service", folder: null, place: "at the end of the section under the header \"Worship\"", insert_at: 3, total_items: 6,
      before: "Worship", after: "Message", untouched: 6, blocked: null, items_hash: "abc",
    },
    songs,
    to_add: songs.length - stuck,
    stuck,
    can_apply: songs.length - stuck > 0,
    propresenter: "ProPresenter 19.0.1 (API v1)",
    can_import: true,
    warnings: [],
    ...over,
  };
};

export const step = (name: AddSongsJobStep["step"], state: AddSongsJobStep["state"], text = "", notes: string[] = []): AddSongsJobStep => ({ step: name, state, text, notes });

export const result = (ref_id: string, title: string, status: AddSongsSongResult["status"], text = "Added to the playlist."): AddSongsSongResult => ({
  ref_id, title, status, file: `${title} (1)`, text, code: null,
});

export const report = (over: Partial<AddSongsReport> = {}): AddSongsReport => ({
  outcome: "done", message: "Added 2 song(s) to the playlist \"Sunday Morning Service\" and read it back: all as planned.", version: "ProPresenter 19.0.1 (API v1)",
  backup: "/home/church/.config/StagePilot/propresenter-playlist-backups/Sunday Morning Service-20261122-081500.json", songs: [], differences: [], playlist_changed: true, ...over,
});

export const jobOf = (over: Partial<AddSongsJob> = {}, songs: AddSongsJobSong[] = []): AddSongsJob => ({
  id: "fedcba9876543210fedcba9876543210", plan_id: "0123456789abcdef0123456789abcdef", state: "running", started_at: "2026-11-22T08:15:00Z", steps: [], songs,
  events: [], report: null, error: null, ...over,
});
