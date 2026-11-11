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
 * A new run starts when a countdown starts after `endedMs`; nothing needs resetting.
 */
export type PlanRunMemory = {
  planId: string;
  firstSongMs: number | null;
  endedMs: number | null;
};

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
  const memory: PlanRunMemory = previous && previous.planId === obs.planId ? previous : { planId: obs.planId, firstSongMs: null, endedMs: null };
  const lastIndex = obs.durations.length - 1;
  const started = obs.timer.started_at ? Date.parse(obs.timer.started_at) : Number.NaN;
  const running = obs.timer.status === "running" && Number.isFinite(started);
  const songStartMs = running ? started - Math.max(0, obs.timer.position_seconds ?? 0) * 1_000 : null;
  const index = obs.playingIndex ?? null;
  let { firstSongMs, endedMs } = memory;

  if (lastIndex >= 0 && (index !== null || running)) {
    // A countdown that starts after the last run ended begins a new run; otherwise the run in progress keeps its first song.
    if (firstSongMs === null || (songStartMs !== null && endedMs !== null && songStartMs > endedMs)) {
      firstSongMs = songStartMs ?? obs.now; // seen between songs after a page reload: the best we know is now
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
  if (firstSongMs === memory.firstSongMs && endedMs === memory.endedMs && memory === previous) return previous;
  return { planId: obs.planId, firstSongMs, endedMs };
}
