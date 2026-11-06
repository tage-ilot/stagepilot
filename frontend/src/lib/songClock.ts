import type { ApplicationState, Song } from "../types";

/**
 * The Now playing card's two numbers, both derived from ONE position so they cannot disagree.
 *
 * - song length: the playing song's scheduled length. After a resume or seek the backend re-bases
 *   `timer.duration_seconds` to the time left, so it is only a fallback (`position + duration`).
 * - position: `timer.position_seconds` (where the song was at `timer.started_at`) plus the time
 *   since then while the timer runs; the frozen position (or length minus the frozen remaining
 *   time) when it is stopped.
 *
 * `remaining` counts down in whole seconds (rounded up, so it shows the second that is still
 * running) and `elapsed` is the song length minus that, so `elapsed + remaining === length`
 * always. Past the end (the ProPresenter countdown overruns into a negative count-up, which
 * StagePilot's own clock deliberately does not show) remaining stays 00:00 and elapsed stays at
 * the song length.
 */
export interface SongClock {
  length: number;
  elapsed: number;
  remaining: number;
}

export function songClock(
  timer: Pick<ApplicationState["timer"], "status" | "duration_seconds" | "started_at" | "position_seconds" | "remaining_seconds">,
  playingSong: Pick<Song, "duration_seconds"> | null | undefined,
  nowMs: number,
): SongClock {
  const startPosition = Math.max(0, timer.position_seconds ?? 0);
  const length = Math.max(0, playingSong?.duration_seconds ?? (startPosition + (timer.duration_seconds ?? 0)));
  const clamp = (seconds: number) => Math.min(length, Math.max(0, seconds));

  let remaining: number;
  if (timer.status === "running" && timer.started_at) {
    const since = Math.max(0, nowMs - Date.parse(timer.started_at)) / 1_000;
    remaining = length - clamp(Math.ceil(startPosition + since));
  } else if (timer.status === "stopped" && timer.remaining_seconds != null) {
    // Stop keeps the remaining time: a stopped countdown shows the frozen value instead of blanking.
    remaining = clamp(timer.remaining_seconds);
  } else if (timer.status === "stopped" && timer.position_seconds != null) {
    remaining = length - clamp(Math.round(startPosition));
  } else {
    remaining = length;
  }
  return { length, remaining, elapsed: length - remaining };
}
