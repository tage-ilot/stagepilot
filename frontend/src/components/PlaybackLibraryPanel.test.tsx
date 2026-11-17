import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { LibraryPlan, LibraryStatus } from "../types";
import { PlaybackLibraryPanel } from "./PlaybackLibraryPanel";

const status = (over: Partial<LibraryStatus> = {}): LibraryStatus => ({
  enabled: true, address: "http://10.20.0.102", state: "ready", message: "Playback library 0.1.1 is ready: 12 songs in 3 setlists.",
  version: "0.1.1", songs: 12, setlists: 3, stale: false, checked_at: null, ...over,
});
const planOf = (over: Partial<LibraryPlan> = {}): LibraryPlan => ({
  enabled: true, state: "ready", message: null, plan_date: "2026-11-22", setlist: { title: "Set", date: "2026-11-22", songs: 2 }, newest: null, stale: false, checked_at: null,
  songs: [
    { key: "a", title: "Song One", plan_position: 1, plan_seconds: 355, status: "matched", library: { setlist_song_id: 1, position: 1, title: "Song One", key: "Ab", key_original: "C", bpm: 65, scan_length_s: 388.64, countdown_length_s: 388.64, length_source: "last_measure", arrangement: "Default", section_count: 8, sections: [], artwork: null, rental_expires_utc: null } },
    { key: "b", title: "Song Two", plan_position: 2, plan_seconds: 300, status: "missing", library: null },
  ],
  ...over,
});
const controller = (over: Partial<PlaybackLibraryController> = {}): PlaybackLibraryController => ({
  enabled: true, address: "http://10.20.0.102", status: status(), plan: planOf(), error: null, pending: null,
  save: vi.fn().mockResolvedValue(true), check: vi.fn(), ...over,
});
const mount = (library: PlaybackLibraryController) =>
  render(<PlaybackLibraryPanel library={library} plan={{ date: "2026-11-22", service_times: ["10:00"] } as never} timeZone="America/Los_Angeles" />);

describe("Playback library panel", () => {
  it("is off by default: says so, asks for nothing and has nothing to check", () => {
    mount(controller({ enabled: false, address: "", status: null, plan: null }));
    expect(screen.getByRole("switch", { name: "Use the Playback library" })).not.toBeChecked();
    expect(screen.getByTestId("library-status")).toHaveTextContent("Off. StagePilot does not ask the Playback computer for anything.");
    expect(screen.getByRole("button", { name: "Check" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.queryByTestId("library-songs")).toBeNull();
  });

  it("saves the switch and the address, then checks with the Check button", async () => {
    const library = controller({ enabled: false, address: "", status: null, plan: null });
    mount(library);
    const user = userEvent.setup();
    await user.click(screen.getByRole("switch", { name: "Use the Playback library" }));
    await user.type(screen.getByLabelText("Playback library address"), "10.20.0.102");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(library.save).toHaveBeenCalledWith(true, "10.20.0.102");
  });

  it("checks the helper now and shows its words", async () => {
    const library = controller();
    mount(library);
    await userEvent.setup().click(screen.getByRole("button", { name: "Check" }));
    expect(library.check).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("library-status")).toHaveTextContent("Playback library 0.1.1 is ready: 12 songs in 3 setlists.");
  });

  it("shows each plan song as a chip: title, key, BPM and the countdown length (sections and arrangement are in the tooltip)", () => {
    mount(controller());
    const chips = within(screen.getByTestId("library-songs")).getAllByRole("listitem");
    expect(chips).toHaveLength(2);
    expect(chips[0]).toHaveTextContent("Song OneAb (was C) · 65 BPM · 6:29");
    expect(chips[0]).not.toHaveTextContent("sections");
    expect(chips[0]).not.toHaveTextContent("Default");
    expect(chips[0]).toHaveAttribute("title", expect.stringContaining("8 sections, Default"));
    // A song Playback does not have keeps its words, as a chip of the same shape: no cover, no bar.
    expect(chips[1]).toHaveTextContent("Song Twonot in Playback's setlist");
    expect(chips[1]).toHaveClass("rounded-ds-xl", "border");
    expect(chips[1]!.querySelector("img")).toBeNull();
    expect(within(chips[1]!).queryByRole("img")).toBeNull();
  });

  it("puts the chips in a responsive grid of 270 px columns with a 10 px gap", () => {
    mount(controller());
    const grid = screen.getByTestId("library-songs");
    expect(grid.className).toContain("grid-template-columns:repeat(auto-fill,minmax(270px,1fr))");
    expect(grid.className).toContain("gap-[10px]");
  });

  it("lists every notice: the actionable ones and the length, which is only information", () => {
    mount(controller());
    const notices = within(screen.getByTestId("library-notices")).getAllByRole("listitem").map((item) => item.textContent);
    expect(notices).toEqual([
      "Song One: Playback counts this song down to 6:29; the plan says 5:55.",
      "Song Two is not in Playback's setlist.",
    ]);
  });

  it("has no notices when everything agrees", () => {
    const agree = planOf({ songs: [planOf().songs[0]!] });
    agree.songs[0]!.plan_seconds = 389;
    mount(controller({ plan: agree }));
    expect(screen.queryByTestId("library-notices")).toBeNull();
  });

  it("says it plainly when the helper is not ready yet or does not answer", () => {
    mount(controller({ status: status({ state: "waiting", message: "The Playback library is running but not ready yet: macOS has to be told to allow it. Click Allow on the Playback computer, or wait a moment." }), plan: planOf({ state: "waiting", songs: [] }) }));
    expect(screen.getByTestId("library-status")).toHaveTextContent("not ready yet");
    expect(screen.queryByTestId("library-songs")).toBeNull();
  });
});
