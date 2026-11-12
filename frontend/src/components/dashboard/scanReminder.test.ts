import { describe, expect, it } from "vitest";

import type { PlaybackStatusResponse } from "../../types";
import { playbackStatus } from "../../test/playbackFixtures";
import {
  SCAN_REMINDER_LABEL,
  SCAN_REMINDER_WINDOW_MS,
  scanReminderCheck,
  scanReminderDue,
  serviceStartsMs,
  zonedWallTimeToMs,
  type ScanReminderInput,
} from "./scanReminder";

const MIN = 60_000;
const LA = "America/Los_Angeles";
const at = (iso: string) => Date.parse(iso);

// Sunday 2026-11-15 09:00 in Los Angeles is 17:00 UTC (standard time, UTC-8).
const SUNDAY_PLAN = { date: "2026-11-15", service_times: ["09:00"] };
const START = at("2026-11-15T17:00:00Z");
const before = (minutes: number) => START - minutes * MIN;

/** A scanned Playback with a saved order, unless a test changes it. `scannedAgo` counts minutes before the service start. */
const scanned = (minutesBeforeStart: number | null, extra: Partial<PlaybackStatusResponse> = {}) =>
  playbackStatus({
    discovery: "done",
    order_mode: "scanned",
    captured_at: minutesBeforeStart === null ? null : new Date(START - minutesBeforeStart * MIN).toISOString(),
    ...extra,
  });

const input = (now: number, overrides: Partial<ScanReminderInput> = {}): ScanReminderInput => ({
  now, plan: SUNDAY_PLAN, timeZone: LA, playback: scanned(180), songRunning: false, ...overrides,
});
const due = (now: number, overrides: Partial<ScanReminderInput> = {}) => scanReminderDue(input(now, overrides));

describe("scan reminder window (F-S1)", () => {
  it("is not due 61 minutes before the service, and is from 60 minutes before", () => {
    expect(due(before(61))).toBeNull();
    expect(due(before(60) - 1)).toBeNull();
    expect(due(before(60))).toMatchObject({ startMs: START, minutesLeft: 60 });
  });

  it("is due 30 and 1 minutes before, counting whole minutes up", () => {
    expect(due(before(30))).toMatchObject({ minutesLeft: 30 });
    expect(due(before(1))).toMatchObject({ minutesLeft: 1 });
    expect(due(START - 10_000)).toMatchObject({ minutesLeft: 1 });
    expect(due(before(42) + 20_000)).toMatchObject({ minutesLeft: 42 });
  });

  it("is gone from the start time on, and for good after it", () => {
    expect(due(START)).toBeNull();
    expect(due(START + 1)).toBeNull();
    expect(due(START + 30 * MIN)).toBeNull();
    expect(due(START + 6 * 60 * MIN)).toBeNull();
  });

  it("needs a scan since the window opened: one before the window does not count, one at or inside it does", () => {
    expect(due(before(30), { playback: scanned(61) })).not.toBeNull();
    expect(due(before(30), { playback: scanned(60.001) })).not.toBeNull();
    expect(due(before(30), { playback: scanned(60) })).toBeNull();
    expect(due(before(30), { playback: scanned(45) })).toBeNull();
    expect(due(before(30), { playback: scanned(31) })).toBeNull();
    // The same scan, 70 minutes before the start, is old news once the hour before the service has begun.
    expect(due(before(80), { playback: scanned(70) })).toBeNull();
    expect(due(before(50), { playback: scanned(70) })).not.toBeNull();
  });

  it("treats no scan, or an unreadable scan time, as not scanned", () => {
    expect(due(before(30), { playback: scanned(null, { song_order: [] }) })).toBeNull(); // no order at all has its own notice
    expect(due(before(30), { playback: scanned(null, { order_mode: "learned", learned_songs: [] }) })).not.toBeNull();
    expect(due(before(30), { playback: scanned(10, { captured_at: "yesterday-ish", order_mode: "learned" }) })).not.toBeNull();
  });

  it("a scan that finishes while the page is open makes the row go away on the next look", () => {
    let status = scanned(180);
    const now = before(25);
    expect(scanReminderCheck(input(now, { playback: status }))?.id).toBe("scan-playback");
    status = { ...status, discovery: "running" };
    expect(scanReminderCheck(input(now, { playback: status }))).toBeNull(); // scanning right now: nothing to remind of
    status = { ...status, discovery: "done", captured_at: new Date(now + 20_000).toISOString() };
    expect(scanReminderCheck(input(now + 30_000, { playback: status }))).toBeNull();
    // ... and it stays gone as the clock runs on to the start.
    for (const minutes of [20, 10, 5, 1]) expect(scanReminderCheck(input(before(minutes), { playback: status }))).toBeNull();
  });

  it("counts down minute by minute with the clock until the start", () => {
    const text = (minutes: number) => scanReminderCheck(input(before(minutes)))?.detail;
    expect(text(42)).toContain("The service starts in 42 minutes and Playback has not been scanned in the hour before it.");
    expect(text(2)).toContain("starts in 2 minutes");
    expect(text(1)).toContain("starts in 1 minute and");
    expect(text(0)).toBeUndefined();
  });
});

describe("what has to be true before it is listed", () => {
  const now = before(30);

  it("needs a loaded plan with a known service time", () => {
    expect(due(now, { plan: null })).toBeNull();
    expect(due(now, { plan: undefined })).toBeNull();
    expect(due(now, { plan: { date: "2026-11-15", service_times: [] } })).toBeNull();
    expect(due(now, { plan: { date: "2026-11-15", service_times: ["soon", "25:00", "09:75", ""] } })).toBeNull();
    expect(due(now, { plan: { date: "not a date", service_times: ["09:00"] } })).toBeNull();
  });

  it("needs Playback enabled, connected and the active source", () => {
    expect(due(now)).not.toBeNull();
    expect(due(now, { playback: null })).toBeNull();
    expect(due(now, { playback: undefined })).toBeNull();
    expect(due(now, { playback: scanned(180, { enabled: false }) })).toBeNull();
    expect(due(now, { playback: scanned(180, { connected: false }) })).toBeNull();
    expect(due(now, { playback: scanned(180, { active_source: "midi" }) })).toBeNull();
    expect(due(now, { playback: scanned(180, { active_source: "none" }) })).toBeNull();
  });

  it("stays away while a song plays (Playback's transport or StagePilot's timer)", () => {
    expect(due(now, { playback: scanned(180, { playing: true }) })).toBeNull();
    expect(due(now, { songRunning: true })).toBeNull();
    expect(due(now, { songRunning: false, playback: scanned(180, { playing: false }) })).not.toBeNull();
  });

  it("needs a time zone", () => {
    expect(due(now, { timeZone: undefined })).toBeNull();
    expect(due(now, { timeZone: "" })).toBeNull();
    expect(due(now, { timeZone: "Mars/Olympus_Mons" })).toBeNull();
  });

  it("does not add a second row when the scan is already called out of date: the plan or the setlist changed, or there is no order", () => {
    expect(due(now, { playback: scanned(180, { plan_changed: true }) })).toBeNull();
    expect(due(now, { playback: scanned(180, { stale: true }) })).toBeNull();
    const noOrder = { song_order: [], order_mode: "none" as const, discovery: "idle" as const };
    expect(due(now, { playback: scanned(null, { ...noOrder, settings: { ...playbackStatus().settings, learn_order: false } }) })).toBeNull();
    // With learning on, "no order yet" is not a notice (the order is learned as songs play), so a scan is what is missing.
    expect(due(now, { playback: scanned(null, noOrder) })).not.toBeNull();
    // A learned order has no such notice (it counts down fine), so the reminder does apply and says to scan.
    expect(due(now, { playback: scanned(null, { order_mode: "learned", learned_songs: [], plan_changed: true }) })).not.toBeNull();
  });

  it("the row is an optional warning: never blocking, no Optional wording", () => {
    const check = scanReminderCheck(input(now))!;
    expect(check).toMatchObject({ id: "scan-playback", label: SCAN_REMINDER_LABEL, required: false, severity: "warning", status: "error", passed: false });
    expect(check.label).toBe("Scan Playback again");
    expect(`${check.label} ${check.detail}`).not.toMatch(/optional/i);
    expect(check.detail).toBe("The service starts in 30 minutes and Playback has not been scanned in the hour before it. Scan it so song order and lengths are right.");
  });
});

describe("several service times on the plan's day", () => {
  const plan = { date: "2026-11-15", service_times: ["11:00", "09:00", "09:00"] }; // 17:00Z and 19:00Z, unsorted, one twice
  const second = at("2026-11-15T19:00:00Z");
  const run = (now: number, capturedAt: string | null) =>
    scanReminderDue({ now, plan, timeZone: LA, playback: playbackStatus({ discovery: "done", order_mode: "scanned", captured_at: capturedAt }) });

  it("each service gets its own reminder: the next one that has not started", () => {
    expect(run(START - 30 * MIN, null)).toMatchObject({ startMs: START, minutesLeft: 30 });
    expect(run(START + 30 * MIN, null)).toBeNull(); // first has started; the second is 90 minutes away
    expect(run(second - 45 * MIN, null)).toMatchObject({ startMs: second, minutesLeft: 45 });
    expect(run(second, null)).toBeNull();
  });

  it("a scan counts for the service it was made for, not the next one", () => {
    const earlyScan = new Date(START - 20 * MIN).toISOString();
    expect(run(START - 10 * MIN, earlyScan)).toBeNull();
    expect(run(second - 45 * MIN, earlyScan)).not.toBeNull(); // scanned for the 9:00; the 11:00 window opens at 10:00
    expect(run(second - 45 * MIN, new Date(second - 50 * MIN).toISOString())).toBeNull();
  });

  it("services less than an hour apart each have their own window; while the plan goes nothing is listed (F-S2)", () => {
    const close = { date: "2026-11-15", service_times: ["09:00", "09:30", "11:00"] };
    const check = (now: number) => scanReminderDue({ now, plan: close, timeZone: LA, playback: playbackStatus({ discovery: "done", order_mode: "scanned", captured_at: null }) });
    expect(check(START - 30 * MIN)).toMatchObject({ startMs: START });
    expect(check(START + 10 * MIN)).toBeNull(); // the plan is going (no song lengths: start + 30 minutes); 11:00 is still far away
    expect(check(START + 31 * MIN)).toBeNull(); // the plan is over; 11:00 is an hour and 29 minutes away
    expect(check(at("2026-11-15T19:00:00Z") - 59 * MIN)).toMatchObject({ startMs: at("2026-11-15T19:00:00Z") });
    const learned = playbackStatus({ discovery: "done", order_mode: "learned", captured_at: null });
    const pair = (now: number, second: string) => scanReminderDue({ now, plan: { date: "2026-11-15", service_times: ["09:00", second] }, timeZone: LA, playback: learned });
    expect(pair(START + 5 * MIN, "09:59")).toBeNull(); // the first service is going
    expect(pair(START + 5 * MIN, "10:00")).toBeNull();
    expect(pair(START + 35 * MIN, "09:59")).toMatchObject({ startMs: START + 59 * MIN }); // the first is over, the second is 24 minutes away
    expect(pair(START + 35 * MIN, "10:00")).toMatchObject({ startMs: START + 60 * MIN });
  });
});

describe("time zones and daylight saving", () => {
  it("reads the service time in the plan's zone", () => {
    const plan = { date: "2026-11-15", service_times: ["09:00"] };
    const run = (now: number, timeZone: string) => scanReminderDue({ now, plan, timeZone, playback: scanned(null, { order_mode: "learned" }) });
    expect(run(at("2026-11-15T13:30:00Z"), "America/New_York")).toMatchObject({ startMs: at("2026-11-15T14:00:00Z"), minutesLeft: 30 });
    expect(run(at("2026-11-15T13:30:00Z"), LA)).toBeNull(); // 09:00 in LA is 17:00Z
    expect(run(at("2026-11-15T08:30:00Z"), "Europe/London")).toMatchObject({ minutesLeft: 30 });
    expect(run(at("2026-11-15T03:00:00Z"), "Asia/Kolkata")).toMatchObject({ startMs: at("2026-11-15T03:30:00Z"), minutesLeft: 30 }); // UTC+5:30
  });

  it("uses summer time or winter time as the zone says", () => {
    expect(serviceStartsMs({ date: "2026-07-12", service_times: ["09:00"] }, LA)).toEqual([at("2026-07-12T16:00:00Z")]);
    expect(serviceStartsMs({ date: "2026-12-13", service_times: ["09:00"] }, LA)).toEqual([at("2026-12-13T17:00:00Z")]);
    expect(serviceStartsMs({ date: "2026-12-13", service_times: ["09:00"] }, "Australia/Sydney")).toEqual([at("2026-12-12T22:00:00Z")]);
  });

  it("clocks going forward: a 03:30 service sees the window open 60 real minutes before, at 01:30 standard time", () => {
    const plan = { date: "2027-03-14", service_times: ["03:30"] }; // 02:00 PST jumps to 03:00 PDT; 03:30 PDT is 10:30Z
    const start = at("2027-03-14T10:30:00Z");
    expect(serviceStartsMs(plan, LA)).toEqual([start]);
    const run = (now: number, captured: string | null = null) =>
      scanReminderDue({ now, plan, timeZone: LA, playback: scanned(null, { order_mode: "learned", captured_at: captured }) });
    expect(run(start - 61 * MIN)).toBeNull(); // 01:29 PST
    expect(run(start - 60 * MIN)).toMatchObject({ minutesLeft: 60 }); // 01:30 PST, not 02:30 on the wall
    expect(run(start - 20 * MIN)).toMatchObject({ minutesLeft: 20 });
    expect(run(start - 20 * MIN, "2027-03-14T09:45:00Z")).toBeNull(); // 01:45 PST scan is inside the window
    expect(run(start - 20 * MIN, "2027-03-14T09:29:00Z")).not.toBeNull(); // 01:29 PST scan is not
  });

  it("clocks going back: the window is 60 real minutes, not 60 minutes of the repeated hour", () => {
    const plan = { date: "2026-11-01", service_times: ["02:30"] }; // 02:30 PST = 10:30Z; 01:00-02:00 happened twice that night
    const start = at("2026-11-01T10:30:00Z");
    expect(serviceStartsMs(plan, LA)).toEqual([start]);
    const run = (now: number, captured: string | null = null) =>
      scanReminderDue({ now, plan, timeZone: LA, playback: scanned(null, { order_mode: "learned", captured_at: captured }) });
    expect(run(at("2026-11-01T09:29:00Z"))).toBeNull(); // 01:29 PST (the second 01:29)
    expect(run(at("2026-11-01T09:30:00Z"))).toMatchObject({ minutesLeft: 60 });
    // A scan from the first pass through 01:45 (PDT, 08:45Z) is 105 real minutes old and does not count.
    expect(run(at("2026-11-01T09:40:00Z"), "2026-11-01T08:45:00Z")).not.toBeNull();
    expect(run(at("2026-11-01T09:40:00Z"), "2026-11-01T09:35:00Z")).toBeNull();
    const twice = { date: "2026-11-01", service_times: ["01:30"] }; // happens twice: the first (PDT, 08:30Z) is taken
    expect(serviceStartsMs(twice, LA)).toEqual([at("2026-11-01T08:30:00Z")]);
  });

  it("a time that does not exist (02:30 on the spring day) moves forward by the gap; a time that happens twice is the first", () => {
    expect(zonedWallTimeToMs(LA, 2027, 3, 14, 2, 30)).toBe(at("2027-03-14T10:30:00Z"));
    expect(zonedWallTimeToMs(LA, 2026, 11, 1, 1, 30)).toBe(at("2026-11-01T08:30:00Z"));
    expect(zonedWallTimeToMs(LA, 2026, 11, 1, 2, 30)).toBe(at("2026-11-01T10:30:00Z"));
    expect(zonedWallTimeToMs("UTC", 2026, 11, 15, 9, 0)).toBe(at("2026-11-15T09:00:00Z"));
    expect(zonedWallTimeToMs("Nope/Zone", 2026, 11, 15, 9, 0)).toBeNull();
  });

  it("the window is exactly 60 real minutes", () => {
    expect(SCAN_REMINDER_WINDOW_MS).toBe(60 * MIN);
  });
});
