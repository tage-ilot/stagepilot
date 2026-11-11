import type { PlaybackStatusResponse, ServicePlan } from "../../types";
import { playbackOrderNotice } from "../playbackStatus";
import type { ReadinessCheck } from "./dashboardReadiness";

/**
 * "Scan Playback again": a reminder, in the header status pill, that Playback should be scanned in the hour before the service
 * (owner, 2026-11-11: "I always playback scan right before the service because sometimes the timings get changed last minute").
 * A fresh scan also corrects a song order StagePilot only learned, and Playback's measured lengths feed the countdowns.
 *
 * The rule (everything in one place; the dashboard only supplies the data it already holds and its own clock):
 *  - Playback is the active source (enabled, connected, Playback API), is not playing, and is not in the middle of a scan;
 *  - a service plan is loaded and its service times are known. A time is "HH:MM" on `plan.date` in the saved time zone, the
 *    same zone Planning Center's times were read in (so daylight-saving changes are handled by the zone, not by arithmetic);
 *  - the service is the NEXT one that has not started. Services less than an hour after the one before it share its reminder
 *    (there is no time to scan between them), so they get none of their own;
 *  - now is within the 60 minutes before that start (the start itself is already too late);
 *  - the last completed scan (`captured_at`) is older than the window start (start minus 60 minutes).
 * It clears when a scan completes, when the start passes, or when a song plays.
 *
 * A scan that is out of date for another reason (the setlist or the plan changed, or there is no order at all) already has its
 * own blocking notice (`playbackOrderNotice`); then this row stays away, so the same cause is never listed twice.
 */
export const SCAN_REMINDER_LABEL = "Scan Playback again";
export const SCAN_REMINDER_WINDOW_MS = 60 * 60 * 1000;

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

export type ScanReminderInput = {
  /** The dashboard's clock (ms since 1970). The app has no server clock; the song countdown uses this one too. */
  now: number;
  /** The loaded service plan, or null when none is loaded (the caller decides what "loaded" means). */
  plan: Pick<ServicePlan, "date" | "service_times"> | null | undefined;
  /** The saved time zone setting (an IANA name). */
  timeZone: string | null | undefined;
  playback: PlaybackStatusResponse | null | undefined;
  /** A song's timer is running. */
  songRunning?: boolean;
};

export type ScanReminder = {
  /** The service start the reminder is about (ms). */
  startMs: number;
  /** Whole minutes left, rounded up, at least 1. */
  minutesLeft: number;
};

/** The reminder that is due right now, or null when none is. */
export function scanReminderDue({ now, plan, timeZone, playback, songRunning = false }: ScanReminderInput): ScanReminder | null {
  if (!plan || !timeZone || !playback) return null;
  if (!playback.enabled || !playback.connected || playback.active_source !== "playback_api") return null;
  if (songRunning || playback.playing || playback.discovery === "running") return null;
  if (playbackOrderNotice(playback)) return null; // its own notice already says the scan is out of date
  const starts = serviceStartsMs(plan, timeZone);
  const next = starts.find((start, index) => start > now && (index === 0 || start - (starts[index - 1] ?? 0) >= SCAN_REMINDER_WINDOW_MS));
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
