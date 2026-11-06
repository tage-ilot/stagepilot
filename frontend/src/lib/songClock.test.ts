import { describe, expect, it } from "vitest";
import { songClock } from "./songClock";

const LENGTH = 336; // 05:36, the length in the rig screenshot
const song = { duration_seconds: LENGTH };
const T0 = Date.parse("2026-07-13T16:00:00.000Z");
const iso = (offsetMs: number) => new Date(T0 + offsetMs).toISOString();
const running = (startedOffsetMs: number, positionSeconds: number | null, durationSeconds: number | null) => ({
  status: "running" as const,
  started_at: iso(startedOffsetMs),
  position_seconds: positionSeconds,
  duration_seconds: durationSeconds,
  remaining_seconds: null,
});
const stopped = (positionSeconds: number | null, remainingSeconds: number | null) => ({
  status: "stopped" as const,
  started_at: null,
  position_seconds: positionSeconds,
  duration_seconds: null,
  remaining_seconds: remainingSeconds,
});

function expectConsistent(clock: ReturnType<typeof songClock>) {
  expect(clock.elapsed + clock.remaining).toBe(clock.length);
  expect(clock.elapsed).toBeGreaterThanOrEqual(0);
  expect(clock.remaining).toBeGreaterThanOrEqual(0);
}

describe("songClock: elapsed and remaining always add up to the song length", () => {
  it("fresh start", () => {
    const timer = running(0, 0, LENGTH);
    const at0 = songClock(timer, song, T0);
    expect(at0).toEqual({ length: LENGTH, elapsed: 0, remaining: LENGTH });
    const at30 = songClock(timer, song, T0 + 30_000);
    expect(at30).toEqual({ length: LENGTH, elapsed: 30, remaining: 306 });
    // Part way through a second the running second counts as spent, like the countdown.
    expect(songClock(timer, song, T0 + 30_400)).toEqual({ length: LENGTH, elapsed: 31, remaining: 305 });
    for (let ms = 0; ms <= 400_000; ms += 777) expectConsistent(songClock(timer, song, T0 + ms));
  });

  it("a start back-dated by the Playback position counts from that position", () => {
    // started_at is 0.57 s before the event was handled
    const timer = running(-570, 0, LENGTH);
    expect(songClock(timer, song, T0).elapsed).toBe(1);
    expect(songClock(timer, song, T0 + 29_430)).toEqual({ length: LENGTH, elapsed: 30, remaining: 306 });
  });

  it("pause freezes both numbers", () => {
    const clock = songClock(stopped(69.4, 267), song, T0 + 999_000);
    expect(clock).toEqual({ length: LENGTH, elapsed: 69, remaining: 267 });
    expectConsistent(clock);
  });

  it("pause with only a position (no remaining) still adds up", () => {
    const clock = songClock(stopped(69.4, null), song, T0);
    expect(clock).toEqual({ length: LENGTH, elapsed: 69, remaining: 267 });
  });

  it("resume re-bases the timer to the remaining time but elapsed stays the song position (rig screenshot)", () => {
    // Resumed at 2:12 into the 5:36 song: backend says duration 204 s (03:24 left), position 132.
    const timer = running(0, 132, 204);
    const clock = songClock(timer, song, T0 + 69_000);
    // One minute nine after the resume: 3:24 -> 2:15 left; elapsed 2:12 + 1:09 = 3:21
    expect(clock).toEqual({ length: LENGTH, elapsed: 201, remaining: 135 });
    expectConsistent(clock);
    // The old formula showed elapsed 01:09 here.
    expect(clock.elapsed).not.toBe(69);
  });

  it("seek forward", () => {
    const timer = running(0, 200, 136);
    expect(songClock(timer, song, T0)).toEqual({ length: LENGTH, elapsed: 200, remaining: 136 });
    expect(songClock(timer, song, T0 + 10_000)).toEqual({ length: LENGTH, elapsed: 210, remaining: 126 });
  });

  it("seek back", () => {
    const timer = running(0, 20, 316);
    expect(songClock(timer, song, T0)).toEqual({ length: LENGTH, elapsed: 20, remaining: 316 });
    expect(songClock(timer, song, T0 + 5_000)).toEqual({ length: LENGTH, elapsed: 25, remaining: 311 });
  });

  it("seek while stopped holds the new position", () => {
    expect(songClock(stopped(250, 86), song, T0)).toEqual({ length: LENGTH, elapsed: 250, remaining: 86 });
  });

  it("stopped at the very end", () => {
    expect(songClock(stopped(LENGTH, 0), song, T0)).toEqual({ length: LENGTH, elapsed: LENGTH, remaining: 0 });
  });

  it("idle shows nothing elapsed", () => {
    const idle = { status: "idle" as const, started_at: null, position_seconds: null, duration_seconds: null, remaining_seconds: null };
    expect(songClock(idle, song, T0)).toEqual({ length: LENGTH, elapsed: 0, remaining: LENGTH });
    expect(songClock(idle, null, T0)).toEqual({ length: 0, elapsed: 0, remaining: 0 });
  });

  it("overrun past the end: remaining stays 00:00 and elapsed stays at the song length, still adding up", () => {
    const timer = running(0, 0, LENGTH);
    for (const extra of [0, 1_000, 5_500, 120_000]) {
      const clock = songClock(timer, song, T0 + LENGTH * 1_000 + extra);
      expect(clock).toEqual({ length: LENGTH, elapsed: LENGTH, remaining: 0 });
    }
    // and after a resume near the end
    const resumed = running(0, 330, 6);
    expect(songClock(resumed, song, T0 + 60_000)).toEqual({ length: LENGTH, elapsed: LENGTH, remaining: 0 });
  });

  it("falls back to position + duration when the song is unknown", () => {
    const timer = running(0, 132, 204);
    expect(songClock(timer, null, T0 + 10_000)).toEqual({ length: 336, elapsed: 142, remaining: 194 });
  });

  it("a clock set back (negative time since start) does not go below zero", () => {
    const timer = running(5_000, 0, LENGTH);
    expect(songClock(timer, song, T0)).toEqual({ length: LENGTH, elapsed: 0, remaining: LENGTH });
  });
});
