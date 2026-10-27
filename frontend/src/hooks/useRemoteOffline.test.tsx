import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { REMOTE_OFFLINE_GRACE_MS, useRemoteOffline } from "./useRemoteOffline";

const status = (over: Partial<api.RemoteStatus>): api.RemoteStatus => ({
  available: true, provisioned: true, credential_available: true, enabled: true, state: "connected",
  url: null, needs_operator: false, message: null, temporary_url: false, permanently_revoked: false, ...over,
});

async function advance(ms: number) {
  await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
}

describe("useRemoteOffline", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

  it("stays false while Remote Access is off, even if the tunnel reports an error", async () => {
    const get = vi.spyOn(api, "getRemoteStatus").mockResolvedValue(status({ enabled: false, state: "off" }));
    const { result } = renderHook(() => useRemoteOffline(true));
    await advance(REMOTE_OFFLINE_GRACE_MS * 2);
    expect(get).toHaveBeenCalled();
    expect(result.current).toBe(false);
  });

  it("stays false while it is connected", async () => {
    vi.spyOn(api, "getRemoteStatus").mockResolvedValue(status({ state: "connected" }));
    const { result } = renderHook(() => useRemoteOffline(true));
    await advance(REMOTE_OFFLINE_GRACE_MS * 2);
    expect(result.current).toBe(false);
  });

  it("waits out the retries: false during the grace period, true once it has passed", async () => {
    vi.spyOn(api, "getRemoteStatus").mockResolvedValue(status({ state: "reconnecting" }));
    const { result } = renderHook(() => useRemoteOffline(true));
    await advance(REMOTE_OFFLINE_GRACE_MS - 10_000);
    expect(result.current).toBe(false);
    await advance(20_000);
    expect(result.current).toBe(true);
  });

  it("counts an error state and a tunnel that is still enabling the same way", async () => {
    for (const state of ["error", "enabling"] as const) {
      vi.spyOn(api, "getRemoteStatus").mockResolvedValue(status({ state }));
      const { result, unmount } = renderHook(() => useRemoteOffline(true));
      await advance(REMOTE_OFFLINE_GRACE_MS + 10_000);
      expect(result.current, state).toBe(true);
      unmount();
    }
  });

  it("clears at once on reconnect and restarts the wait for the next outage", async () => {
    const get = vi.spyOn(api, "getRemoteStatus").mockResolvedValue(status({ state: "reconnecting" }));
    const { result } = renderHook(() => useRemoteOffline(true));
    await advance(REMOTE_OFFLINE_GRACE_MS + 10_000);
    expect(result.current).toBe(true);
    get.mockResolvedValue(status({ state: "connected" }));
    await advance(10_000);
    expect(result.current).toBe(false);
    get.mockResolvedValue(status({ state: "reconnecting" }));
    await advance(REMOTE_OFFLINE_GRACE_MS - 20_000);
    expect(result.current).toBe(false);
  });

  it("ignores failed status requests (neither raises nor clears it)", async () => {
    const get = vi.spyOn(api, "getRemoteStatus").mockRejectedValue(new Error("down"));
    const { result } = renderHook(() => useRemoteOffline(true));
    await advance(REMOTE_OFFLINE_GRACE_MS * 2);
    expect(result.current).toBe(false);
    get.mockResolvedValue(status({ state: "reconnecting" }));
    await advance(REMOTE_OFFLINE_GRACE_MS + 10_000);
    expect(result.current).toBe(true);
    get.mockRejectedValue(new Error("down"));
    await advance(30_000);
    expect(result.current).toBe(true);
  });

  it("does not ask at all for Viewers and remote sessions", async () => {
    const get = vi.spyOn(api, "getRemoteStatus").mockResolvedValue(status({ state: "reconnecting" }));
    const { result } = renderHook(() => useRemoteOffline(false));
    await advance(REMOTE_OFFLINE_GRACE_MS * 2);
    expect(get).not.toHaveBeenCalled();
    expect(result.current).toBe(false);
  });
});
