import type { SongLightingCueMap } from "../types";

/** How many cues one saved map holds for the method on screen (MIDI `cues`, OSC `osc_cues`). */
export const cuesInMap = (map: SongLightingCueMap | undefined, method: "osc" | "midi"): number =>
  (method === "osc" ? map?.osc_cues?.length : map?.cues?.length) ?? 0;

/**
 * The Lighting timeline's "what is in here" part: only the songs of the service on screen count, looked up with the
 * same key as the song cards. A saved map of a song from another service is never counted, so X can never exceed Y.
 */
export const serviceCueText = (
  songKeys: string[],
  cueMaps: Record<string, SongLightingCueMap> | undefined,
  method: "osc" | "midi",
  formatCount: (count: number) => string,
): string => {
  // A song listed twice in one service is one saved map: count it once.
  const keys = [...new Set(songKeys)];
  const counts = keys.map((key) => cuesInMap(cueMaps?.[key], method));
  const total = counts.reduce((sum, count) => sum + count, 0);
  if (total === 0) return "No cues yet";
  const withCues = counts.filter((count) => count > 0).length;
  const songTotal = songKeys.length;
  return `${formatCount(total)} cue${total === 1 ? "" : "s"} on ${withCues} of ${songTotal} song${songTotal === 1 ? "" : "s"}`;
};
