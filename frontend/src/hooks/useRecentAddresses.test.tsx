import { act, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { RecentAddresses } from "../components/RecentAddresses";
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

describe("<RecentAddresses>", () => {
  it("draws nothing until there is a recent address that is not already in the box", () => {
    const { container, rerender } = render(<RecentAddresses onPick={vi.fn()} scope="lighting" value="" />);
    expect(container).toBeEmptyDOMElement();
    rememberAddress("lighting", "10.30.0.211");
    rerender(<RecentAddresses onPick={vi.fn()} scope="lighting" value="10.30.0.211" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("remembers the working address, lists the others as one-tap buttons and hands the tapped one back", async () => {
    rememberAddress("lighting", "10.30.0.50");
    const onPick = vi.fn();
    const { rerender } = render(<RecentAddresses onPick={onPick} scope="lighting" value="" working="10.30.0.211" />);
    expect(readRecentAddresses("lighting")).toEqual(["10.30.0.211", "10.30.0.50"]);
    const group = screen.getByRole("group", { name: "Recent addresses" });
    expect(group).toHaveTextContent("Recent:10.30.0.21110.30.0.50");
    await userEvent.setup().click(screen.getByRole("button", { name: "Use 10.30.0.50" }));
    expect(onPick).toHaveBeenCalledWith("10.30.0.50");
    // Not working (disconnected): nothing new is remembered.
    rerender(<RecentAddresses onPick={onPick} scope="lighting" value="" working={null} />);
    expect(readRecentAddresses("lighting")).toEqual(["10.30.0.211", "10.30.0.50"]);
  });
});
