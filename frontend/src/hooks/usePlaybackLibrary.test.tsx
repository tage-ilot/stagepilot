import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { SettingsResponse } from "../types";
import { LIBRARY_REFRESH_MS, usePlaybackLibrary } from "./usePlaybackLibrary";

vi.mock("../api", () => ({
  getPlaybackLibraryStatus: vi.fn(),
  getPlaybackLibraryPlan: vi.fn(),
  updatePlaybackLibrarySettings: vi.fn(),
  getSettings: vi.fn(),
}));

const settings = (library?: { enabled: boolean; address: string }) => ({ settings: { playback_library: library } }) as unknown as SettingsResponse;
const status = { enabled: true, address: "http://10.20.0.102", state: "ready", message: "ok", version: null, songs: null, setlists: null, stale: false, checked_at: null };
const plan = { enabled: true, state: "ready", message: null, plan_date: null, setlist: null, newest: null, songs: [], stale: false, checked_at: null };

beforeEach(() => {
  vi.mocked(api.getPlaybackLibraryStatus).mockResolvedValue(status as never);
  vi.mocked(api.getPlaybackLibraryPlan).mockResolvedValue(plan as never);
});
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });

describe("usePlaybackLibrary", () => {
  it("asks for nothing at all while the setting is off or absent", async () => {
    vi.useFakeTimers();
    renderHook(() => usePlaybackLibrary(true, settings(undefined), vi.fn()));
    renderHook(() => usePlaybackLibrary(true, settings({ enabled: false, address: "10.0.0.1" }), vi.fn()));
    renderHook(() => usePlaybackLibrary(true, null, vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_REFRESH_MS * 3); });
    expect(api.getPlaybackLibraryStatus).not.toHaveBeenCalled();
    expect(api.getPlaybackLibraryPlan).not.toHaveBeenCalled();
  });

  it("asks nothing of a window that cannot configure", async () => {
    renderHook(() => usePlaybackLibrary(false, settings({ enabled: true, address: "10.0.0.1" }), vi.fn()));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(api.getPlaybackLibraryStatus).not.toHaveBeenCalled();
  });

  it("asks once when it is on, then every 30 seconds, and stops when the window closes", async () => {
    vi.useFakeTimers();
    const { result, unmount } = renderHook(() => usePlaybackLibrary(true, settings({ enabled: true, address: "http://10.20.0.102" }), vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(api.getPlaybackLibraryStatus).toHaveBeenCalledTimes(1);
    expect(api.getPlaybackLibraryStatus).toHaveBeenLastCalledWith(false);
    expect(result.current.status?.state).toBe("ready");
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_REFRESH_MS - 100); });
    expect(api.getPlaybackLibraryStatus).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(200); });
    expect(api.getPlaybackLibraryPlan).toHaveBeenCalledTimes(2);
    unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_REFRESH_MS * 3); });
    expect(api.getPlaybackLibraryPlan).toHaveBeenCalledTimes(2);
  });

  it("skips a refresh while the window is hidden", async () => {
    vi.useFakeTimers();
    const hidden = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    renderHook(() => usePlaybackLibrary(true, settings({ enabled: true, address: "10.0.0.1" }), vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_REFRESH_MS * 2 + 100); });
    expect(api.getPlaybackLibraryStatus).not.toHaveBeenCalled();
    hidden.mockRestore();
  });

  it("the Check button asks now, with check=1", async () => {
    const { result } = renderHook(() => usePlaybackLibrary(true, settings({ enabled: true, address: "10.0.0.1" }), vi.fn()));
    await waitFor(() => expect(result.current.status).not.toBeNull());
    act(() => result.current.check());
    await waitFor(() => expect(api.getPlaybackLibraryStatus).toHaveBeenLastCalledWith(true));
    expect(api.getPlaybackLibraryPlan).toHaveBeenLastCalledWith(true);
  });

  it("saving writes only the setting, then reloads the settings", async () => {
    vi.mocked(api.updatePlaybackLibrarySettings).mockResolvedValue(status as never);
    const fresh = settings({ enabled: true, address: "http://10.20.0.102" });
    vi.mocked(api.getSettings).mockResolvedValue(fresh);
    const onSettings = vi.fn();
    const { result } = renderHook(() => usePlaybackLibrary(true, settings(undefined), onSettings));
    let saved = false;
    await act(async () => { saved = await result.current.save(true, "10.20.0.102"); });
    expect(saved).toBe(true);
    expect(api.updatePlaybackLibrarySettings).toHaveBeenCalledWith(true, "10.20.0.102");
    expect(onSettings).toHaveBeenCalledWith(fresh);
  });

  it("says why a save failed", async () => {
    vi.mocked(api.updatePlaybackLibrarySettings).mockRejectedValue(new Error("Enter the Playback library address as host or host:port, e.g. 10.20.0.102:8780."));
    const { result } = renderHook(() => usePlaybackLibrary(true, settings(undefined), vi.fn()));
    await act(async () => { await result.current.save(true, "10.0.0.5/x"); });
    expect(result.current.error).toContain("host or host:port");
  });
});
