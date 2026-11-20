// The ProPresenter Link's shapes. Everything the backend generates (ts-rs) is imported from `generated/ts`, never written out
// again here. The two routes that answer with a plain JSON envelope the server crate does not generate a type for (the playlist
// list and the job) are composed below from generated pieces; they are the only hand-written shapes.
import type { AddSongsEvent } from "../../generated/ts/AddSongsEvent";
import type { AddSongsReport } from "../../generated/ts/AddSongsReport";
import type { AddSongsSongResult } from "../../generated/ts/AddSongsSongResult";
import type { AddSongsStep } from "../../generated/ts/AddSongsStep";
import type { AddSongsStepState } from "../../generated/ts/AddSongsStepState";

export type { AddSongsAction } from "../../generated/ts/AddSongsAction";
export type { AddSongsEvent } from "../../generated/ts/AddSongsEvent";
export type { AddSongsMapping } from "../../generated/ts/AddSongsMapping";
export type { AddSongsOutcome } from "../../generated/ts/AddSongsOutcome";
export type { AddSongsPlan } from "../../generated/ts/AddSongsPlan";
export type { AddSongsPlaylist } from "../../generated/ts/AddSongsPlaylist";
export type { AddSongsReport } from "../../generated/ts/AddSongsReport";
export type { AddSongsSong } from "../../generated/ts/AddSongsSong";
export type { AddSongsSongResult } from "../../generated/ts/AddSongsSongResult";
export type { AddSongsSongStatus } from "../../generated/ts/AddSongsSongStatus";
export type { AddSongsStep } from "../../generated/ts/AddSongsStep";
export type { AddSongsStepState } from "../../generated/ts/AddSongsStepState";
export type { LinkMode } from "../../generated/ts/LinkMode";
export type { LinkState } from "../../generated/ts/LinkState";
export type { LinkStatus } from "../../generated/ts/LinkStatus";

/** One playlist of `GET /api/v1/propresenter-link/playlists`: folders are flattened (`folder` names the one it is in), `headers` are its header items in order. */
export interface ProPresenterPlaylist {
  id: string;
  name: string;
  folder: string | null;
  headers: string[];
}

/** Where in the playlist the songs go: after the section under a header (by name), or at the end. */
export type AddSongsPlace = { after_header: string } | { place: "end" };

/** The body of `POST add-songs/preview`: the songs are named by Playback's setlist ids (or, when it has none, by MultiTracks numbers). */
export type AddSongsPreviewRequest = (
  | { setlist_song_ids: number[] }
  | { content_ids: number[] }
) & { playlist_id: string } & AddSongsPlace;

/** What `POST add-songs/apply` answers (202). */
export interface AddSongsStarted {
  job_id: string;
  plan_id: string;
  state: "running";
}

/** One step of a job as the route sums it up: the latest word for the step, and its side notes. */
export interface AddSongsJobStep {
  step: AddSongsStep;
  state: AddSongsStepState;
  text: string;
  notes: string[];
}

export interface AddSongsJobSong {
  ref_id: string;
  title: string;
  steps: AddSongsJobStep[];
  /** Filled when the job has finished. */
  result: AddSongsSongResult | null;
}

/** `GET add-songs/jobs/<id>`. `report` arrives with `state: "done"`; `error` with `state: "failed"` (the job stopped without an answer). */
export interface AddSongsJob {
  id: string;
  plan_id: string;
  state: "running" | "done" | "failed";
  started_at: string;
  steps: AddSongsJobStep[];
  songs: AddSongsJobSong[];
  events: AddSongsEvent[];
  report: AddSongsReport | null;
  error: string | null;
}
