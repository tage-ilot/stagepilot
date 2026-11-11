import { describe, expect, it } from "vitest";

import { observePlanRun, PLAN_IDLE_MS, type PlanRunMemory, type PlanRunObservation } from "./planProgress";

const MIN = 60_000;
const BASE = Date.parse("2026-11-15T18:00:00Z");
const iso = (ms: number) => new Date(ms).toISOString();
const SONGS = [300, 300, 300];

const obs = (now: number, over: Partial<PlanRunObservation> = {}): PlanRunObservation => ({
  now, planId: "p1", durations: SONGS, playingIndex: null,
  timer: { status: "idle", started_at: null }, ...over,
});
const running = (startMs: number, position = 0): PlanRunObservation["timer"] => ({ status: "running", started_at: iso(startMs), position_seconds: position });
const stopped = (remaining: number): PlanRunObservation["timer"] => ({ status: "stopped", started_at: null, remaining_seconds: remaining, position_seconds: 300 - remaining });

describe("what the dashboard remembers of the plan being played (F-S2)", () => {
  it("knows nothing before a song is started, and nothing without a plan", () => {
    expect(observePlanRun(null, obs(BASE))).toEqual({ planId: "p1", firstSongMs: null, endedMs: null, lastActiveMs: null });
    expect(observePlanRun(null, obs(BASE, { planId: null }))).toBeNull();
    expect(observePlanRun({ planId: "p1", firstSongMs: BASE, endedMs: null, lastActiveMs: BASE }, obs(BASE, { planId: undefined }))).toBeNull();
  });

  it("remembers the first song's start and keeps it as later songs start", () => {
    let memory = observePlanRun(null, obs(BASE + MIN, { playingIndex: 0, timer: running(BASE + MIN) }));
    expect(memory).toEqual({ planId: "p1", firstSongMs: BASE + MIN, endedMs: null, lastActiveMs: BASE + MIN });
    memory = observePlanRun(memory, obs(BASE + 5 * MIN, { playingIndex: 0, timer: running(BASE + MIN) }));
    memory = observePlanRun(memory, obs(BASE + 8 * MIN, { playingIndex: 1, timer: running(BASE + 7 * MIN) }));
    expect(memory).toEqual({ planId: "p1", firstSongMs: BASE + MIN, endedMs: null, lastActiveMs: BASE + 8 * MIN });
  });

  it("a resumed song counts from where the song began (position), not from the resume", () => {
    const memory = observePlanRun(null, obs(BASE + 5 * MIN, { playingIndex: 0, timer: running(BASE + 4 * MIN, 120) }));
    expect(memory?.firstSongMs).toBe(BASE + 2 * MIN);
  });

  it("the last song ends when its countdown reaches 0:00, whether or not the timer stops", () => {
    const start = BASE + 20 * MIN;
    let memory = observePlanRun(null, obs(start, { playingIndex: 2, timer: running(start) }));
    expect(memory?.endedMs).toBeNull();
    memory = observePlanRun(memory, obs(start + 4 * MIN + 59_000, { playingIndex: 2, timer: running(start) }));
    expect(memory?.endedMs).toBeNull();
    memory = observePlanRun(memory, obs(start + 5 * MIN, { playingIndex: 2, timer: running(start) }));
    expect(memory?.endedMs).toBe(start + 5 * MIN);
    // A countdown that resumed at 2:00 into the song ends 3 minutes after the resume.
    const resumed = observePlanRun(null, obs(start + 2 * MIN, { playingIndex: 2, timer: running(start, 120) }));
    expect(resumed?.endedMs).toBeNull();
    expect(observePlanRun(resumed, obs(start + 3 * MIN + 1, { playingIndex: 2, timer: running(start, 120) }))?.endedMs).toBe(start + 3 * MIN);
  });

  it("a last song stopped at 0 seconds remaining is over; stopped part-way, or a song that is not the last, is not", () => {
    const first = observePlanRun(null, obs(BASE, { playingIndex: 2, timer: stopped(0) }));
    expect(first).toEqual({ planId: "p1", firstSongMs: BASE, endedMs: BASE, lastActiveMs: BASE });
    expect(observePlanRun(null, obs(BASE, { playingIndex: 2, timer: stopped(90) }))?.endedMs).toBeNull();
    expect(observePlanRun(null, obs(BASE, { playingIndex: 1, timer: stopped(0) }))?.endedMs).toBeNull();
  });

  it("does not claim an end when the last song has no length", () => {
    const start = BASE;
    const memory = observePlanRun(null, obs(start + 30 * MIN, { durations: [300, 300, null], playingIndex: 2, timer: running(start) }));
    expect(memory?.endedMs).toBeNull();
    expect(observePlanRun(null, obs(BASE, { durations: [300, 300, 0], playingIndex: 2, timer: stopped(0) }))?.endedMs).toBeNull();
  });

  it("a countdown started after the end begins a new run; a stopped end is not dated again", () => {
    let memory: PlanRunMemory | null = observePlanRun(null, obs(BASE, { playingIndex: 2, timer: stopped(0) }));
    const same = observePlanRun(memory, obs(BASE + 10 * MIN, { playingIndex: 2, timer: stopped(0) }));
    expect(same).toBe(memory); // nothing changed: the same object, so no re-render
    memory = observePlanRun(memory, obs(BASE + 62 * MIN, { playingIndex: 0, timer: running(BASE + 60 * MIN) }));
    expect(memory).toEqual({ planId: "p1", firstSongMs: BASE + 60 * MIN, endedMs: BASE, lastActiveMs: BASE + 62 * MIN });
  });

  it("another plan starts from nothing", () => {
    const old: PlanRunMemory = { planId: "p1", firstSongMs: BASE, endedMs: BASE + 10 * MIN, lastActiveMs: BASE + 10 * MIN };
    expect(observePlanRun(old, obs(BASE + 5 * MIN, { planId: "p2" }))).toEqual({ planId: "p2", firstSongMs: null, endedMs: null, lastActiveMs: null });
  });
});

describe("the last song activity (F-S3)", () => {
  it("follows a running countdown up to its 0:00 and stays there when the song stops or runs on", () => {
    const start = BASE;
    let memory = observePlanRun(null, obs(start + 30_000, { playingIndex: 0, timer: running(start) }));
    expect(memory?.lastActiveMs).toBe(start + 30_000);
    memory = observePlanRun(memory, obs(start + 4 * MIN, { playingIndex: 0, timer: running(start) }));
    expect(memory?.lastActiveMs).toBe(start + 4 * MIN);
    // The song is paused part-way: the last time it was seen running is kept.
    memory = observePlanRun(memory, obs(start + 6 * MIN, { playingIndex: 0, timer: stopped(60) }));
    expect(memory?.lastActiveMs).toBe(start + 4 * MIN);
    // A countdown that runs on past 0:00 (Presentation timer) counts as activity only until 0:00.
    const long = observePlanRun(memory, obs(start + 20 * MIN, { playingIndex: 0, timer: running(start + 10 * MIN) }));
    expect(long?.lastActiveMs).toBe(start + 15 * MIN);
    const overrun = observePlanRun(null, obs(start + 30 * MIN, { playingIndex: 0, timer: running(start) }));
    expect(overrun?.lastActiveMs).toBe(start + 5 * MIN);
  });

  it("a song stopped at 0:00 that was never seen running is dated when first seen, and not moved by later looks", () => {
    const first = observePlanRun(null, obs(BASE, { playingIndex: 0, timer: stopped(0) }));
    expect(first?.lastActiveMs).toBe(BASE);
    expect(observePlanRun(first, obs(BASE + 20 * MIN, { playingIndex: 0, timer: stopped(0) }))).toBe(first);
  });

  it("a countdown that starts more than 5 minutes after the last activity begins a new run; a shorter gap does not", () => {
    const first = observePlanRun(null, obs(BASE + 4 * MIN, { playingIndex: 0, timer: running(BASE) })); // active until 04:00
    const soon = observePlanRun(first, obs(BASE + 8 * MIN, { playingIndex: 1, timer: running(BASE + 7 * MIN) }));
    expect(soon?.firstSongMs).toBe(BASE); // 3 minutes of talk: the same run
    const late = observePlanRun(first, obs(BASE + 11 * MIN, { playingIndex: 1, timer: running(BASE + 10 * MIN) }));
    expect(late?.firstSongMs).toBe(BASE + 10 * MIN); // 6 minutes of quiet: a new run
    expect(PLAN_IDLE_MS).toBe(5 * MIN);
  });

  it("one song that kept playing while the page was asleep is the same run", () => {
    const first = observePlanRun(null, obs(BASE + MIN, { playingIndex: 0, timer: running(BASE) }));
    const later = observePlanRun(first, obs(BASE + 4 * MIN, { playingIndex: 0, timer: running(BASE) }));
    expect(later?.firstSongMs).toBe(BASE);
    expect(later?.lastActiveMs).toBe(BASE + 4 * MIN);
  });
});
