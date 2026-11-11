import type { PlaybackStatusResponse, ServicePlan, Song } from "../../types";
import { playbackOrderNotice } from "../playbackStatus";
import type { ReadinessCheck } from "./dashboardReadiness";
import type { PlanRunMemory } from "./planProgress";

/**
 * "Scan Playback again": a reminder, in the header status pill, that Playback should be scanned in the hour before the service
 * (owner, 2026-11-11: "I always playback scan right before the service because sometimes the timings get changed last minute").
 * A fresh scan also corrects a song order StagePilot only learned, and Playback's measured lengths feed the countdowns.
 *
 * The rule (everything in one place; the dashboard only supplies the data it already holds and its own clock):
 *  - Playback is the active source (enabled, connected, Playback API), is not playing, and is not in the middle of a scan;
 *  - a service plan is loaded and its service times are known. A time is "HH:MM" on `plan.date` in the saved time zone, the
 *    same zone Planning Center's times were read in (so daylight-saving changes are handled by the zone, not by arithmetic);
 *  - the plan is not going (see `planGoing`: from a service's start, or from the first song of a run-through, until the last
 *    song has ended);
 *  - the service is the NEXT one that has not started, however many there are (one, two, three; unsorted or repeated times
 *    are sorted and de-duplicated);
 *  - now is within the 60 minutes before that start (the start itself is already too late);
 *  - the last completed scan (`captured_at`) is older than the window start (start minus 60 minutes).
 * It clears when a scan completes, when the start passes, or when the plan starts going (a song plays, or a service begins).
 *
 * Several services: each later service that is still ahead gets its own reminder once the plan is over. A service less than
 * an hour after the one before it simply has its window open earlier; a scan made after that window opened (for instance
 * before the first service) covers both, and while the plan is going nothing is listed, so no second reminder is ever raised
 * in the middle of a service.
 *
 * A scan that is out of date for another reason (the setlist or the plan changed, or there is no order at all) already has its
 * own blocking notice (`playbackOrderNotice`); then this row stays away, so the same cause is never listed twice.
 */
export const SCAN_REMINDER_LABEL = "Scan Playback again";
export const SCAN_REMINDER_WINDOW_MS = 60 * 60 * 1000;
/** Time estimate for how long a plan goes when the app cannot tell when its last song ended: the song lengths plus this. */
export const PLAN_GOING_SLACK_MS = 30 * 60 * 1000;
/** The plan never counts as going longer than this after the service start or the first song, whatever else is true. */
export const PLAN_GOING_CAP_MS = 3 * 60 * 60 * 1000;

const DAY_MS = 24 * 60 * 60 * 1000;
const formatters = new Map<string, Intl.DateTimeFormat | null>();

function formatterFor(timeZone: string): Intl.DateTimeFormat | null {
  if (!formatters.has(timeZone)) {
    try {
      formatters.set(timeZone, new Intl.DateTimeFormat("en-US", {
        timeZone, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric",
      }));
    } catch {
      formatters.set(timeZone, null); // not a time zone this browser knows
    }
  }
  return formatters.get(timeZone) ?? null;
}

/** How far the zone's wall clock is ahead of UTC at the instant `at`, in ms (null for an unknown zone). */
function zoneOffsetMs(timeZone: string, at: number): number | null {
  const format = formatterFor(timeZone);
  if (!format) return null;
  const part: Partial<Record<string, number>> = {};
  for (const { type, value } of format.formatToParts(new Date(at))) if (type !== "literal") part[type] = Number(value);
  const wall = Date.UTC(part.year ?? 1970, (part.month ?? 1) - 1, part.day ?? 1, (part.hour ?? 0) % 24, part.minute ?? 0, part.second ?? 0);
  return wall - Math.floor(at / 1000) * 1000;
}

/**
 * The instant at which the wall clock in `timeZone` reads y-m-d h:mm. A time that happens twice (clocks go back) is the first
 * one; a time that never happens (clocks go forward) is moved ahead by the length of the gap, as a calendar does.
 */
export function zonedWallTimeToMs(timeZone: string, y: number, m: number, d: number, h: number, min: number): number | null {
  const wall = Date.UTC(y, m - 1, d, h, min);
  const before = zoneOffsetMs(timeZone, wall - DAY_MS);
  const after = zoneOffsetMs(timeZone, wall + DAY_MS);
  if (before === null || after === null) return null;
  const matches = [...new Set([before, after])]
    .map((offset) => wall - offset)
    .filter((instant) => zoneOffsetMs(timeZone, instant) === wall - instant);
  return matches.length ? Math.min(...matches) : wall - before;
}

/** The plan's service start times as instants, earliest first. Anything that is not a clock time is ignored. */
export function serviceStartsMs(plan: Pick<ServicePlan, "date" | "service_times">, timeZone: string): number[] {
  const day = /^(\d{4})-(\d{2})-(\d{2})/.exec(plan.date ?? "");
  if (!day) return [];
  const starts = new Set<number>();
  for (const text of plan.service_times ?? []) {
    const clock = /^(\d{1,2}):(\d{2})/.exec(text.trim());
    if (!clock || Number(clock[1]) > 23 || Number(clock[2]) > 59) continue;
    const instant = zonedWallTimeToMs(timeZone, Number(day[1]), Number(day[2]), Number(day[3]), Number(clock[1]), Number(clock[2]));
    if (instant !== null) starts.add(instant);
  }
  return [...starts].sort((a, b) => a - b);
}

export type ScanPlan = Pick<ServicePlan, "date" | "service_times"> & {
  id?: string;
  /** Only the lengths are read. A song without a length counts as 0 in the time estimate. */
  songs?: ReadonlyArray<Pick<Song, "duration_seconds">>;
};

/**
 * Is the loaded plan going right now? From a service's start time (or from the first song of a run-through, if that came
 * first) until the plan's last song has ended. `run` is what the dashboard saw of the songs (planProgress.ts).
 *
 *  - A run is in progress (a song was started and the last song has not ended since): the plan is going, between songs too.
 *    The end is the last song's own countdown reaching 0:00 (`run.endedMs`). When the last song has no length the app cannot
 *    tell, so the time estimate is used: the start plus the sum of the song lengths plus 30 minutes.
 *  - No song has been started for the latest service start that no finished run covers (the service began, or songs are not
 *    being tracked): the same time estimate.
 *  - The hard cap: never more than 3 hours after the service start or the first song, whichever came first.
 * A service that a finished run began after (the run-through ended before the start does not cover it) is over.
 */
export function planGoing(now: number, starts: readonly number[], songs: ScanPlan["songs"], run: PlanRunMemory | null | undefined): boolean {
  const durations = (songs ?? []).map((song) => Math.max(0, song.duration_seconds ?? 0));
  const totalMs = durations.reduce((sum, seconds) => sum + seconds, 0) * 1_000;
  const lastLength = durations[durations.length - 1] ?? 0;
  const ended = run?.endedMs ?? null;
  // Service starts already passed that no finished run has covered (a run covers the starts before it ended).
  const open = starts.filter((start) => start <= now && (ended === null || start > ended));
  const first = run?.firstSongMs ?? null;
  const active = first !== null && (ended === null || ended < first);
  if (active) {
    const anchor = Math.min(first, open[0] ?? first);
    if (now >= anchor + PLAN_GOING_CAP_MS) return false;
    return lastLength > 0 ? true : now < anchor + totalMs + PLAN_GOING_SLACK_MS;
  }
  const latest = open[open.length - 1];
  if (latest === undefined) return false;
  return now < latest + PLAN_GOING_CAP_MS && now < latest + totalMs + PLAN_GOING_SLACK_MS;
}

export type ScanReminderInput = {
  /** The dashboard's clock (ms since 1970). The app has no server clock; the song countdown uses this one too. */
  now: number;
  /** The loaded service plan, or null when none is loaded (the caller decides what "loaded" means). */
  plan: ScanPlan | null | undefined;
  /** The saved time zone setting (an IANA name). */
  timeZone: string | null | undefined;
  playback: PlaybackStatusResponse | null | undefined;
  /** A song's timer is running. */
  songRunning?: boolean;
  /** What the dashboard saw of the songs being played (see planProgress.ts); null before anything is known. */
  run?: PlanRunMemory | null;
};

export type ScanReminder = {
  /** The service start the reminder is about (ms). */
  startMs: number;
  /** Whole minutes left, rounded up, at least 1. */
  minutesLeft: number;
};

/** The reminder that is due right now, or null when none is. */
export function scanReminderDue({ now, plan, timeZone, playback, songRunning = false, run = null }: ScanReminderInput): ScanReminder | null {
  if (!plan || !timeZone || !playback) return null;
  if (!playback.enabled || !playback.connected || playback.active_source !== "playback_api") return null;
  if (songRunning || playback.playing || playback.discovery === "running") return null;
  if (playbackOrderNotice(playback)) return null; // its own notice already says the scan is out of date
  const starts = serviceStartsMs(plan, timeZone);
  const memory = run && (!plan.id || run.planId === plan.id) ? run : null; // memory of another plan says nothing about this one
  if (planGoing(now, starts, plan.songs, memory)) return null;
  const next = starts.find((start) => start > now);
  if (next === undefined || next - now > SCAN_REMINDER_WINDOW_MS) return null;
  const scannedAt = playback.captured_at ? Date.parse(playback.captured_at) : Number.NaN;
  if (Number.isFinite(scannedAt) && scannedAt >= next - SCAN_REMINDER_WINDOW_MS) return null;
  return { startMs: next, minutesLeft: Math.max(1, Math.ceil((next - now) / 60_000)) };
}

export function scanReminderCheck(input: ScanReminderInput): ReadinessCheck | null {
  const due = scanReminderDue(input);
  if (!due) return null;
  return {
    id: "scan-playback",
    label: SCAN_REMINDER_LABEL,
    passed: false,
    required: false,
    severity: "warning",
    status: "error",
    detail: `The service starts in ${due.minutesLeft} ${due.minutesLeft === 1 ? "minute" : "minutes"} and Playback has not been scanned in the hour before it. Scan it so song order and lengths are right.`,
  };
}
