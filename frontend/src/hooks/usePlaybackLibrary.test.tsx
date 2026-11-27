import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { libraryPlan, libraryStatus } from "../test/libraryFixtures";
import type { SettingsResponse } from "../types";
import { LIBRARY_REFRESH_MS, LIBRARY_WATCH_MS, usePlaybackLibrary } from "./usePlaybackLibrary";

vi.mock("../api", () => ({
  getPlaybackLibraryStatus: vi.fn(),
  getPlaybackLibraryPlan: vi.fn(),
  updatePlaybackLibrarySettings: vi.fn(),
  choosePlaybackLibrarySetlist: vi.fn(),
  pairPlaybackLibrarySong: vi.fn(),
  unpairPlaybackLibrarySong: vi.fn(),
  getSettings: vi.fn(),
}));

/** The saved settings: the Playback computer (`host`) and the optional override of the helper's address. */
const settings = (host: string | null, library?: { enabled?: boolean | null; address: string }) =>
  ({ settings: { playback_api: host === null ? undefined : { host }, playback_library: library } }) as unknown as SettingsResponse;

beforeEach(() => {
  vi.mocked(api.getPlaybackLibraryStatus).mockResolvedValue(libraryStatus("ready"));
  vi.mocked(api.getPlaybackLibraryPlan).mockResolvedValue(libraryPlan([]));
});
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });

describe("usePlaybackLibrary", () => {
  it("asks for nothing at all until a Playback computer is saved (or Advanced names an address)", async () => {
    vi.useFakeTimers();
    renderHook(() => usePlaybackLibrary(true, settings(null), vi.fn()));
    renderHook(() => usePlaybackLibrary(true, settings(""), vi.fn()));
    renderHook(() => usePlaybackLibrary(true, settings(null, { address: "" }), vi.fn()));
    renderHook(() => usePlaybackLibrary(true, null, vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_REFRESH_MS * 3); });
    expect(api.getPlaybackLibraryStatus).not.toHaveBeenCalled();
    expect(api.getPlaybackLibraryPlan).not.toHaveBeenCalled();
  });

  it("asks whenever a Playback computer is saved, with no switch to turn on", async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => usePlaybackLibrary(true, settings("10.30.0.141"), vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(api.getPlaybackLibraryStatus).toHaveBeenCalledTimes(1);
    expect(result.current.target).toBe("10.30.0.141");
    expect(result.current.address).toBe("");
    // An override names the computer by itself.
    const other = renderHook(() => usePlaybackLibrary(true, settings(null, { address: "http://10.20.0.102" }), vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(other.result.current.target).toBe("http://10.20.0.102");
    expect(api.getPlaybackLibraryStatus).toHaveBeenCalledTimes(2);
  });

  it("an old file whose switch was off still reads like any other: the window just asks and the backend says what the helper says", async () => {
    vi.useFakeTimers();
    renderHook(() => usePlaybackLibrary(true, settings("10.20.0.102", { enabled: false, address: "" }), vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(api.getPlaybackLibraryStatus).toHaveBeenCalledTimes(1);
  });

  it("asks nothing of a window that cannot configure", async () => {
    renderHook(() => usePlaybackLibrary(false, settings("10.30.0.141"), vi.fn()));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(api.getPlaybackLibraryStatus).not.toHaveBeenCalled();
  });

  it("asks once, then every 30 seconds, and stops when the window closes", async () => {
    vi.useFakeTimers();
    const { result, unmount } = renderHook(() => usePlaybackLibrary(true, settings("10.30.0.141"), vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(api.getPlaybackLibraryStatus).toHaveBeenCalledTimes(1);
    expect(api.getPlaybackLibraryStatus).toHaveBeenLastCalledWith(false);
    expect(result.current.status?.helper.state).toBe("ready");
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_REFRESH_MS - 100); });
    expect(api.getPlaybackLibraryStatus).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(200); });
    expect(api.getPlaybackLibraryPlan).toHaveBeenCalledTimes(2);
    unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_REFRESH_MS * 3); });
    expect(api.getPlaybackLibraryPlan).toHaveBeenCalledTimes(2);
  });

  it("looks every few seconds while a setup box waits, and goes back to every 30 without losing what it holds", async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => usePlaybackLibrary(true, settings("10.30.0.141"), vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    const held = result.current.status;
    expect(held).not.toBeNull();
    act(() => result.current.watch(true));
    expect(result.current.status).toBe(held);
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_WATCH_MS * 3 + 100); });
    expect(vi.mocked(api.getPlaybackLibraryStatus).mock.calls.length).toBeGreaterThanOrEqual(4);
    act(() => result.current.watch(false));
    const before = vi.mocked(api.getPlaybackLibraryStatus).mock.calls.length;
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_WATCH_MS * 3); });
    expect(vi.mocked(api.getPlaybackLibraryStatus).mock.calls.length - before).toBeLessThanOrEqual(1);
  });

  it("forgets what it held when the Playback computer changes", async () => {
    const { result, rerender } = renderHook(({ host }) => usePlaybackLibrary(true, settings(host), vi.fn()), { initialProps: { host: "10.30.0.141" } });
    await waitFor(() => expect(result.current.status).not.toBeNull());
    vi.mocked(api.getPlaybackLibraryStatus).mockImplementation(() => new Promise(() => undefined));
    rerender({ host: "10.20.0.102" });
    expect(result.current.status).toBeNull();
    expect(result.current.plan).toBeNull();
  });

  it("skips a refresh while the window is hidden", async () => {
    vi.useFakeTimers();
    const hidden = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    renderHook(() => usePlaybackLibrary(true, settings("10.30.0.141"), vi.fn()));
    await act(async () => { await vi.advanceTimersByTimeAsync(LIBRARY_REFRESH_MS * 2 + 100); });
    expect(api.getPlaybackLibraryStatus).not.toHaveBeenCalled();
    hidden.mockRestore();
  });

  it("the Check button asks now, with check=1", async () => {
    const { result } = renderHook(() => usePlaybackLibrary(true, settings("10.30.0.141"), vi.fn()));
    await waitFor(() => expect(result.current.status).not.toBeNull());
    act(() => result.current.check());
    await waitFor(() => expect(api.getPlaybackLibraryStatus).toHaveBeenLastCalledWith(true));
    expect(api.getPlaybackLibraryPlan).toHaveBeenLastCalledWith(true);
  });

  it("saving the address writes only that, then reloads the settings", async () => {
    vi.mocked(api.updatePlaybackLibrarySettings).mockResolvedValue(libraryStatus("ready"));
    const fresh = settings("10.30.0.141", { address: "http://10.20.0.102" });
    vi.mocked(api.getSettings).mockResolvedValue(fresh);
    const onSettings = vi.fn();
    const { result } = renderHook(() => usePlaybackLibrary(true, settings("10.30.0.141"), onSettings));
    let saved = false;
    await act(async () => { saved = await result.current.saveAddress("10.20.0.102"); });
    expect(saved).toBe(true);
    expect(api.updatePlaybackLibrarySettings).toHaveBeenCalledWith("10.20.0.102");
    expect(onSettings).toHaveBeenCalledWith(fresh);
  });

  it("says why a save failed", async () => {
    vi.mocked(api.updatePlaybackLibrarySettings).mockRejectedValue(new Error("Enter the Playback library address as host or host:port, e.g. 10.20.0.102:8780."));
    const { result } = renderHook(() => usePlaybackLibrary(true, settings("10.30.0.141"), vi.fn()));
    await act(async () => { await result.current.saveAddress("10.0.0.5/x"); });
    expect(result.current.error).toContain("host or host:port");
  });

  it("choosing a setlist sends only that, shows the answer, and says why when it fails", async () => {
    const chosen = libraryPlan([], { chosen: true });
    vi.mocked(api.choosePlaybackLibrarySetlist).mockResolvedValueOnce(chosen).mockRejectedValueOnce(new Error("No service plan is loaded."));
    const { result } = renderHook(() => usePlaybackLibrary(true, settings("10.30.0.141"), vi.fn()));
    await waitFor(() => expect(result.current.plan).not.toBeNull());
    let done = false;
    await act(async () => { done = await result.current.chooseSetlist(7); });
    expect(done).toBe(true);
    expect(api.choosePlaybackLibrarySetlist).toHaveBeenCalledWith(7);
    expect(result.current.plan).toBe(chosen);
    expect(result.current.pending).toBeNull();
    await act(async () => { done = await result.current.chooseSetlist(null); });
    expect(done).toBe(false);
    expect(result.current.error).toBe("No service plan is loaded.");
  });

  it("does not choose anything when the window may not configure", async () => {
    const { result } = renderHook(() => usePlaybackLibrary(false, settings("10.30.0.141"), vi.fn()));
    let done = true;
    await act(async () => { done = await result.current.chooseSetlist(7); });
    expect(done).toBe(false);
    expect(api.choosePlaybackLibrarySetlist).not.toHaveBeenCalled();
  });
  it("pairing and undoing send only the plan song's key (and Playback's song), show the answer, and say why when it fails", async () => {
    const paired = libraryPlan([], { chosen: false });
    const undone = libraryPlan([], { stale: true });
    vi.mocked(api.pairPlaybackLibrarySong).mockResolvedValueOnce(paired).mockRejectedValueOnce(new Error("That Playback song is not one this song can be paired with now."));
    vi.mocked(api.unpairPlaybackLibrarySong).mockResolvedValueOnce(undone);
    const { result } = renderHook(() => usePlaybackLibrary(true, settings("10.30.0.141"), vi.fn()));
    await waitFor(() => expect(result.current.plan).not.toBeNull());
    expect(result.current.canPair).toBe(true);
    let done = false;
    await act(async () => { done = await result.current.pairSong("k2", 2000003); });
    expect(done).toBe(true);
    expect(api.pairPlaybackLibrarySong).toHaveBeenCalledWith("k2", 2000003);
    expect(result.current.plan).toBe(paired);
    expect(result.current.pending).toBeNull();
    await act(async () => { done = await result.current.pairSong("k2", 1); });
    expect(done).toBe(false);
    expect(result.current.error).toBe("That Playback song is not one this song can be paired with now.");
    await act(async () => { done = await result.current.unpairSong("k2"); });
    expect(done).toBe(true);
    expect(api.unpairPlaybackLibrarySong).toHaveBeenCalledWith("k2");
    expect(result.current.plan).toBe(undone);
    expect(result.current.error).toBeNull();
  });

  it("a window that may not configure (a remote viewer) never pairs or undoes", async () => {
    const { result } = renderHook(() => usePlaybackLibrary(false, settings("10.30.0.141"), vi.fn()));
    expect(result.current.canPair).toBe(false);
    let done = true;
    await act(async () => { done = await result.current.pairSong("k2", 1); });
    expect(done).toBe(false);
    await act(async () => { done = await result.current.unpairSong("k2"); });
    expect(done).toBe(false);
    expect(api.pairPlaybackLibrarySong).not.toHaveBeenCalled();
    expect(api.unpairPlaybackLibrarySong).not.toHaveBeenCalled();
  });
});
