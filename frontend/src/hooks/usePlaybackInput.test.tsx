import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { playbackSettings, playbackStatus } from "../test/playbackFixtures";
import { usePlaybackInput } from "./usePlaybackInput";

vi.mock("../api", () => ({
  getPlaybackStatus: vi.fn(), getPlaybackEvents: vi.fn(), getSettings: vi.fn(),
  updateSettings: vi.fn(), updatePlaybackSettings: vi.fn(), findPlayback: vi.fn(), cancelPlaybackScan: vi.fn(), discoverPlaybackSongOrder: vi.fn(),
}));
const tick = async (ms = 0) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
};

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  vi.mocked(api.getPlaybackStatus).mockResolvedValue(playbackStatus());
  vi.mocked(api.getPlaybackEvents).mockResolvedValue({ events: [], capacity: 100 });
  vi.mocked(api.getSettings).mockResolvedValue(playbackSettings);
  vi.mocked(api.updateSettings).mockResolvedValue(playbackSettings);
  vi.mocked(api.updatePlaybackSettings).mockResolvedValue(playbackStatus());
  vi.mocked(api.findPlayback).mockResolvedValue(playbackStatus({ connected: false, host: null }));
  vi.mocked(api.discoverPlaybackSongOrder).mockResolvedValue(playbackStatus());
});
afterEach(() => vi.useRealTimers());

describe("usePlaybackInput", () => {
  it("polls live ownership independently of aggregate status; stops after unmount", async () => {
    const { result, unmount } = renderHook(() => usePlaybackInput(true, vi.fn()));
    await tick();
    expect(result.current.status?.active_source).toBe("playback_api");
    vi.mocked(api.getPlaybackStatus).mockResolvedValue(playbackStatus({ active_source: "midi", reason: "Connected via MIDI." }));
    await tick(750);
    expect(result.current.status?.active_source).toBe("midi");
    const count = vi.mocked(api.getPlaybackStatus).mock.calls.length;
    unmount();
    await tick(1500);
    expect(api.getPlaybackStatus).toHaveBeenCalledTimes(count);
  });

  it("never polls or mutates for Viewers and clears state when access is removed", async () => {
    const onSettings = vi.fn();
    const { result, rerender } = renderHook(({ can }) => usePlaybackInput(can, onSettings), { initialProps: { can: false } });
    await tick();
    act(() => { result.current.scan(); result.current.discover(); void result.current.selectSource("real"); });
    expect(api.getPlaybackStatus).not.toHaveBeenCalled();
    expect(api.findPlayback).not.toHaveBeenCalled();
    expect(api.discoverPlaybackSongOrder).not.toHaveBeenCalled();
    expect(api.getSettings).not.toHaveBeenCalled();
    rerender({ can: true });
    await tick();
    expect(result.current.status).not.toBeNull();
    rerender({ can: false });
    expect(result.current.status).toBeNull();
  });

  it("saves only connection fields, reads back persisted settings, and preserves MIDI/order on source change", async () => {
    const onSettings = vi.fn();
    const { result } = renderHook(() => usePlaybackInput(true, onSettings));
    await tick();
    const input = { enabled: true, host: "192.0.2.10", port: 8080, auto_scan: true, fast_transport: true };
    act(() => { void result.current.save(input); });
    await tick();
    expect(api.updatePlaybackSettings).toHaveBeenCalledWith(input);
    expect(onSettings).toHaveBeenCalledWith(playbackSettings);
    act(() => { void result.current.selectSource("real"); });
    await tick();
    expect(api.updateSettings).toHaveBeenCalledWith({ ...playbackSettings.settings,
      integration_modes: { ...playbackSettings.settings.integration_modes, midi_source: "real" },
    });
  });

  it("holds the scan pending until the backend says it finished, not on MIDI fallback; a lost backend is bounded", async () => {
    const scanning = playbackStatus({ active_source: "midi", scan: { state: "scanning", candidates: [], reason: null, current: 1, total: 4 } });
    vi.mocked(api.getPlaybackStatus).mockResolvedValue(scanning);
    const { result } = renderHook(() => usePlaybackInput(true, vi.fn()));
    await tick();
    act(() => result.current.scan());
    await tick();
    expect(api.findPlayback).toHaveBeenCalledWith(undefined);
    expect(result.current.pending).toBe("scan");
    await tick(750);
    expect(result.current.pending).toBe("scan");
    vi.mocked(api.getPlaybackStatus).mockResolvedValue(playbackStatus());
    await tick(750);
    expect(result.current.pending).toBeNull();
  });

  it("scan accepts a host so Connect needs no separate save, and cancel releases the pending state", async () => {
    vi.mocked(api.getPlaybackStatus).mockResolvedValue(playbackStatus({ scan: { state: "scanning", candidates: [], reason: null, current: 0, total: 0 } }));
    vi.mocked(api.cancelPlaybackScan).mockResolvedValue(playbackStatus());
    const { result } = renderHook(() => usePlaybackInput(true, vi.fn()));
    await tick();
    act(() => result.current.scan("192.0.2.10"));
    await tick();
    expect(api.findPlayback).toHaveBeenCalledWith("192.0.2.10");
    expect(result.current.pending).toBe("scan");
    act(() => result.current.cancelScan());
    await tick();
    expect(api.cancelPlaybackScan).toHaveBeenCalledOnce();
    expect(result.current.pending).toBeNull();
  });

  it("save resolves true on success and false with a visible error on failure", async () => {
    const { result } = renderHook(() => usePlaybackInput(true, vi.fn()));
    await tick();
    let ok: boolean | undefined;
    await act(async () => { ok = await result.current.save({ enabled: false, host: null, port: 8080, auto_scan: true, fast_transport: true }); });
    expect(ok).toBe(true);
    vi.mocked(api.updatePlaybackSettings).mockRejectedValue(new Error("Could not write settings."));
    await act(async () => { ok = await result.current.save({ enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true }); });
    expect(ok).toBe(false);
    expect(result.current.error).toBe("Could not write settings.");
  });

  it("polls progress while discovery HTTP call waits; rejects concurrent operations and failed 200 responses", async () => {
    const done = deferred<ReturnType<typeof playbackStatus>>();
    vi.mocked(api.discoverPlaybackSongOrder).mockReturnValue(done.promise);
    const { result } = renderHook(() => usePlaybackInput(true, vi.fn()));
    await tick();
    act(() => { result.current.discover(); result.current.scan(); void result.current.selectSource("real"); });
    expect(api.discoverPlaybackSongOrder).toHaveBeenCalledOnce();
    expect(api.findPlayback).not.toHaveBeenCalled();
    vi.mocked(api.getPlaybackStatus).mockResolvedValue(playbackStatus({ discovery: "running", progress: 8 }));
    await tick(750);
    expect(result.current.status?.progress).toBe(8);
    await act(async () => done.resolve(playbackStatus({ discovery: "failed", last_error: "Disconnected during discovery." })));
    expect(result.current.error).toBe("Disconnected during discovery.");
    expect(result.current.pending).toBeNull();
  });

  it("surfaces server conflicts and clears polling error on recovery", async () => {
    vi.mocked(api.getPlaybackStatus).mockRejectedValueOnce(new Error("Status unavailable."));
    const { result } = renderHook(() => usePlaybackInput(true, vi.fn()));
    await tick();
    expect(result.current.error).toBe("Status unavailable.");
    await tick(750);
    expect(result.current.error).toBeNull();
    vi.mocked(api.discoverPlaybackSongOrder).mockRejectedValue(new Error("Stop Playback first."));
    act(() => result.current.discover());
    await tick();
    expect(result.current.error).toBe("Stop Playback first.");
  });

  it("keeps the scan request serialized if concurrent status polling fails", async () => {
    const scan = deferred<ReturnType<typeof playbackStatus>>();
    vi.mocked(api.findPlayback).mockReturnValue(scan.promise);
    const { result } = renderHook(() => usePlaybackInput(true, vi.fn()));
    await tick();
    act(() => result.current.scan());
    vi.mocked(api.getPlaybackStatus).mockRejectedValue(new Error("Temporary network error."));
    await tick(750);
    expect(result.current.pending).toBe("scan");
    act(() => result.current.scan());
    expect(api.findPlayback).toHaveBeenCalledOnce();
    await act(async () => scan.resolve(playbackStatus()));
    vi.mocked(api.getPlaybackStatus).mockResolvedValue(playbackStatus());
    await tick(750);
    expect(result.current.pending).toBeNull();
  });

  it("does not let an old poll overwrite a saved connection or complete mutations after access revocation", async () => {
    const oldPoll = deferred<ReturnType<typeof playbackStatus>>();
    vi.mocked(api.getPlaybackStatus).mockReturnValueOnce(oldPoll.promise);
    const onSettings = vi.fn();
    const { result, rerender } = renderHook(({ can }) => usePlaybackInput(can, onSettings), { initialProps: { can: true } });
    act(() => { void result.current.save(playbackStatus().settings); });
    await tick();
    await act(async () => oldPoll.resolve(playbackStatus({ connected: false })));
    expect(result.current.status?.connected).toBe(true);
    const save = deferred<ReturnType<typeof playbackStatus>>();
    vi.mocked(api.updatePlaybackSettings).mockReturnValueOnce(save.promise);
    act(() => { void result.current.save(playbackStatus().settings); });
    onSettings.mockClear();
    rerender({ can: false });
    await act(async () => save.resolve(playbackStatus()));
    expect(result.current.status).toBeNull();
    expect(onSettings).not.toHaveBeenCalled();
  });
});
