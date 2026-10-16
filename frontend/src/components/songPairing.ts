// Which Playback song goes with which service-plan song when either side has songs to skip.
// Neither list is ever reordered: the Nth song that is not skipped on one side pairs with the Nth that is
// not skipped on the other. Mirrors backend/src/stagepilot/plugins/playback_api/arrangement.py.

export type ArrangementLine =
  | { kind: "pair"; plan: number; playback: number }
  | { kind: "plan-skip"; plan: number }
  | { kind: "playback-skip"; playback: number }
  | { kind: "plan-alone"; plan: number }
  | { kind: "playback-alone"; playback: number };

export function arrangeSongs(
  planIds: readonly string[],
  playbackCount: number,
  planSkips: ReadonlySet<string>,
  playbackSkips: ReadonlySet<number>,
  /** Playback song IDs in order; a skip is stored by ID, so it follows the song. */
  order: readonly number[],
): ArrangementLine[] {
  const lines: ArrangementLine[] = [];
  let plan = 0;
  let playback = 0;
  while (plan < planIds.length || playback < playbackCount) {
    if (plan < planIds.length && planSkips.has(planIds[plan]!)) { lines.push({ kind: "plan-skip", plan }); plan += 1; continue; }
    if (playback < playbackCount && playbackSkips.has(order[playback]!)) { lines.push({ kind: "playback-skip", playback }); playback += 1; continue; }
    if (plan < planIds.length && playback < playbackCount) { lines.push({ kind: "pair", plan, playback }); plan += 1; playback += 1; continue; }
    if (plan < planIds.length) { lines.push({ kind: "plan-alone", plan }); plan += 1; continue; }
    lines.push({ kind: "playback-alone", playback });
    playback += 1;
  }
  return lines;
}

/** Whole seconds the two lengths are apart, or null when either is unknown. */
export function secondsApart(plan: number | null | undefined, playback: number | null | undefined): number | null {
  if (typeof plan !== "number" || typeof playback !== "number" || !Number.isFinite(plan) || !Number.isFinite(playback)) return null;
  return Math.abs(plan - Math.floor(playback + 0.5));
}

/** Past this many seconds the pair is probably two different songs. */
export const LIKELY_DIFFERENT_SECONDS = 120;
/** Within this many seconds the pair is a confident match. */
export const CLOSE_SECONDS = 30;
