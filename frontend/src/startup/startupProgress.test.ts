import { describe, expect, it } from "vitest";

import {
  deriveStartupPhase,
  STARTUP_PHASE_CEILINGS,
  startupPresentation,
  timeBasedProgress,
  type StartupProgressInput,
} from "./startupProgress";

const startingInput: StartupProgressInput = {
  supervisorState: "starting",
  applicationStateLoaded: false,
  connectionEstablished: false,
  retrying: false,
  startupAttempt: 0,
};

describe("startup progress curve", () => {
  it("continues advancing throughout a delayed 15-second backend startup", () => {
    const checkpoints = [0, 500, 2_000, 5_000, 10_000, 15_000].map((elapsedMs) =>
      timeBasedProgress({
        phase: "starting-backend",
        elapsedMs,
        startingValue: 1,
      }),
    );

    expect(checkpoints[0]).toBeGreaterThanOrEqual(1);
    for (let index = 1; index < checkpoints.length; index += 1) {
      expect(checkpoints[index] ?? 0).toBeGreaterThan(checkpoints[index - 1] ?? 0);
    }
    expect(checkpoints[4]).not.toBe(checkpoints[5]);
    expect(checkpoints[5]).toBeLessThan(STARTUP_PHASE_CEILINGS["starting-backend"]);
  });

  it("keeps visibly advancing toward a truthful ceiling over long startups", () => {
    const at20Seconds = timeBasedProgress({
      phase: "starting-backend",
      elapsedMs: 20_000,
      startingValue: 1,
    });
    const at30Seconds = timeBasedProgress({
      phase: "starting-backend",
      elapsedMs: 30_000,
      startingValue: 1,
    });
    const at60Seconds = timeBasedProgress({
      phase: "starting-backend",
      elapsedMs: 60_000,
      startingValue: 1,
    });

    expect(at30Seconds - at20Seconds).toBeGreaterThan(5);
    expect(at60Seconds).toBeGreaterThan(at30Seconds);
    expect(at60Seconds).toBeLessThanOrEqual(88);
  });

  it("is bounded and finite for invalid or extreme elapsed values", () => {
    for (const elapsedMs of [-1, Number.NaN, Number.POSITIVE_INFINITY, 1e12]) {
      const value = timeBasedProgress({
        phase: "starting-backend",
        elapsedMs,
        startingValue: 1,
      });
      expect(Number.isFinite(value)).toBe(true);
      expect(value).toBeGreaterThanOrEqual(1);
      expect(value).toBeLessThanOrEqual(88);
    }
  });

  it("freezes confirmed failures and resumes monotonically for retry", () => {
    const frozen = timeBasedProgress({
      phase: "failed",
      elapsedMs: 30_000,
      startingValue: 47.5,
    });
    const retried = timeBasedProgress({
      phase: "starting-backend",
      elapsedMs: 2_000,
      startingValue: frozen,
    });

    expect(frozen).toBe(47.5);
    expect(retried).toBeGreaterThanOrEqual(frozen);
    expect(retried).toBeLessThanOrEqual(88);
  });

  it("completes quickly only after dashboard data is actually ready", () => {
    const beforeReady = timeBasedProgress({
      phase: "loading-state",
      elapsedMs: 60_000,
      startingValue: 90,
    });
    const completing = timeBasedProgress({
      phase: "preparing-dashboard",
      elapsedMs: 240,
      startingValue: beforeReady,
    });
    const complete = timeBasedProgress({
      phase: "preparing-dashboard",
      elapsedMs: 500,
      startingValue: beforeReady,
    });

    expect(beforeReady).toBeLessThanOrEqual(94);
    expect(completing).toBeGreaterThan(beforeReady);
    expect(completing).toBeLessThan(100);
    expect(complete).toBe(100);
  });
});

describe("startup phase model", () => {
  it.each([
    [{ ...startingInput, supervisorState: null }, 0, "initializing"],
    [{ ...startingInput, supervisorState: null }, 500, "starting-backend"],
    [startingInput, 500, "starting-backend"],
    [{ ...startingInput, supervisorState: "ready" }, 500, "backend-ready"],
    [{ ...startingInput, supervisorState: "external" }, 500, "backend-ready"],
    [{ ...startingInput, connectionEstablished: true }, 500, "loading-state"],
    [{ ...startingInput, applicationStateLoaded: true }, 500, "preparing-dashboard"],
    [{ ...startingInput, supervisorState: "failed" }, 500, "failed"],
    [{ ...startingInput, supervisorState: "stopped" }, 500, "failed"],
    [
      { ...startingInput, supervisorState: "failed", retrying: true, startupAttempt: 1 },
      500,
      "starting-backend",
    ],
  ] satisfies [StartupProgressInput, number, string][])(
    "derives %s at %i ms as %s",
    (input, elapsedMs, expected) => {
      expect(deriveStartupPhase(input, elapsedMs)).toBe(expected);
    },
  );

  it("uses honest delayed, failure, retry, and ready language", () => {
    expect(
      startupPresentation({
        phase: "starting-backend",
        totalElapsedMs: 10_000,
        retrying: false,
      }),
    ).toMatchObject({ tone: "delayed", headline: expect.stringContaining("longer") });
    expect(
      startupPresentation({
        phase: "starting-backend",
        totalElapsedMs: 10_000,
        retrying: true,
      }),
    ).toMatchObject({ headline: "Retrying the engine" });
    expect(
      startupPresentation({ phase: "failed", totalElapsedMs: 10_000, retrying: false }),
    ).toMatchObject({ tone: "failed", moving: false });
    expect(
      startupPresentation({ phase: "complete", totalElapsedMs: 10_000, retrying: false }),
    ).toMatchObject({ tone: "complete", moving: false });
  });
});
