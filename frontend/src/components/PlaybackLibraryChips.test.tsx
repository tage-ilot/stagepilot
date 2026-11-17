import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { LibraryPlan, LibrarySection, LibrarySong } from "../types";
import { libraryController, libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import { PlaybackLibrarySongs } from "./PlaybackLibrarySongs";
import { arrangementLabel } from "./playbackLibrary";

const sectionsOf = (...parts: Array<[string, number, number]>): LibrarySection[] => parts.map(([name, start, end]) => ({ name, start, end }));
const library = (over: Partial<LibrarySong> = {}): LibrarySong => librarySong({ sections: sectionsOf(["Count Off", 0, 4], ["Verse 1", 4, 24], ["Vamp on 1", 24, 28]), ...over });
const planSong = librarySongOf;
const planOf = libraryPlan;
const mount = (plan: LibraryPlan | null, over: Partial<PlaybackLibraryController> = {}) =>
  render(<PlaybackLibrarySongs
    library={libraryController("ready", { plan, ...over })}
    plan={{ date: "2026-11-22", service_times: ["10:00"] } as never}
    timeZone="America/Los_Angeles"
  />);
const chips = () => within(screen.getByTestId("library-songs")).getAllByRole("listitem");

describe("the song chips in the Playback config", () => {
  it("shows one chip per plan song, in plan order, with the title and key, BPM and length under it", () => {
    mount(planOf([planSong("Song Two", 1), planSong("Song One", 2, { library: library({ key: "G", key_original: "G", bpm: 72.4, countdown_length_s: 300 }) })]));
    const [first, second] = chips();
    expect(first).toHaveTextContent("Song TwoAb (was C) · 65 BPM · 6:29");
    expect(second).toHaveTextContent("Song OneG · 72.4 BPM · 5:00");
    expect(chips().map((chip) => chip.dataset.song)).toEqual(["k1", "k2"]);
  });

  it("shows a 28 px cover with no alt text, loaded lazily, fetched through StagePilot with the picture's etag in the address", () => {
    mount(planOf([planSong("Song One", 1)]));
    const cover = chips()[0]!.querySelector("img")!;
    expect(cover).toHaveAttribute("alt", "");
    expect(cover).toHaveAttribute("loading", "lazy");
    expect(cover).toHaveAttribute("decoding", "async");
    expect(cover).toHaveAttribute("width", "28");
    expect(cover).toHaveAttribute("height", "28");
    expect(cover.className).toMatch(/\bh-7\b/);
    expect(cover.className).toMatch(/\bw-7\b/);
    const src = cover.getAttribute("src")!;
    expect(src).toContain("/api/v1/playback-library/songs/2000001/artwork?v=");
    expect(decodeURIComponent(src.split("?v=")[1]!)).toBe("\"0123456789abcdef\"");
    // The window never names the Playback computer.
    expect(src).not.toContain("10.20.0.102");
    expect(src).not.toContain("8780");
  });

  it("a changed picture has another address, so it is fetched again", () => {
    const { unmount } = mount(planOf([planSong("Song One", 1)]));
    const before = chips()[0]!.querySelector("img")!.getAttribute("src");
    unmount();
    mount(planOf([planSong("Song One", 1, { library: library({ artwork: { type: "image/png", bytes: 10, etag: "\"fedcba9876543210\"" } }) })]));
    expect(chips()[0]!.querySelector("img")!.getAttribute("src")).not.toBe(before);
  });

  it("shows no cover box at all for a song with no picture (an old helper, or none stored)", () => {
    mount(planOf([planSong("Song One", 1, { library: library({ artwork: null }) })]));
    expect(chips()[0]!.querySelector("img")).toBeNull();
    expect(chips()[0]).toHaveTextContent("Song OneAb (was C) · 65 BPM · 6:29");
  });

  it("draws the arrangement as one block per section, in order, as wide as the time it plays, tinted like Playback", () => {
    mount(planOf([planSong("Song One", 1)]));
    const bar = within(chips()[0]!).getByRole("img");
    expect(bar).toHaveClass("library-bar");
    const blocks = Array.from(bar.children) as HTMLElement[];
    expect(blocks).toHaveLength(3);
    expect(blocks.map((block) => block.style.getPropertyValue("--w"))).toEqual(["4", "20", "4"]);
    expect(blocks.map((block) => block.style.getPropertyValue("--c"))).toEqual(["#A0A0A0", "#7566FF", "#0096FF"]);
    expect(blocks.every((block) => block.classList.contains("library-seg"))).toBe(true);
    // No words on the bar, and the blocks are not tab stops.
    expect(blocks.every((block) => block.textContent === "" && !block.hasAttribute("tabindex") && block.tagName === "SPAN")).toBe(true);
    expect(bar.querySelectorAll("a, button, [tabindex]")).toHaveLength(0);
  });

  it("gives each block its name and times for the mouse, and the whole bar one spoken label", () => {
    mount(planOf([planSong("Song One", 1, { library: library({ sections: sectionsOf(["Count Off", 0, 3.61], ["Verse 1", 3.61, 75.9], ["Vamp on 1", 75.9, 3725]) }) })]));
    const bar = within(chips()[0]!).getByRole("img");
    expect(Array.from(bar.children).map((block) => block.getAttribute("title"))).toEqual([
      "Count Off · 0:00–0:03", "Verse 1 · 0:03–1:15", "Vamp on 1 · 1:15–62:05",
    ]);
    expect(bar).toHaveAttribute("aria-label", "Arrangement: Count Off, Verse 1, Vamp on 1 (3 sections)");
  });

  it("names at most twelve sections aloud, then how many more", () => {
    const many = sectionsOf(...Array.from({ length: 30 }, (_, i): [string, number, number] => [i === 0 ? "Count Off" : `Verse ${i}`, i * 10, i * 10 + 10]));
    expect(arrangementLabel(many)).toBe(`Arrangement: Count Off, ${Array.from({ length: 11 }, (_, i) => `Verse ${i + 1}`).join(", ")} and 18 more (30 sections)`);
    expect(arrangementLabel(many.slice(0, 12))).toBe(`Arrangement: Count Off, ${Array.from({ length: 11 }, (_, i) => `Verse ${i + 1}`).join(", ")} (12 sections)`);
    expect(arrangementLabel(many.slice(0, 1))).toBe("Arrangement: Count Off (1 section)");
    mount(planOf([planSong("Song One", 1, { library: library({ sections: many }) })]));
    expect(within(chips()[0]!).getByRole("img")).toHaveAttribute("aria-label", expect.stringMatching(/ and 18 more \(30 sections\)$/));
  });

  it("repeats stay separate blocks and a name the table does not know is grey", () => {
    mount(planOf([planSong("Song One", 1, { library: library({ sections: sectionsOf(["Chorus", 0, 10], ["Chorus", 10, 20], ["Guitar Feature", 20, 30], ["Vamp", 30, 31]) }) })]));
    const blocks = Array.from(within(chips()[0]!).getByRole("img").children) as HTMLElement[];
    expect(blocks.map((block) => block.style.getPropertyValue("--c"))).toEqual(["#FC620F", "#FC620F", "#A0A0A0", "#DB4439"]);
  });

  it("puts the number of sections and the arrangement in the tooltip, not on the face", () => {
    mount(planOf([planSong("Song One", 1)]));
    expect(chips()[0]).toHaveAttribute("title", expect.stringContaining("3 sections, Default"));
    expect(within(chips()[0]!).queryByText(/sections/)).toBeNull();
  });

  it("draws no bar for a song with no usable section, but still its cover and facts", () => {
    mount(planOf([planSong("Song One", 1, { library: library({ sections: [], section_count: null }) })]));
    expect(within(chips()[0]!).queryByRole("img")).toBeNull();
    expect(chips()[0]!.querySelector("img")).not.toBeNull();
    expect(chips()[0]).toHaveTextContent("Song OneAb (was C) · 65 BPM · 6:29");
  });

  it("keeps the words for a song Playback is missing or cannot tell apart, as a chip with no cover and no bar", () => {
    mount(planOf([
      planSong("Song One", 1),
      planSong("Song Two", 2, { status: "missing", library: null }),
      planSong("Song Three", 3, { status: "duplicate_in_plan", library: null }),
      planSong("Song Four", 4, { status: "duplicate_in_playback", library: null }),
    ]));
    const rows = chips();
    expect(rows).toHaveLength(4);
    expect(rows[1]).toHaveTextContent("Song Twonot in Playback's setlist");
    expect(rows[2]).toHaveTextContent("Song Threeappears more than once, so StagePilot cannot tell which Playback song it is");
    expect(rows[3]).toHaveTextContent("Song Fourappears more than once, so StagePilot cannot tell which Playback song it is");
    for (const row of rows.slice(1)) {
      expect(row.querySelector("img")).toBeNull();
      expect(row).toHaveClass(...chips()[0]!.className.split(" ").filter((name) => name.startsWith("rounded") || name === "border"));
      expect(row).not.toHaveAttribute("title");
    }
  });

  it("shows no chips unless the helper has current data: nothing at all for the states that have none", () => {
    for (const state of ["waiting", "problem", "off"] as const) {
      const { unmount } = mount(planOf([planSong("Song One", 1)], { state, message: "Words for the state." }));
      expect(screen.queryByTestId("playback-library")).toBeNull();
      expect(screen.queryByTestId("library-songs")).toBeNull();
      expect(screen.queryByRole("img")).toBeNull();
      unmount();
    }
    // Another date is the one notice that stands without chips.
    mount(planOf([], { state: "no_setlist", message: "Playback has no setlist for 2026-11-22; its newest is Set from 2026-11-15." }));
    expect(screen.queryByTestId("library-songs")).toBeNull();
    expect(screen.getByTestId("library-notices")).toHaveTextContent("Playback has no setlist for 2026-11-22");
  });

  it("keeps showing the chips (marked old by the status) while Playback does not answer for a moment", () => {
    mount(planOf([planSong("Song One", 1)], { stale: true, message: "Playback did not answer just now; this is what it said a moment ago." }));
    expect(chips()).toHaveLength(1);
  });
});
