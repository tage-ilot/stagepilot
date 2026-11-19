import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { readRecentAddresses, rememberAddress, useRecentAddresses } from "./useRecentAddresses";

beforeEach(() => window.localStorage.clear());

describe("recent addresses", () => {
  it("keeps the newest first, at most five, with no repeats (a name in any letter case counts once)", () => {
    for (const address of ["10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4", "10.0.0.5", "10.0.0.6"]) rememberAddress("playback", address);
    expect(readRecentAddresses("playback")).toEqual(["10.0.0.6", "10.0.0.5", "10.0.0.4", "10.0.0.3", "10.0.0.2"]);
    rememberAddress("playback", "10.0.0.3");
    expect(readRecentAddresses("playback")).toEqual(["10.0.0.3", "10.0.0.6", "10.0.0.5", "10.0.0.4", "10.0.0.2"]);
    rememberAddress("lighting", "Stage-Mac.local");
    rememberAddress("lighting", "stage-mac.LOCAL");
    expect(readRecentAddresses("lighting")).toEqual(["Stage-Mac.local"]);
  });

  it("keeps the three places apart", () => {
    rememberAddress("playback", "10.0.0.1");
    rememberAddress("lighting", "10.0.0.2");
    rememberAddress("presentation", "10.0.0.3");
    expect([readRecentAddresses("playback"), readRecentAddresses("lighting"), readRecentAddresses("presentation")]).toEqual([["10.0.0.1"], ["10.0.0.2"], ["10.0.0.3"]]);
  });

  it("never stores something that is not a bare host: empty, spaces, a URL, a port, a path", () => {
    for (const bad of ["", "   ", null, undefined, "a b", "http://10.0.0.1", "10.0.0.1/x", "user@10.0.0.1", "x".repeat(300)]) rememberAddress("playback", bad);
    expect(readRecentAddresses("playback")).toEqual([]);
  });

  it("ignores a damaged stored value and storage that refuses", () => {
    window.localStorage.setItem("stagepilot.recent-addresses.playback", "{not json");
    expect(readRecentAddresses("playback")).toEqual([]);
    window.localStorage.setItem("stagepilot.recent-addresses.playback", JSON.stringify([1, "ok.local", "a b", "ok.local"]));
    expect(readRecentAddresses("playback")).toEqual(["ok.local"]);
    const spy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("full"); });
    expect(() => rememberAddress("playback", "10.0.0.9")).not.toThrow();
    spy.mockRestore();
  });

  it("a hook in one place hears about an address another place just saved", () => {
    const { result } = renderHook(() => useRecentAddresses("lighting"));
    expect(result.current).toEqual([]);
    act(() => rememberAddress("lighting", "10.30.0.211"));
    expect(result.current).toEqual(["10.30.0.211"]);
  });
});
