import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { readWaveform, useSongWaveform } from "./useSongWaveform";

vi.mock("../api", () => ({ getPlaybackLibraryWaveform: vi.fn() }));

afterEach(() => vi.clearAllMocks());
const wave = { step_s: 0.125, peak: [0, 128, 255] };

describe("useSongWaveform", () => {
  it("asks for nothing while there is no song to ask for", async () => {
    const { result } = renderHook(() => useSongWaveform(null));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(api.getPlaybackLibraryWaveform).not.toHaveBeenCalled();
    expect(result.current).toBeNull();
  });

  it("shows nothing while it loads, then the numbers, asking once", async () => {
    vi.mocked(api.getPlaybackLibraryWaveform).mockResolvedValue(wave);
    const { result } = renderHook(() => useSongWaveform(2000001));
    expect(result.current).toBeNull();
    await waitFor(() => expect(result.current).toEqual(wave));
    expect(api.getPlaybackLibraryWaveform).toHaveBeenCalledTimes(1);
    expect(api.getPlaybackLibraryWaveform).toHaveBeenCalledWith(2000001);
  });

  it("ignores a failure silently", async () => {
    vi.mocked(api.getPlaybackLibraryWaveform).mockRejectedValue(new Error("No waveform."));
    const { result } = renderHook(() => useSongWaveform(2000001));
    await waitFor(() => expect(api.getPlaybackLibraryWaveform).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(result.current).toBeNull();
  });

  it("ignores an answer that is not the promised shape", async () => {
    vi.mocked(api.getPlaybackLibraryWaveform).mockResolvedValue({ step_s: 0, peak: [1] });
    const { result } = renderHook(() => useSongWaveform(2000001));
    await waitFor(() => expect(api.getPlaybackLibraryWaveform).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(result.current).toBeNull();
  });

  it("never shows one song's numbers for another song, and asks again for the new song", async () => {
    vi.mocked(api.getPlaybackLibraryWaveform).mockResolvedValueOnce(wave).mockResolvedValueOnce({ step_s: 1, peak: [9] });
    const { result, rerender } = renderHook(({ id }) => useSongWaveform(id), { initialProps: { id: 1 as number | null } });
    await waitFor(() => expect(result.current).toEqual(wave));
    rerender({ id: 2 });
    expect(result.current).toBeNull();
    await waitFor(() => expect(result.current).toEqual({ step_s: 1, peak: [9] }));
    rerender({ id: null });
    expect(result.current).toBeNull();
    expect(api.getPlaybackLibraryWaveform).toHaveBeenCalledTimes(2);
  });

  it("asks again when the arrangement changed (another key) but not when only the plan was read again", async () => {
    vi.mocked(api.getPlaybackLibraryWaveform).mockResolvedValue(wave);
    const { rerender } = renderHook(({ key }) => useSongWaveform(1, key), { initialProps: { key: "3:100" } });
    await waitFor(() => expect(api.getPlaybackLibraryWaveform).toHaveBeenCalledTimes(1));
    rerender({ key: "3:100" });
    expect(api.getPlaybackLibraryWaveform).toHaveBeenCalledTimes(1);
    rerender({ key: "4:120" });
    await waitFor(() => expect(api.getPlaybackLibraryWaveform).toHaveBeenCalledTimes(2));
  });
});

describe("readWaveform", () => {
  it("takes the backend's shape and nothing else", () => {
    expect(readWaveform(wave)).toEqual(wave);
    for (const bad of [null, "x", {}, { step_s: 1 }, { step_s: 1, peak: [] }, { step_s: -1, peak: [1] }, { step_s: 1, peak: [1, "2"] }, { step_s: 1, peak: [1, -2] }, { step_s: Number.NaN, peak: [1] }]) {
      expect(readWaveform(bad)).toBeNull();
    }
  });
});
