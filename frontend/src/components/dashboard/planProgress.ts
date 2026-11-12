import type { ApplicationState } from "../../types";

/**
 * What the dashboard has seen of the loaded plan being played, for the "Scan Playback again" reminder (see scanReminder.ts).
 *
 * The backend state keeps no history: `playing_song_index` says which song was started last, and `timer` says whether that
 * song's countdown is running or stopped. Nothing there says "the plan is over". So the dashboard remembers two moments, while
 * it is open and from the same state it already receives:
 *  - `firstSongMs`: when the first song of the run in progress was started (a run-through counts, so does a service);
 *  - `endedMs`: when the plan's LAST song ended.
 *
 * "The last song ended" is read from that song's own countdown, which is the most reliable thing the app holds:
 *  - the last song (`playing_song_index` is the last index) has a scheduled length;
 *  - its timer is running and `started_at` plus the time left at that moment is behind us (the countdown reached 0:00), or
 *  - its timer is stopped with 0 seconds remaining (Playback stopped it at the end, or the operator stopped it there).
 * A song that is paused part-way (stopped with time remaining) or a plan whose last song has no length is NOT read as
 * over: `scanReminder.ts` then falls back to a time estimate, and always applies a hard cap.
 *
 * A new run starts when a countdown starts after `endedMs`, or more than `PLAN_IDLE_MS` after the last song activity;
 * nothing needs resetting.
 *
 * F-S3 added one moment, `lastActiveMs`: the last time a song was playing or its countdown reached 0:00 (a song that is
 * stopped or paused leaves it at the last time it was seen running). A sound check that never reaches the last song has no
 * `endedMs`; `scanReminder.ts` reads "5 minutes after `lastActiveMs`" as that run being over. Still three numbers per plan.
 */
export type PlanRunMemory = {
  planId: string;
  firstSongMs: number | null;
  endedMs: number | null;
  /** Last song activity (ms): a countdown running (up to its 0:00) or just reaching 0:00. Null until a song is seen. */
  lastActiveMs: number | null;
};

/** A run with no song activity for this long is over (a sound check that stopped). Gaps between songs of a service are not judged by it. */
export const PLAN_IDLE_MS = 5 * 60 * 1000;

export type PlanRunObservation = {
  /** The dashboard's clock (ms). */
  now: number;
  planId: string | null | undefined;
  /** The plan's songs in order (only their lengths are read). */
  durations: ReadonlyArray<number | null | undefined>;
  /** `state.playing_song_index`, or the current song's index when a countdown runs without one. */
  playingIndex: number | null | undefined;
  timer: Pick<ApplicationState["timer"], "status" | "started_at" | "position_seconds" | "remaining_seconds">;
};

export function observePlanRun(previous: PlanRunMemory | null, obs: PlanRunObservation): PlanRunMemory | null {
  if (!obs.planId) return null;
  const memory: PlanRunMemory = previous && previous.planId === obs.planId ? previous : { planId: obs.planId, firstSongMs: null, endedMs: null, lastActiveMs: null };
  const lastIndex = obs.durations.length - 1;
  const started = obs.timer.started_at ? Date.parse(obs.timer.started_at) : Number.NaN;
  const running = obs.timer.status === "running" && Number.isFinite(started);
  const songStartMs = running ? started - Math.max(0, obs.timer.position_seconds ?? 0) * 1_000 : null;
  const index = obs.playingIndex ?? null;
  let { firstSongMs, endedMs, lastActiveMs } = memory;

  if (lastIndex >= 0 && (index !== null || running)) {
    // A countdown that starts after the last run ended, or after the run went quiet for 5 minutes, begins a new run;
    // otherwise the run in progress keeps its first song. (`started` is when this countdown began or resumed, so a tab that
    // was asleep while one song kept playing does not look like a gap.)
    const afterQuiet = running && lastActiveMs !== null && started - lastActiveMs > PLAN_IDLE_MS;
    if (firstSongMs === null || afterQuiet || (songStartMs !== null && endedMs !== null && songStartMs > endedMs)) {
      firstSongMs = songStartMs ?? obs.now; // seen between songs after a page reload: the best we know is now
    }
    const length = index !== null ? obs.durations[index] : null;
    const songEndMs = running && length && length > 0 ? started + Math.max(0, length - Math.max(0, obs.timer.position_seconds ?? 0)) * 1_000 : null;
    if (running) {
      // Playing counts as activity up to the countdown's 0:00 (a song without a length: while its timer runs).
      lastActiveMs = Math.max(lastActiveMs ?? 0, songEndMs === null ? obs.now : Math.min(obs.now, songEndMs));
    } else if (lastActiveMs === null) {
      // Seen only stopped (page opened between songs, or a countdown that stopped at 0:00): dated when first seen.
      lastActiveMs = obs.now;
    }
    const lastLength = obs.durations[lastIndex];
    if (index === lastIndex && lastLength && lastLength > 0) {
      const runActive = endedMs === null || firstSongMs === null || endedMs < firstSongMs;
      if (running) {
        const endMs = started + Math.max(0, lastLength - Math.max(0, obs.timer.position_seconds ?? 0)) * 1_000;
        if (obs.now >= endMs && (endedMs === null || endMs > endedMs)) endedMs = endMs;
      } else if (obs.timer.status === "stopped" && obs.timer.remaining_seconds === 0 && runActive) {
        endedMs = obs.now; // stopped at the very end: the dashboard saw it now
      }
    }
  }
  if (lastActiveMs === memory.lastActiveMs && firstSongMs === memory.firstSongMs && endedMs === memory.endedMs && memory === previous) return previous;
  return { planId: obs.planId, firstSongMs, endedMs, lastActiveMs };
}
