import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { helperStatus } from "../test/propresenterLinkFixtures";
import { LINK_REFRESH_MS, LINK_WATCH_MS, useProPresenterLink } from "./useProPresenterLink";

vi.mock("../api", () => ({ getProPresenterLinkStatus: vi.fn() }));

beforeEach(() => { vi.mocked(api.getProPresenterLinkStatus).mockResolvedValue(helperStatus("ready")); });
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });

describe("useProPresenterLink", () => {
  it("asks nothing until a ProPresenter computer is saved AND a window that shows the answer is open", async () => {
    vi.useFakeTimers();
    renderHook(() => useProPresenterLink(false, true));
    renderHook(() => useProPresenterLink(true, false));
    renderHook(() => useProPresenterLink(false, false));
    await act(async () => { await vi.advanceTimersByTimeAsync(LINK_REFRESH_MS * 3); });
    expect(api.getProPresenterLinkStatus).not.toHaveBeenCalled();
  });

  it("asks at once, then every 30 seconds, and keeps the answer", async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useProPresenterLink(true, true));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(api.getProPresenterLinkStatus).toHaveBeenCalledTimes(1);
    expect(api.getProPresenterLinkStatus).toHaveBeenLastCalledWith(false);
    expect(result.current.status?.state).toBe("ready");
    await act(async () => { await vi.advanceTimersByTimeAsync(LINK_REFRESH_MS); });
    expect(api.getProPresenterLinkStatus).toHaveBeenCalledTimes(2);
  });

  it("looks every few seconds while a setup box waits", async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useProPresenterLink(true, true));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    act(() => result.current.watch(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(LINK_WATCH_MS * 2 + 50); });
    expect(vi.mocked(api.getProPresenterLinkStatus).mock.calls.length).toBeGreaterThanOrEqual(3);
  });

  it("Check asks again now with check=1 and says it is pending", async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useProPresenterLink(true, true));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    await act(async () => { result.current.check(); await vi.advanceTimersByTimeAsync(5); });
    expect(api.getProPresenterLinkStatus).toHaveBeenLastCalledWith(true);
    expect(result.current.pending).toBeNull();
  });

  it("a read that fails is a sentence, and the next good read clears it", async () => {
    vi.useFakeTimers();
    vi.mocked(api.getProPresenterLinkStatus).mockRejectedValueOnce(new TypeError("Failed to fetch")).mockResolvedValue(helperStatus("ready"));
    const { result } = renderHook(() => useProPresenterLink(true, true));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(result.current.error).toMatch(/Can't reach StagePilot/);
    await act(async () => { await vi.advanceTimersByTimeAsync(LINK_REFRESH_MS); });
    expect(result.current.error).toBeNull();
  });

  it("forgets what it knew when no ProPresenter computer is saved any more", async () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ on }) => useProPresenterLink(on, true), { initialProps: { on: true } });
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(result.current.status).not.toBeNull();
    rerender({ on: false });
    expect(result.current.status).toBeNull();
  });

  it("does not ask while the page is hidden", async () => {
    vi.useFakeTimers();
    const hidden = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    renderHook(() => useProPresenterLink(true, true));
    await act(async () => { await vi.advanceTimersByTimeAsync(LINK_REFRESH_MS * 2); });
    expect(api.getProPresenterLinkStatus).not.toHaveBeenCalled();
    hidden.mockRestore();
  });
});
