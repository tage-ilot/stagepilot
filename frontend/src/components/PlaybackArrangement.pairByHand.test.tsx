import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import { libraryController, libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
import type { LibraryPlan, LibrarySong, PairChoice } from "../types";
import { PlaybackArrangement } from "./PlaybackArrangement";
import { chipLibrary } from "./playbackLibrary";

const PLAN = ["Song One", "Oceans", "Our Name For It", "Third"].map((title, i) => ({ id: `p${i + 1}`, title, duration_seconds: 300 }));
const pb = (title: string, position: number, over: Partial<LibrarySong> = {}) => librarySong({ title, position, setlist_song_id: 2000000 + position, content_id: 5000000 + position, countdown_length_s: 297, ...over });
const choice = (song: LibrarySong, suggested = false): PairChoice => ({ setlist_song_id: song.setlist_song_id, content_id: song.content_id!, position: song.position, title: song.title, key: song.key, countdown_length_s: song.countdown_length_s, suggested });

const OCEANS = pb("Oceans Where Feet May Fail", 2, { key: "D" });
const OTHER = pb("A Completely Other Name", 3, { key: "Ab", countdown_length_s: 243 });
const LAST = pb("Last Song", 4);

/** One matched, one uncertain (Oceans), one with no match at all, one with no Playback song left to pick. */
const library = (over: Partial<LibraryPlan> = {}): LibraryPlan => libraryPlan([
  librarySongOf("Song One", 1, { library: pb("Song One", 1) }),
  librarySongOf("Oceans", 2, { status: "uncertain", confidence: "uncertain", library: null, candidate: OCEANS, pair_choices: [choice(OCEANS, true), choice(OTHER), choice(LAST)] }),
  librarySongOf("Our Name For It", 3, { status: "missing", confidence: null, library: null, pair_choices: [choice(OCEANS), choice(OTHER), choice(LAST)] }),
  librarySongOf("Third", 4, { status: "missing", confidence: null, library: null, pair_choices: [] }),
], over);

const status = () => playbackStatus({ song_order: [], song_count: 0, plan_song_count: 4, song_lengths: [], plan_songs: PLAN, skipped_song_ids: [], skipped_plan_song_ids: [] });
const mount = (plan: LibraryPlan, over: Partial<PlaybackLibraryController> = {}) => {
  const controller = libraryController("ready", { plan, ...over });
  render(<PlaybackArrangement library={controller} status={status()} />);
  return controller;
};
describe("pairing a song by hand, in the Song order section", () => {
  it("offers a plain action on a song that is only alike, and on one with no match; a matched song has none", () => {
    mount(library());
    const lines = [...document.querySelectorAll(".arr-line")] as HTMLElement[];
    expect(within(lines[0]!).queryByRole("button", { name: /Pick the Playback song/ })).toBeNull();
    expect(within(lines[1]!).getByRole("button", { name: "Pick the Playback song for Oceans" })).toHaveTextContent("Pick the Playback song");
    expect(within(lines[2]!).getByRole("button", { name: "Pick the Playback song for Our Name For It" })).toBeInTheDocument();
    // Nothing is paired by looking, and the uncertain pair is not a pair.
    expect(lines[1]!.querySelector("[data-stamp]")).toHaveTextContent("Check title");
  });

  it("says so, with no button, when Playback has no other song to pair", () => {
    mount(library());
    const lines = [...document.querySelectorAll(".arr-line")] as HTMLElement[];
    expect(within(lines[3]!).getByText("Playback has no other song to pair")).toBeInTheDocument();
    expect(within(lines[3]!).queryByRole("button", { name: /Pick/ })).toBeNull();
  });

  it("opens a short radio list (the alike song first, each with its key and length) and pairs only on Confirm", async () => {
    const user = userEvent.setup();
    const controller = mount(library());
    const line = document.querySelectorAll(".arr-line")[1] as HTMLElement;
    await user.click(within(line).getByRole("button", { name: /Pick the Playback song/ }));
    const chooser = within(line).getByRole("group", { name: "Which Playback song is “Oceans”?" });
    const radios = within(chooser).getAllByRole("radio");
    expect(radios.map((r) => r.closest("label")!.textContent)).toEqual([
      "Oceans Where Feet May FailLooks alikeD · 4:57", "A Completely Other NameAb · 4:03", "Last SongAb · 4:57",
    ]);
    const confirm = within(chooser).getByRole("button", { name: "Confirm" });
    expect(confirm).toBeDisabled();
    expect(radios.every((r) => !(r as HTMLInputElement).checked)).toBe(true);
    await user.click(radios[1]!);
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    expect(controller.pairSong).toHaveBeenCalledWith("k2", 2000003);
    expect(controller.unpairSong).not.toHaveBeenCalled();
  });

  it("Cancel closes it and pairs nothing", async () => {
    const user = userEvent.setup();
    const controller = mount(library());
    const line = document.querySelectorAll(".arr-line")[2] as HTMLElement;
    await user.click(within(line).getByRole("button", { name: /Pick the Playback song/ }));
    await user.click(within(line).getAllByRole("radio")[0]!);
    await user.click(within(line).getByRole("button", { name: "Cancel" }));
    expect(within(line).queryByRole("group")).toBeNull();
    expect(controller.pairSong).not.toHaveBeenCalled();
    // Opened again, nothing is still picked.
    await user.click(within(line).getByRole("button", { name: /Pick the Playback song/ }));
    expect(within(line).getAllByRole("radio").every((r) => !(r as HTMLInputElement).checked)).toBe(true);
  });

  it("keeps the chooser open and says why when the pair could not be saved", async () => {
    const user = userEvent.setup();
    const controller = mount(library(), { pairSong: vi.fn().mockResolvedValue(false), error: "StagePilot could not pair these two songs." });
    const line = document.querySelectorAll(".arr-line")[2] as HTMLElement;
    await user.click(within(line).getByRole("button", { name: /Pick the Playback song/ }));
    await user.click(within(line).getAllByRole("radio")[2]!);
    await user.click(within(line).getByRole("button", { name: "Confirm" }));
    expect(controller.pairSong).toHaveBeenCalledWith("k3", 2000004);
    expect(within(line).getByRole("alert")).toHaveTextContent("could not pair");
    expect(within(line).getByRole("group")).toBeInTheDocument();
  });

  it("shows a pair a person made as Paired by you, takes the Playback song's facts, and undoes it", async () => {
    const user = userEvent.setup();
    const paired = library();
    paired.songs[2] = librarySongOf("Our Name For It", 3, { confidence: "by_hand", library: OTHER, pair_choices: [], plan_seconds: 243 });
    paired.songs[1] = { ...paired.songs[1]!, pair_choices: paired.songs[1]!.pair_choices.filter((c) => c.title !== OTHER.title) };
    const controller = mount(paired);
    const line = document.querySelectorAll(".arr-line")[2] as HTMLElement;
    expect(line.querySelector("[data-stamp]")).toHaveTextContent("Paired by you");
    expect(line).toHaveTextContent("Paired by you: the plan says “Our Name For It”, Playback has “A Completely Other Name”.");
    expect(line.querySelector("p.sr-only")).toHaveTextContent("Paired by you.");
    expect(within(line).queryByRole("button", { name: /Pick the Playback song/ })).toBeNull();
    await user.click(within(line).getByRole("button", { name: "Undo the pair for Our Name For It" }));
    expect(controller.unpairSong).toHaveBeenCalledWith("k3");
    // The pair counts as linked in the summary.
    expect(screen.getByTestId("arrangement-summary")).toHaveTextContent("2 of 4 service songs go with a Playback song");
  });

  it("is only for someone who may change settings: a viewer sees the words and no button", () => {
    const paired = library();
    paired.songs[2] = librarySongOf("Our Name For It", 3, { confidence: "by_hand", library: OTHER, plan_seconds: 243 });
    mount(paired, { canPair: false });
    expect(screen.queryByRole("button", { name: /Pick the Playback song|Undo/ })).toBeNull();
    expect(screen.queryByText("Playback has no other song to pair")).toBeNull();
    expect(document.querySelectorAll(".arr-line")[2]!.querySelector("[data-stamp]")).toHaveTextContent("Paired by you");
  });

  it("says Paired by you in the song chip's hover text too, so nobody mistakes it for a match by name", () => {
    const song = librarySongOf("Our Name For It", 3, { confidence: "by_hand", library: OTHER });
    expect(chipLibrary(song, Date.now())!.title).toMatch(/^Paired by you: “Our Name For It” goes with “A Completely Other Name” in Playback\. Playback: /);
    expect(chipLibrary(librarySongOf("Song One", 1, { library: pb("Song One", 1) }), Date.now())!.title).not.toMatch(/Paired by you/);
  });
});
