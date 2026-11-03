import type { OscLightingCue, SongLightingCueMap } from "../types";

import { isCaptured } from "./lightingLooks";

/** What a clear would erase, counted from the saved cue map. */
export interface ClearCounts { looks: number; handmade: number; loudness: number }

export type ClearKind = "song" | "all" | "loudness";

/** One clear waiting for the person's answer. Shown by ClearCuesDialog; `onErase` runs only on "Erase permanently". */
export interface ClearCuesRequest extends ClearCounts {
  kind: ClearKind;
  /** The song title, or `every song in "<plan>"`. */
  subject: string;
  onErase: () => void;
}

/** Captured rows are looks; every other OSC row, or every MIDI cue, is hand-made. */
export const countCueMap = (map: SongLightingCueMap | undefined, method: "osc" | "midi"): ClearCounts => {
  const osc: OscLightingCue[] = map?.osc_cues ?? [];
  const looks = method === "osc" ? osc.filter(isCaptured).length : 0;
  const handmade = method === "osc" ? osc.length - looks : (map?.cues.length ?? 0);
  return { looks, handmade, loudness: map?.loudness ? 1 : 0 };
};

export const sumCounts = (parts: ClearCounts[]): ClearCounts =>
  parts.reduce((total, part) => ({ looks: total.looks + part.looks, handmade: total.handmade + part.handmade, loudness: total.loudness + part.loudness }), { looks: 0, handmade: 0, loudness: 0 });

export const clearSongRequest = (songTitle: string, counts: ClearCounts, onErase: () => void): ClearCuesRequest =>
  ({ kind: "song", subject: songTitle, looks: counts.looks, handmade: counts.handmade, loudness: 0, onErase });

export const clearAllRequest = (songTitles: string[], planTitle: string | null, counts: ClearCounts, onErase: () => void): ClearCuesRequest => ({
  kind: "all",
  subject: songTitles.length === 1 ? songTitles[0]! : `every song in "${planTitle ?? "the current service plan"}"`,
  looks: counts.looks, handmade: counts.handmade, loudness: 0, onErase,
});

export const clearLoudnessRequest = (songTitle: string, onErase: () => void): ClearCuesRequest =>
  ({ kind: "loudness", subject: songTitle, looks: 0, handmade: 0, loudness: 1, onErase });

export const confirmRerecordLooks = () =>
  window.confirm("Recording replaces this song's captured looks, including your edits. Record anyway?");

export const confirmRemoveFixture = (fixture: string, songTitle: string) =>
  window.confirm(`Remove fixture "${fixture}" from "${songTitle}"? Its messages are deleted from every look row, and rows left empty are removed.`);

export const confirmMergeChangesValues = (addresses: string[]) =>
  window.confirm(`Merging moves later values earlier for ${addresses.length} address${addresses.length === 1 ? "" : "es"} (for example ${addresses[0]}). Merge anyway?`);

