import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { StartupProgressInput } from "./startupProgress";
import { useStartupProgress } from "./useStartupProgress";

const input = (overrides: Partial<StartupProgressInput> = {}): StartupProgressInput => ({
  supervisorState: "starting",
  applicationStateLoaded: false,
  connectionEstablished: false,
  retrying: false,
  startupAttempt: 0,
  ...overrides,
});

describe("useStartupProgress", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) =>
      window.setTimeout(() => callback(performance.now()), 16),
    );
    vi.stubGlobal("cancelAnimationFrame", (id: number) => window.clearTimeout(id));
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("regresses the former 58% stall during a 15-second healthy startup", () => {
    const { result } = renderHook(() => useStartupProgress(input()));
    const checkpoints: number[] = [];

    for (const elapsed of [2_000, 3_000, 5_000, 5_000]) {
      act(() => vi.advanceTimersByTime(elapsed));
      checkpoints.push(result.current.value);
    }

    expect(checkpoints[1] ?? 0).toBeGreaterThan(checkpoints[0] ?? 0);
    expect(checkpoints[2] ?? 0).toBeGreaterThan(checkpoints[1] ?? 0);
    expect(checkpoints[3] ?? 0).toBeGreaterThan(checkpoints[2] ?? 0);
    expect(checkpoints[3] ?? 100).toBeLessThanOrEqual(88);
  });

  it("freezes on failure and resumes from the frozen value on retry", () => {
    const { result, rerender } = renderHook(
      ({ current }) => useStartupProgress(current),
      { initialProps: { current: input() } },
    );
    act(() => vi.advanceTimersByTime(5_000));
    const beforeFailure = result.current.value;

    rerender({ current: input({ supervisorState: "failed" }) });
    act(() => vi.advanceTimersByTime(5_000));
    expect(result.current.value).toBeCloseTo(beforeFailure, 4);
    expect(result.current.tone).toBe("failed");

    rerender({
      current: input({ supervisorState: "starting", retrying: true, startupAttempt: 1 }),
    });
    act(() => vi.advanceTimersByTime(2_000));
    expect(result.current.value).toBeGreaterThan(beforeFailure);
    expect(result.current.headline).toBe("Retrying the engine");
  });

  it("completes smoothly once real application state exists", () => {
    const { result, rerender } = renderHook(
      ({ current }) => useStartupProgress(current),
      { initialProps: { current: input() } },
    );
    act(() => vi.advanceTimersByTime(1_000));
    rerender({
      current: input({
        supervisorState: "ready",
        connectionEstablished: true,
        applicationStateLoaded: true,
      }),
    });

    act(() => vi.advanceTimersByTime(250));
    expect(result.current.value).toBeGreaterThan(1);
    expect(result.current.value).toBeLessThan(100);
    act(() => vi.advanceTimersByTime(350));
    expect(result.current.value).toBe(100);
    expect(result.current.complete).toBe(true);
  });

  it("continues truthful updates with reduced motion and cleans up timers", () => {
    const { result, unmount } = renderHook(() =>
      useStartupProgress(input(), { reducedMotion: true }),
    );
    act(() => vi.advanceTimersByTime(1_000));
    expect(result.current.value).toBeGreaterThan(1);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
