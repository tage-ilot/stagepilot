import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { ProPresenterScan } from "../propresenterScan";
import { SCAN_POLL_MS, useProPresenterScan } from "./useProPresenterScan";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getProPresenterScan: vi.fn(),
  startProPresenterScan: vi.fn(),
  cancelProPresenterScan: vi.fn(),
}));

const scan = (over: Partial<ProPresenterScan> = {}): ProPresenterScan => ({
  state: "idle", candidates: [], current: 0, total: 0, phase: null, elapsed: 0, reason: null, error_class: null, networks: [], hosts_probed: 0, hosts_total: 0, details: null, settings_url: null, ...over,
} as ProPresenterScan);

beforeEach(() => { vi.useFakeTimers({ shouldAdvanceTime: true }); });
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });

describe("useProPresenterScan", () => {
  it("asks nothing until Scan is pressed", () => {
    const { result } = renderHook(() => useProPresenterScan());
    expect(result.current.scan.state).toBe("idle");
    expect(api.startProPresenterScan).not.toHaveBeenCalled();
    expect(api.getProPresenterScan).not.toHaveBeenCalled();
  });

  it("start posts once, then polls while it scans and stops when it ends, with the computers in the one scan shape", async () => {
    vi.mocked(api.startProPresenterScan).mockResolvedValue(scan({ state: "scanning", current: 0, total: 255 }));
    vi.mocked(api.getProPresenterScan)
      .mockResolvedValueOnce(scan({ state: "scanning", current: 100, total: 255 }))
      .mockResolvedValueOnce(scan({ state: "found", current: 255, total: 255, candidates: [{ host: "192.0.2.21", port: 11025, name: "Front MacBook", version: "21.0" }] } as Partial<ProPresenterScan>));
    const { result } = renderHook(() => useProPresenterScan());
    act(() => result.current.start());
    await waitFor(() => expect(result.current.scan.state).toBe("scanning"));
    expect(api.startProPresenterScan).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(SCAN_POLL_MS + 10); });
    expect(result.current.scan.current).toBe(100);
    await act(async () => { await vi.advanceTimersByTimeAsync(SCAN_POLL_MS + 10); });
    expect(result.current.scan.state).toBe("found");
    expect(result.current.scan.candidates).toEqual([expect.objectContaining({ host: "192.0.2.21", port: 11025, label: "Front MacBook" })]);
    await act(async () => { await vi.advanceTimersByTimeAsync(SCAN_POLL_MS * 3); });
    expect(api.getProPresenterScan).toHaveBeenCalledTimes(2);
  });

  it("cancel posts the cancel and stops polling", async () => {
    vi.mocked(api.startProPresenterScan).mockResolvedValue(scan({ state: "scanning", total: 255 }));
    vi.mocked(api.cancelProPresenterScan).mockResolvedValue(scan({ state: "not_found" }));
    const { result } = renderHook(() => useProPresenterScan());
    act(() => result.current.start());
    await waitFor(() => expect(result.current.scan.state).toBe("scanning"));
    act(() => result.current.cancel());
    await waitFor(() => expect(result.current.scan.state).toBe("not_found"));
    await act(async () => { await vi.advanceTimersByTimeAsync(SCAN_POLL_MS * 3); });
    expect(api.getProPresenterScan).not.toHaveBeenCalled();
  });

  it("a macOS permission block is the one permission_denied shape; a request that fails says why", async () => {
    vi.mocked(api.startProPresenterScan).mockResolvedValueOnce(scan({ state: "not_found", error_class: "permission_denied", settings_url: "x-apple.systempreferences:a" }));
    const { result } = renderHook(() => useProPresenterScan());
    act(() => result.current.start());
    await waitFor(() => expect(result.current.scan.error).toBe("permission_denied"));
    expect(result.current.scan.settingsUrl).toBe("x-apple.systempreferences:a");
    vi.mocked(api.startProPresenterScan).mockRejectedValueOnce(new Error("no route"));
    act(() => result.current.start());
    await waitFor(() => expect(result.current.error).toBeTruthy());
  });
});
