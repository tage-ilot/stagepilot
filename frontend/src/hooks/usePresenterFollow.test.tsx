import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { playbackSettings } from "../test/playbackFixtures";
import { followStatus } from "../test/presenterFollowFixtures";
import type { SettingsResponse } from "../types";
import { FOLLOW_IDLE_REFRESH_MS, FOLLOW_REFRESH_MS, usePresenterFollow } from "./usePresenterFollow";

vi.mock("../api", () => ({ getPresenterFollowStatus: vi.fn(), presenterFollowTryAgain: vi.fn(), getSettings: vi.fn(), updateSettings: vi.fn() }));

const withFollow = (follow?: { enabled: boolean; offset_ms: number }): SettingsResponse => ({ ...playbackSettings, settings: { ...playbackSettings.settings, propresenter: { ...playbackSettings.settings.propresenter, ...(follow ? { presenter_follow: follow } : {}) } } });
const noop = () => undefined;

beforeEach(() => {
  vi.mocked(api.getPresenterFollowStatus).mockResolvedValue(followStatus());
  vi.mocked(api.getSettings).mockResolvedValue(withFollow());
  vi.mocked(api.updateSettings).mockImplementation(async (settings) => ({ ...playbackSettings, settings }));
});
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });

describe("usePresenterFollow", () => {
  it("asks nothing until a window that shows the answer is open and the person may configure", async () => {
    vi.useFakeTimers();
    renderHook(() => usePresenterFollow(false, true, null, noop));
    renderHook(() => usePresenterFollow(true, false, null, noop));
    await act(async () => { await vi.advanceTimersByTimeAsync(FOLLOW_IDLE_REFRESH_MS * 2); });
    expect(api.getPresenterFollowStatus).not.toHaveBeenCalled();
  });

  it("looks every 30 seconds while the switch is off and every 2 while it is on", async () => {
    vi.useFakeTimers();
    const off = renderHook(() => usePresenterFollow(true, true, withFollow(), noop));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(api.getPresenterFollowStatus).toHaveBeenCalledTimes(1);
    expect(off.result.current.enabled).toBe(false);
    await act(async () => { await vi.advanceTimersByTimeAsync(FOLLOW_REFRESH_MS * 3); });
    expect(api.getPresenterFollowStatus).toHaveBeenCalledTimes(1);
    off.unmount();
    vi.mocked(api.getPresenterFollowStatus).mockClear();
    const on = renderHook(() => usePresenterFollow(true, true, withFollow({ enabled: true, offset_ms: 150 }), noop));
    await act(async () => { await vi.advanceTimersByTimeAsync(FOLLOW_REFRESH_MS * 3 + 50); });
    expect(vi.mocked(api.getPresenterFollowStatus).mock.calls.length).toBeGreaterThanOrEqual(3);
    expect(on.result.current).toMatchObject({ enabled: true, offsetMs: 150 });
  });

  it("is off, with no offset, when the settings carry no presenter_follow", () => {
    const { result } = renderHook(() => usePresenterFollow(true, false, withFollow(), noop));
    expect(result.current).toMatchObject({ enabled: false, offsetMs: 0 });
  });

  it("saves the switch on top of the settings as they are now, keeps the offset and hands the answer on", async () => {
    vi.mocked(api.getSettings).mockResolvedValue(withFollow({ enabled: false, offset_ms: 250 }));
    const onSettings = vi.fn();
    const { result } = renderHook(() => usePresenterFollow(true, true, withFollow(), onSettings));
    let saved = false;
    await act(async () => { saved = await result.current.setEnabled(true); });
    expect(saved).toBe(true);
    const sent = vi.mocked(api.updateSettings).mock.calls[0]![0];
    expect(sent.propresenter.presenter_follow).toEqual({ enabled: true, offset_ms: 250 });
    // Nothing else of the settings is lost.
    expect(sent.propresenter.host).toBe("127.0.0.1");
    expect(sent.playback_api).toEqual(playbackSettings.settings.playback_api);
    expect(onSettings).toHaveBeenCalledTimes(1);
    expect(result.current.pending).toBeNull();
  });

  it("saves a whole-number offset in range, and refuses another without asking the backend", async () => {
    vi.mocked(api.getSettings).mockResolvedValue(withFollow({ enabled: true, offset_ms: 0 }));
    const { result } = renderHook(() => usePresenterFollow(true, true, withFollow({ enabled: true, offset_ms: 0 }), noop));
    await act(async () => { expect(await result.current.setOffset(-300)).toBe(true); });
    expect(vi.mocked(api.updateSettings).mock.calls[0]![0].propresenter.presenter_follow).toEqual({ enabled: true, offset_ms: -300 });
    vi.mocked(api.updateSettings).mockClear();
    for (const bad of [2001, -2001, 1.5, Number.NaN]) await act(async () => { expect(await result.current.setOffset(bad)).toBe(false); });
    expect(api.updateSettings).not.toHaveBeenCalled();
  });

  it("says in words that a save failed and leaves the switch where it was", async () => {
    vi.mocked(api.updateSettings).mockRejectedValue(new Error("boom"));
    const onSettings = vi.fn();
    const { result } = renderHook(() => usePresenterFollow(true, true, withFollow(), onSettings));
    await act(async () => { expect(await result.current.setEnabled(true)).toBe(false); });
    expect(result.current.error).toBeTruthy();
    expect(result.current.enabled).toBe(false);
    expect(onSettings).not.toHaveBeenCalled();
  });

  it("Try again asks the route and keeps the new status", async () => {
    vi.mocked(api.presenterFollowTryAgain).mockResolvedValue(followStatus({ state: "ready", message: "Nothing to follow" }));
    const { result } = renderHook(() => usePresenterFollow(true, true, withFollow({ enabled: true, offset_ms: 0 }), noop));
    await act(async () => { result.current.tryAgain(); });
    expect(api.presenterFollowTryAgain).toHaveBeenCalledTimes(1);
    expect(result.current.status?.state).toBe("ready");
  });
});
