import { describe, expect, it } from "vitest";

import type { ApplicationState } from "../../types";
import { playbackStatus } from "../../test/playbackFixtures";
import { observePlanRun, type PlanRunMemory } from "./planProgress";
import {
  PLAN_GOING_CAP_MS,
  PLAN_GOING_SLACK_MS,
  planGoing,
  scanReminderDue,
  serviceStartsMs,
} from "./scanReminder";

/**
 * F-S2: the reminder stays quiet while the loaded plan is going. These tests play whole services on a fake clock: the
 * dashboard's tracker (planProgress.ts) is fed the same state fields the backend sends (playing song index, timer), once
 * every 30 seconds, and the reminder is asked at the minutes that matter.
 */
const MIN = 60_000;
const LA = "America/Los_Angeles";
const PLAN_ID = "plan-1";
// Sunday 2026-11-15 in Los Angeles (UTC-8): 09:00 is 17:00Z.
const T = (hour: number, minute = 0) => Date.UTC(2026, 10, 15, hour + 8, minute);
const iso = (ms: number) => new Date(ms).toISOString();

const FOUR_SONGS = [300, 300, 300, 300]; // four 5-minute songs
type Style = "stops" | "overruns";
type Play = { index: number; startMs: number; seconds: number; style?: Style; pausedAfterSeconds?: number };

/** Four songs back to back with a minute of talking between: song k plays [start + 6k, start + 6k + 5) minutes. */
const songsFrom = (startMs: number, style: Style = "stops", seconds = FOUR_SONGS): Play[] =>
  seconds.map((length, index) => ({ index, startMs: startMs + index * 6 * MIN, seconds: length, style }));

type Sim = {
  due: (now: number, capturedAt?: number | null) => ReturnType<typeof scanReminderDue>;
  run: () => PlanRunMemory | null;
};

function simulate(services: string[], plays: Play[], songSeconds: Array<number | null> = FOUR_SONGS): Sim {
  const plan = {
    id: PLAN_ID,
    date: "2026-11-15",
    service_times: services,
    songs: songSeconds.map((duration_seconds) => ({ duration_seconds })),
  };
  let memory: PlanRunMemory | null = null;
  let clock = T(6);
  const timerAt = (now: number): { index: number | null; timer: ApplicationState["timer"] } => {
    const play = [...plays].reverse().find((candidate) => candidate.startMs <= now);
    if (!play) return { index: null, timer: { status: "idle", duration_seconds: null, started_at: null, last_error: null } };
    const elapsed = (now - play.startMs) / 1000;
    const paused = play.pausedAfterSeconds !== undefined && elapsed >= play.pausedAfterSeconds;
    if (paused) {
      const position = play.pausedAfterSeconds ?? 0;
      return { index: play.index, timer: { status: "stopped", duration_seconds: play.seconds, started_at: null, position_seconds: position, remaining_seconds: play.seconds - position, last_error: null } };
    }
    if (elapsed < play.seconds || play.style === "overruns") {
      return { index: play.index, timer: { status: "running", duration_seconds: play.seconds, started_at: iso(play.startMs), position_seconds: 0, last_error: null } };
    }
    return { index: play.index, timer: { status: "stopped", duration_seconds: play.seconds, started_at: null, position_seconds: play.seconds, remaining_seconds: 0, last_error: null } };
  };
  const advance = (to: number) => {
    for (; clock <= to; clock += 30_000) {
      const { index, timer } = timerAt(clock);
      memory = observePlanRun(memory, { now: clock, planId: PLAN_ID, durations: songSeconds, playingIndex: index, timer });
    }
    clock = Math.max(clock, to);
  };
  return {
    due(now, capturedAt = null) {
      advance(now);
      const { index, timer } = timerAt(now);
      return scanReminderDue({
        now,
        plan,
        timeZone: LA,
        playback: playbackStatus({ discovery: "done", order_mode: "scanned", captured_at: capturedAt === null ? null : iso(capturedAt) }),
        songRunning: timer.status === "running" && index !== null,
        run: memory,
      });
    },
    run: () => memory,
  };
}

const shown = (sim: Sim, now: number, capturedAt: number | null = null) => sim.due(now, capturedAt) !== null;

describe("one service (the church: one a Sunday)", () => {
  // Service 10:00; the first song starts at 10:05; the songs end 10:05, 10:11, 10:17, 10:23 + 5 minutes: the last one ends 10:28.
  const sim = () => simulate(["10:00"], songsFrom(T(10, 5)));

  it("is shown in the hour before, then quiet from the start through every song, between songs and after the last one, for good", () => {
    const s = sim();
    expect(shown(s, T(9, 0))).toBe(true);
    expect(shown(s, T(9, 59))).toBe(true);
    expect(shown(s, T(10, 0))).toBe(false); // the service starts
    expect(shown(s, T(10, 3))).toBe(false); // before the first song
    expect(shown(s, T(10, 7))).toBe(false); // song 1 plays
    expect(shown(s, T(10, 10, ) + 30_000)).toBe(false); // between song 1 and 2
    expect(shown(s, T(10, 15))).toBe(false); // song 3
    expect(shown(s, T(10, 22))).toBe(false); // between 3 and the last
    expect(shown(s, T(10, 27))).toBe(false); // the last song plays
    expect(shown(s, T(10, 28) + 1_000)).toBe(false); // the last song ended: no later start, so never again
    for (const hour of [11, 12, 13, 16, 20]) expect(shown(s, T(hour))).toBe(false);
  });

  it("reads the end from the last song's own countdown, not from a guess", () => {
    const s = sim();
    s.due(T(10, 27));
    expect(s.run()).toMatchObject({ firstSongMs: T(10, 5), endedMs: null });
    s.due(T(10, 29));
    expect(s.run()).toMatchObject({ firstSongMs: T(10, 5), endedMs: T(10, 28) });
  });

  it("a scan inside the hour keeps it away before the start too", () => {
    const s = sim();
    expect(shown(s, T(9, 30), T(9, 10))).toBe(false);
    expect(shown(s, T(9, 55), T(9, 10))).toBe(false);
  });

  it("when the last countdown runs on past its end (the Presentation count-up) the song still counts as ended", () => {
    const s = simulate(["10:00"], songsFrom(T(10, 5), "overruns"));
    expect(shown(s, T(10, 27))).toBe(false);
    expect(shown(s, T(10, 40))).toBe(false);
    expect(s.run()?.endedMs).toBe(T(10, 28));
  });
});

describe("a run-through before the service", () => {
  // A run-through at T-30 (09:30) that finishes at T-10 (09:50): four 5-minute songs back to back (no talking).
  const runThrough = (): Play[] => FOUR_SONGS.map((seconds, index) => ({ index, startMs: T(9, 30) + index * 5 * MIN, seconds, style: "stops" as Style }));

  it("is hidden while it goes (even between songs), back after it until a scan completes", () => {
    const s = simulate(["10:00"], runThrough());
    expect(shown(s, T(9, 29))).toBe(true);
    expect(shown(s, T(9, 30))).toBe(false); // the first song starts
    expect(shown(s, T(9, 35))).toBe(false); // exactly between songs 1 and 2
    expect(shown(s, T(9, 44))).toBe(false); // last song
    expect(shown(s, T(9, 49) + 59_000)).toBe(false);
    expect(shown(s, T(9, 50) + 1_000)).toBe(true); // the run-through ended at T-10: the reminder is back
    expect(shown(s, T(9, 55))).toBe(true);
    expect(shown(s, T(9, 56), T(9, 55))).toBe(false); // a scan completes
    expect(shown(s, T(9, 59), T(9, 55))).toBe(false);
  });

  it("without a scan it stays until the service start, then the service itself keeps it quiet", () => {
    const s = simulate(["10:00"], runThrough());
    expect(shown(s, T(9, 50) + 1_000)).toBe(true);
    expect(shown(s, T(9, 59) + 59_000)).toBe(true);
    expect(shown(s, T(10, 0))).toBe(false);
    expect(shown(s, T(10, 5))).toBe(false);
    expect(shown(s, T(11, 30))).toBe(false); // nothing is ahead
  });

  it("a run-through of a two-service plan does not use up the service: the first service is going from its start", () => {
    const s = simulate(["10:00", "12:00"], [...runThrough(), ...songsFrom(T(10, 5)).map((p) => p)]);
    expect(shown(s, T(9, 55))).toBe(true);
    expect(shown(s, T(10, 0))).toBe(false);
    expect(shown(s, T(10, 15))).toBe(false);
    expect(shown(s, T(10, 40))).toBe(false); // the first service is over; 12:00's window opens at 11:00
    expect(shown(s, T(11, 0))).toBe(true);
  });
});

describe("two services, 10:00 and 12:00", () => {
  const plays = [...songsFrom(T(10, 5)), ...songsFrom(T(12, 5))];

  it("quiet from 10:00 through the end of its songs, back from 11:00 until 12:00 if unscanned, gone at 12:00", () => {
    const s = simulate(["10:00", "12:00"], plays);
    expect(shown(s, T(9, 30))).toBe(true); // before the first service
    for (const [h, m] of [[10, 0], [10, 5], [10, 12], [10, 20], [10, 27], [10, 29], [10, 59]]) expect(shown(s, T(h!, m!)), `${h}:${m}`).toBe(false);
    expect(shown(s, T(11, 0))).toBe(true); // the window of the 12:00 service opens
    expect(shown(s, T(11, 30))).toBe(true);
    expect(shown(s, T(11, 59))).toBe(true);
    expect(shown(s, T(12, 0))).toBe(false);
    expect(shown(s, T(12, 10))).toBe(false); // second service going
    expect(shown(s, T(12, 40))).toBe(false); // over; nothing ahead
    expect(shown(s, T(14, 0))).toBe(false);
  });

  it("a scan after 11:00 clears it; the stale state of the first service does not keep the second quiet or loud", () => {
    const s = simulate(["10:00", "12:00"], plays);
    expect(shown(s, T(11, 20), T(11, 10))).toBe(false);
    expect(shown(s, T(11, 50), T(11, 10))).toBe(false);
    expect(shown(s, T(12, 3), T(11, 10))).toBe(false);
  });

  it("a scan made before 11:00 (for the first service) does not cover the second", () => {
    const s = simulate(["10:00", "12:00"], plays);
    expect(shown(s, T(9, 20), T(9, 5))).toBe(false); // scanned in the first window
    expect(shown(s, T(11, 5), T(9, 5))).toBe(true);
  });

  it("the first song of the second service does not need to start on time: the service start already keeps it quiet", () => {
    const s = simulate(["10:00", "12:00"], [...songsFrom(T(10, 5)), ...songsFrom(T(12, 20))]);
    expect(shown(s, T(11, 59))).toBe(true);
    expect(shown(s, T(12, 0))).toBe(false);
    expect(shown(s, T(12, 10))).toBe(false); // songs not started yet: the stale end of the first run does not count for this service
  });
});

describe("two services exactly 60 minutes apart, 09:00 and 10:00", () => {
  const plays = songsFrom(T(9, 5)); // the first service's songs end at 09:28

  it("quiet while the first goes, back after its last song if unscanned, gone at 10:00", () => {
    const s = simulate(["09:00", "10:00"], plays);
    expect(shown(s, T(8, 30))).toBe(true);
    expect(shown(s, T(9, 0))).toBe(false);
    expect(shown(s, T(9, 12))).toBe(false);
    expect(shown(s, T(9, 27))).toBe(false);
    expect(shown(s, T(9, 28) + 1_000)).toBe(true); // over; 10:00's window opened at 09:00
    expect(shown(s, T(9, 59))).toBe(true);
    expect(shown(s, T(10, 0))).toBe(false);
    expect(shown(s, T(10, 30))).toBe(false);
  });

  it("a scan before 09:00 does not cover 10:00, one after the first service does", () => {
    const s = simulate(["09:00", "10:00"], plays);
    expect(shown(s, T(8, 45), T(8, 30))).toBe(false);
    expect(shown(s, T(9, 40), T(8, 30))).toBe(true);
    expect(shown(s, T(9, 45), T(9, 41))).toBe(false);
  });
});

describe("services closer than an hour, and services that overlap the plan's song time", () => {
  it("09:00 and 09:40: one scan after 08:40 covers both; without it the second gets its reminder once the plan is over", () => {
    const s = simulate(["09:00", "09:40"], songsFrom(T(9, 5)));
    expect(shown(s, T(8, 30))).toBe(true);
    expect(shown(s, T(9, 10))).toBe(false);
    expect(shown(s, T(9, 29))).toBe(true); // over at 09:28; the second is 11 minutes away
    expect(shown(s, T(9, 40))).toBe(false);
    const covered = simulate(["09:00", "09:40"], songsFrom(T(9, 5)));
    expect(shown(covered, T(8, 50), T(8, 45))).toBe(false);
    expect(shown(covered, T(9, 29), T(8, 45))).toBe(false);
  });

  it("09:00 and 09:20 with 23 minutes of songs: the second start passes during the plan and nothing is listed, ever", () => {
    const s = simulate(["09:00", "09:20"], songsFrom(T(9, 2)));
    for (const [h, m] of [[9, 0], [9, 10], [9, 19], [9, 20], [9, 22], [9, 24], [9, 30], [9, 40], [11, 0]]) expect(shown(s, T(h!, m!)), `${h}:${m}`).toBe(false);
  });

  it("two starts inside the same song time, with a third after the plan: the third gets its reminder from an hour before", () => {
    const s = simulate(["09:00", "09:20", "11:00"], songsFrom(T(9, 2)));
    expect(shown(s, T(9, 21))).toBe(false);
    expect(shown(s, T(9, 40))).toBe(false); // over, but 11:00 is 80 minutes away
    expect(shown(s, T(10, 0))).toBe(true);
    expect(shown(s, T(11, 0))).toBe(false);
  });
});

describe("three services (and unsorted or repeated times)", () => {
  const plays = [...songsFrom(T(9, 5)), ...songsFrom(T(11, 5)), ...songsFrom(T(13, 5))];
  const times = ["13:00", "09:00", "11:00", "09:00", "13:00"];

  it("quiet during each service; a reminder in the hour before the second and third; none after the last", () => {
    const s = simulate(times, plays);
    expect(shown(s, T(8, 30))).toBe(true);
    expect(shown(s, T(9, 3))).toBe(false);
    expect(shown(s, T(9, 20))).toBe(false);
    expect(shown(s, T(9, 40))).toBe(false); // over; 11:00 is 80 minutes away
    expect(shown(s, T(10, 0))).toBe(true);
    expect(shown(s, T(10, 59))).toBe(true);
    expect(shown(s, T(11, 0))).toBe(false);
    expect(shown(s, T(11, 20))).toBe(false);
    expect(shown(s, T(11, 40))).toBe(false); // over; 13:00 is 80 minutes away
    expect(shown(s, T(12, 0))).toBe(true);
    expect(shown(s, T(12, 59))).toBe(true);
    expect(shown(s, T(13, 0))).toBe(false);
    expect(shown(s, T(13, 20))).toBe(false);
    expect(shown(s, T(13, 40))).toBe(false);
    expect(shown(s, T(18, 0))).toBe(false);
  });

  it("scans in the hour before each of the later services clear their own reminder", () => {
    const s = simulate(times, plays);
    expect(shown(s, T(10, 30), T(10, 10))).toBe(false);
    expect(shown(s, T(12, 30), T(10, 10))).toBe(true); // that scan was for the 11:00
    expect(shown(s, T(12, 40), T(12, 20))).toBe(false);
  });

  it("the services are sorted and each time counted once", () => {
    expect(serviceStartsMs({ date: "2026-11-15", service_times: times }, LA)).toEqual([T(9), T(11), T(13)]);
  });
});

describe("the hard cap and the time estimate", () => {
  it("a plan that never ends (a song paused for good) stops hiding the reminder 3 hours after the start", () => {
    const stuck: Play[] = [{ index: 0, startMs: T(10, 5), seconds: 300, pausedAfterSeconds: 60 }];
    const s = simulate(["10:00", "13:30"], stuck);
    expect(shown(s, T(12, 0))).toBe(false); // the plan is still "going"; 13:30's window opens at 12:30
    expect(shown(s, T(12, 40))).toBe(false);
    expect(shown(s, T(12, 59))).toBe(false);
    expect(shown(s, T(13, 0))).toBe(true); // 3 hours after the 10:00 start (the first song came later)
  });

  it("the cap runs from the first song when that came before the service", () => {
    const stuck: Play[] = [{ index: 0, startMs: T(9, 0), seconds: 300, pausedAfterSeconds: 60 }];
    const s = simulate(["10:00", "12:30"], stuck);
    expect(shown(s, T(11, 59))).toBe(false); // 12:30's window opens at 11:30; the plan counts as going until 12:00
    expect(shown(s, T(12, 0))).toBe(true);
  });

  it("planGoing: never more than 3 hours after the start, whatever the memory says", () => {
    const run: PlanRunMemory = { planId: PLAN_ID, firstSongMs: T(10, 5), endedMs: null };
    const songs = FOUR_SONGS.map((duration_seconds) => ({ duration_seconds }));
    expect(planGoing(T(12, 59), [T(10)], songs, run)).toBe(true);
    expect(planGoing(T(13, 0), [T(10)], songs, run)).toBe(false);
    expect(planGoing(T(13, 0) - 1, [T(10)], songs, run)).toBe(true);
    expect(PLAN_GOING_CAP_MS).toBe(3 * 60 * MIN);
    // A run-through at 09:00 with the service at 10:00: the first song came first.
    const early: PlanRunMemory = { planId: PLAN_ID, firstSongMs: T(9), endedMs: null };
    expect(planGoing(T(11, 59), [T(10)], songs, early)).toBe(true);
    expect(planGoing(T(12, 0), [T(10)], songs, early)).toBe(false);
  });

  it("a plan with no song lengths uses the estimate: start plus the (zero) song lengths plus 30 minutes", () => {
    const noLengths = [null, null, null, null];
    const plays = songsFrom(T(10, 5), "stops", [0, 0, 0, 0]);
    const s = simulate(["10:00", "12:00"], plays, noLengths);
    expect(shown(s, T(9, 30))).toBe(true);
    expect(shown(s, T(10, 0))).toBe(false);
    expect(shown(s, T(10, 29))).toBe(false); // songs played but none of them can tell its own end
    expect(shown(s, T(10, 30))).toBe(false); // the estimate ends at 10:30, and 12:00's window is not open yet
    expect(shown(s, T(11, 0))).toBe(true);
    expect(shown(s, T(12, 0))).toBe(false);
    expect(s.run()?.endedMs).toBeNull(); // nothing claimed to know when the last song ended
  });

  it("the estimate with lengths and no tracked songs: the start plus the sum of the lengths plus 30 minutes", () => {
    const songs = FOUR_SONGS.map((duration_seconds) => ({ duration_seconds })); // 20 minutes of songs
    expect(PLAN_GOING_SLACK_MS).toBe(30 * MIN);
    expect(planGoing(T(10, 49), [T(10)], songs, null)).toBe(true);
    expect(planGoing(T(10, 50), [T(10)], songs, null)).toBe(false);
    expect(planGoing(T(9, 59), [T(10)], songs, null)).toBe(false);
    const noLengths = [{ duration_seconds: null }, { duration_seconds: null }];
    expect(planGoing(T(10, 29), [T(10)], noLengths, null)).toBe(true);
    expect(planGoing(T(10, 30), [T(10)], noLengths, null)).toBe(false);
    // Without any plan songs the same: the start plus 30 minutes.
    expect(planGoing(T(10, 29), [T(10)], undefined, null)).toBe(true);
    expect(planGoing(T(10, 30), [T(10)], [], null)).toBe(false);
  });

  it("with two service starts, the estimate runs from the latest one that no finished run covers", () => {
    const songs = FOUR_SONGS.map((duration_seconds) => ({ duration_seconds }));
    expect(planGoing(T(10, 49), [T(9), T(10)], songs, null)).toBe(true); // 10:00 + 20 + 30 = 10:50
    expect(planGoing(T(10, 50), [T(9), T(10)], songs, null)).toBe(false);
    const finished: PlanRunMemory = { planId: PLAN_ID, firstSongMs: T(9, 5), endedMs: T(9, 28) };
    expect(planGoing(T(9, 40), [T(9), T(10)], songs, finished)).toBe(false); // the run covers the 09:00 service
    expect(planGoing(T(10, 5), [T(9), T(10)], songs, finished)).toBe(true); // the 10:00 service has begun and nothing has played yet
  });
});

describe("other situations", () => {
  it("memory of another plan says nothing about this one", () => {
    const stale: PlanRunMemory = { planId: "other-plan", firstSongMs: T(9, 30), endedMs: null };
    const plan = { id: PLAN_ID, date: "2026-11-15", service_times: ["10:00"], songs: FOUR_SONGS.map((duration_seconds) => ({ duration_seconds })) };
    const playback = playbackStatus({ discovery: "done", order_mode: "scanned", captured_at: null });
    expect(scanReminderDue({ now: T(9, 40), plan, timeZone: LA, playback, run: stale })).not.toBeNull();
    expect(scanReminderDue({ now: T(9, 40), plan, timeZone: LA, playback, run: { ...stale, planId: PLAN_ID } })).toBeNull();
  });

  it("a song paused part-way is not 'over': the plan stays going", () => {
    const s = simulate(["10:00", "11:30"], [...songsFrom(T(10, 5)).slice(0, 3), { index: 3, startMs: T(10, 23), seconds: 300, pausedAfterSeconds: 120 }]);
    expect(shown(s, T(10, 40))).toBe(false);
    expect(shown(s, T(10, 45))).toBe(false);
    expect(s.run()?.endedMs).toBeNull();
  });

  it("the operator replays the last song after it ended: the plan is going again, then over again", () => {
    const plays = [...songsFrom(T(10, 5)), { index: 3, startMs: T(10, 40), seconds: 300, style: "stops" as Style }];
    const s = simulate(["10:00", "12:00"], plays);
    expect(shown(s, T(10, 35))).toBe(false); // over (12:00's window is not open yet)
    expect(shown(s, T(11, 1))).toBe(true);
    expect(shown(s, T(11, 30))).toBe(true);
    const later = simulate(["10:00", "11:50"], plays);
    expect(shown(later, T(10, 43))).toBe(false); // the replay is going
    expect(shown(later, T(10, 46))).toBe(false); // over again, but 11:50's window opens at 10:50
    expect(shown(later, T(10, 51))).toBe(true);
  });

  it("a page opened in the middle of the plan: the song in progress starts the run, the rest follows", () => {
    // The tracker is new (page just opened at 10:13, song 3 running since 10:12).
    const memory = observePlanRun(null, {
      now: T(10, 13), planId: PLAN_ID, durations: FOUR_SONGS, playingIndex: 2,
      timer: { status: "running", started_at: iso(T(10, 12)), position_seconds: 0, remaining_seconds: undefined },
    });
    expect(memory).toEqual({ planId: PLAN_ID, firstSongMs: T(10, 12), endedMs: null });
    expect(planGoing(T(10, 14), [T(10)], FOUR_SONGS.map((duration_seconds) => ({ duration_seconds })), memory)).toBe(true);
  });
});
