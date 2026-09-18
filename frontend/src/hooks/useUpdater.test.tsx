import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  UpdateCandidate,
  UpdaterAdapter,
  UpdateProgress,
} from "../services/updater";
import { useUpdater } from "./useUpdater";

const progress: UpdateProgress = {
  downloadedBytes: 50,
  totalBytes: 100,
  percentage: 50,
  stage: "downloading",
};

const makeCandidate = (install = vi.fn().mockResolvedValue(undefined)): UpdateCandidate => ({
  currentVersion: "1.1.5",
  availableVersion: "1.2.0",
  releaseNotes: "Safer updates",
  releaseDate: "2026-07-26T00:00:00Z",
  install: async (onProgress) => {
    onProgress(progress);
    await install();
  },
});
const makeAdapter = (candidate: UpdateCandidate | null = null): UpdaterAdapter & {
  check: ReturnType<typeof vi.fn>;
  prepareRelaunch: ReturnType<typeof vi.fn>;
  relaunch: ReturnType<typeof vi.fn>;
} => ({
  isEnabled: () => true,
  check: vi.fn().mockResolvedValue(candidate),
  prepareRelaunch: vi.fn().mockResolvedValue(undefined),
  clearRelaunchMarker: vi.fn(),
  relaunch: vi.fn().mockResolvedValue(undefined),
  restoreAfterRelaunch: vi.fn().mockResolvedValue(null),
});

describe("useUpdater", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("suppresses checks when the runtime adapter is disabled", async () => {
    const adapter = makeAdapter();
    adapter.isEnabled = () => false;
    renderHook(() => useUpdater({ adapter, ready: true, startupDelayMs: 10 }));

    await act(() => vi.advanceTimersByTimeAsync(20));

    expect(adapter.check).not.toHaveBeenCalled();
  });

  it("keeps the header state current when no update is available", async () => {
    const adapter = makeAdapter(null);
    const { result } = renderHook(() =>
      useUpdater({ adapter, ready: true, startupDelayMs: 10 }),
    );

    await act(() => vi.advanceTimersByTimeAsync(20));

    expect(result.current.status).toBe("current");
    expect(result.current.availableVersion).toBeNull();
  });

  it("exposes update metadata only when an update is available", async () => {
    const adapter = makeAdapter(makeCandidate());
    const { result } = renderHook(() =>
      useUpdater({ adapter, ready: true, startupDelayMs: 10 }),
    );

    await act(() => vi.advanceTimersByTimeAsync(20));

    expect(result.current.status).toBe("available");
    expect(result.current.currentVersion).toBe("1.1.5");
    expect(result.current.availableVersion).toBe("1.2.0");
    expect(result.current.releaseNotes).toBe("Safer updates");
  });

  it("does not expose a failed background check as an update dialog", async () => {
    const adapter = makeAdapter();
    adapter.check.mockRejectedValue(new Error("GitHub unavailable"));
    const { result } = renderHook(() =>
      useUpdater({ adapter, ready: true, startupDelayMs: 10 }),
    );

    await act(() => vi.advanceTimersByTimeAsync(20));

    expect(result.current.status).toBe("error");
    expect(result.current.errorDialogOpen).toBe(false);
  });

  it("suppresses concurrent checks", async () => {
    let resolve: (value: null) => void = () => undefined;
    const adapter = makeAdapter();
    adapter.check.mockImplementation(() => new Promise<null>((next) => { resolve = next; }));
    const { result } = renderHook(() =>
      useUpdater({ adapter, ready: true, startupDelayMs: 60_000 }),
    );

    await act(async () => {
      void result.current.checkForUpdate();
      void result.current.checkForUpdate();
    });
    expect(adapter.check).toHaveBeenCalledOnce();
    await act(async () => resolve(null));
  });

  it("does not download until confirmation and relaunches only after installation", async () => {
    const install = vi.fn().mockResolvedValue(undefined);
    const adapter = makeAdapter(makeCandidate(install));
    const { result } = renderHook(() =>
      useUpdater({ adapter, ready: true, startupDelayMs: 10 }),
    );
    await act(() => vi.advanceTimersByTimeAsync(20));

    act(() => result.current.openConfirmation());
    expect(result.current.status).toBe("confirmation");
    expect(install).not.toHaveBeenCalled();

    await act(() => result.current.install());

    expect(adapter.prepareRelaunch).toHaveBeenCalledWith("1.2.0");
    expect(install).toHaveBeenCalledOnce();
    expect(adapter.relaunch).toHaveBeenCalledOnce();
    expect(result.current.status).toBe("restarting");
  });

  it("keeps StagePilot open and offers retry after installation failure", async () => {
    const adapter = makeAdapter(makeCandidate(vi.fn().mockRejectedValue(new Error("Signature rejected"))));
    const { result } = renderHook(() =>
      useUpdater({ adapter, ready: true, startupDelayMs: 10 }),
    );
    await act(() => vi.advanceTimersByTimeAsync(20));
    act(() => result.current.openConfirmation());

    await act(() => result.current.install());

    expect(adapter.relaunch).not.toHaveBeenCalled();
    expect(result.current.status).toBe("error");
    expect(result.current.errorDialogOpen).toBe(true);
    expect(result.current.error).toContain("Signature rejected");
  });

  it("shows a success message only when a valid update marker is restored", async () => {
    const adapter = makeAdapter();
    adapter.restoreAfterRelaunch = vi.fn().mockResolvedValue({
      updatedVersion: "1.2.0",
      route: null,
    });
    const { result } = renderHook(() =>
      useUpdater({ adapter, ready: true, startupDelayMs: 60_000 }),
    );

    await act(async () => undefined);

    expect(result.current.successMessage).toBe("StagePilot updated to 1.2.0.");
  });

  it("passes the current betaEnabled toggle to the adapter on every check", async () => {
    const adapter = makeAdapter(null);
    const { result, rerender } = renderHook(
      ({ betaEnabled }) => useUpdater({ adapter, ready: true, betaEnabled, startupDelayMs: 10 }),
      { initialProps: { betaEnabled: false } },
    );

    await act(() => vi.advanceTimersByTimeAsync(20));
    expect(adapter.check).toHaveBeenLastCalledWith({ betaEnabled: false });

    rerender({ betaEnabled: true });
    await act(async () => result.current.checkForUpdate());
    expect(adapter.check).toHaveBeenLastCalledWith({ betaEnabled: true });
  });
});
