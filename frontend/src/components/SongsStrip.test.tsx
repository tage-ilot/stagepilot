import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { SongsStrip, type SongChip } from "./SongsStrip";

/** The song cards of a list: each card's facts are a list of their own inside it (`FactChips`), so they are left out. */
const cardsIn = (list: HTMLElement) => within(list).getAllByRole("listitem").filter((item) => item.dataset.song !== undefined);

const chips: SongChip[] = [
  { key: "a", title: "Holy Forever", order: 1, looks: 12, handmade: 0, hasLoudness: true },
  { key: "b", title: "Goodness", order: 2, looks: 0, handmade: 0, hasLoudness: false },
  { key: "c", title: "Hand made only", order: 3, looks: 0, handmade: 2, hasLoudness: false },
];

const mount = (props: Partial<Parameters<typeof SongsStrip>[0]> = {}) => {
  const onSelect = vi.fn();
  const onClear = vi.fn();
  render(<SongsStrip chips={chips} liveKey="a" noun="looks" onClear={onClear} onSelect={onSelect} selectedKey="a" {...props} />);
  return { onSelect, onClear };
};

describe("SongsStrip", () => {
  it("has no heading and no Clear all button: that sits below the timeline, in the panel", () => {
    mount();
    expect(screen.queryByText(/Songs in this service -/)).toBeNull();
    expect(screen.queryByRole("heading")).toBeNull();
    expect(screen.getByRole("region", { name: "Songs in this service" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Clear all songs…" })).toBeNull();
  });

  it("keeps plan order and shows each state", () => {
    mount();
    const list = screen.getByRole("list", { name: "Songs in this service" });
    const items = cardsIn(list);
    expect(items.map((item) => item.getAttribute("data-song"))).toEqual(["a", "b", "c"]);
    expect(items[0]).toHaveAttribute("data-state", "captured");
    expect(items[0]).toHaveTextContent("Holy Forever");
    expect(items[0]).toHaveTextContent("12 looks");
    expect(within(items[0]!).getByTestId("song-chip-loudness")).toBeInTheDocument();
    expect(items[1]).toHaveAttribute("data-state", "empty");
    expect(items[1]).toHaveTextContent("not captured");
    expect(within(items[1]!).queryByRole("button", { name: /Clear/ })).toBeNull();
    expect(items[2]).toHaveAttribute("data-state", "captured");
    expect(items[2]).toHaveTextContent("2 cues");
  });

  it("marks the live song with a dot and the selected song as pressed", () => {
    mount({ liveKey: "a", selectedKey: "b" });
    expect(screen.getByRole("button", { name: /Holy Forever.*live/ })).toHaveAttribute("aria-pressed", "false");
    expect(within(screen.getByRole("button", { name: /Holy Forever.*live/ })).getByTestId("song-chip-live")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Goodness.*not captured/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getAllByTestId("song-chip-live")).toHaveLength(1);
  });

  it("clicking or pressing Enter on a chip selects that song, captured or not", async () => {
    const { onSelect } = mount();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /^Goodness/ }));
    expect(onSelect).toHaveBeenLastCalledWith("b");
    screen.getByRole("button", { name: /^Hand made only/ }).focus();
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenLastCalledWith("c");
  });

  it("the x beside the title clears only that song, and is its own named button", async () => {
    const { onClear, onSelect } = mount();
    await userEvent.setup().click(screen.getByRole("button", { name: "Clear cues for Holy Forever..." }));
    expect(onClear).toHaveBeenCalledWith("a");
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("has touch targets of at least 44 px even though the chip looks small", () => {
    mount();
    for (const name of [/^Holy Forever/, "Clear cues for Holy Forever..."]) {
      const button = screen.getByRole("button", { name });
      expect(button.className).toContain("min-h-[44px]");
    }
  });

  it("40 songs grow the grid to show every card: nothing scrolls inside it", () => {
    const many: SongChip[] = Array.from({ length: 40 }, (_, i) => ({ key: `s${i}`, title: `Song number ${i + 1}`, order: i + 1, looks: i % 2 ? 5 : 0, handmade: 0, hasLoudness: false }));
    mount({ chips: many, liveKey: "s3", selectedKey: "s3" });
    const list = screen.getByRole("list", { name: "Songs in this service" });
    expect(cardsIn(list)).toHaveLength(40);
    expect(list.className).toContain("grid");
    expect(list.className).not.toMatch(/max-h-|overflow-/);
  });

  it("renders nothing without a service plan", () => {
    const { container } = render(<SongsStrip chips={[]} liveKey="" noun="looks" onClear={vi.fn()} onSelect={vi.fn()} selectedKey="" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the title on its own line over the count; a very long title truncates but stays named in full", () => {
    const long = "A very long song title that goes on and on and on past any sensible chip width";
    mount({ chips: [
      { key: "n", title: "Battle Belongs", order: 1, looks: 12, handmade: 0, hasLoudness: false },
      { key: "l", title: long, order: 2, looks: 3, handmade: 0, hasLoudness: false },
    ] });
    expect(screen.getByText("Battle Belongs").className).toContain("truncate");
    expect(screen.getByText("12 looks")).toBeInTheDocument();
    expect(screen.getByText(long).className).toContain("truncate");
    expect(screen.getByText(long)).toHaveAttribute("title", long);
    expect(screen.getByRole("button", { name: new RegExp(`^${long}, 3 looks`) })).toBeInTheDocument();
  });
});

describe("SongsStrip capture labels", () => {
  const labelled: SongChip[] = [
    { key: "a", title: "A", order: 1, looks: 12, handmade: 0, hasLoudness: false, label: "fully_captured" },
    { key: "b", title: "B", order: 2, looks: 4, handmade: 0, hasLoudness: false, label: "incomplete" },
    { key: "c", title: "C", order: 3, looks: 0, handmade: 2, hasLoudness: false, label: "manually_cued" },
    { key: "d", title: "D", order: 4, looks: 9, handmade: 1, hasLoudness: false, label: "manually_cued_and_fully_captured" },
    { key: "e", title: "E", order: 5, looks: 0, handmade: 0, hasLoudness: false, label: "not_captured" },
  ];

  it("says which state each song is in instead of a look count", () => {
    mount({ chips: labelled });
    const items = cardsIn(screen.getByRole("list", { name: "Songs in this service" }));
    expect(items.map((item) => item.getAttribute("data-label"))).toEqual(["fully_captured", "incomplete", "manually_cued", "manually_cued_and_fully_captured", "not_captured"]);
    expect(items[0]).toHaveTextContent("Fully captured");
    expect(items[1]).toHaveTextContent("Incomplete capture");
    expect(items[2]).toHaveTextContent("Manually cued");
    expect(items[3]).toHaveTextContent("Manually cued & fully captured");
    expect(items[4]).toHaveTextContent("Not captured");
    expect(items[0]).not.toHaveTextContent("12 looks");
    expect(items[0]).toHaveAttribute("title", expect.stringContaining("12 captured items (12 ramps/steps, 0 loops)"));
  });

  it("uses the manual colour for hand-made only and the captured colour for captures", () => {
    mount({ chips: labelled });
    const items = cardsIn(screen.getByRole("list", { name: "Songs in this service" }));
    expect(items[2]!.className).toContain("border-cue-look-manual");
    expect(items[0]!.className).toContain("border-cue-look-captured");
    expect(items[1]!.className).toContain("border-dashed");
  });

  it("draws a song the Library Link knows as a card: cover, arrangement bar and the capture words then the BPM, but not the key or length", () => {
    const withLibrary: SongChip[] = [
      { ...chips[0]!, library: {
        facts: ["Ab (was C)", "65 BPM", "6:29 (plan 5:55)", "8 sections", "Default"],
        title: "Playback: Ab (was C), 65 BPM, 6:29 (plan 5:55), 8 sections, Default. Playback counts this song down to 6:29; the plan says 5:55.",
        cover: "/api/v1/playback-library/artwork/7?v=abc", bpm: "65 BPM",
        sections: [{ name: "Verse 1", start: 0, end: 30 }, { name: "Chorus", start: 30, end: 60 }],
      } },
      chips[1]!,
    ];
    mount({ chips: withLibrary });
    const items = cardsIn(screen.getByRole("list", { name: "Songs in this service" }));
    expect(items[0]).toHaveTextContent("Holy Forever12 looks ♪ 65 BPM");
    expect(items[0]).not.toHaveTextContent(/6:29|Ab/);
    // The capture words, the loudness note and the tempo are one square chip each, with no dot or bullet between them.
    expect(within(items[0]!).getAllByRole("listitem").map((chip) => chip.textContent)).toEqual(["12 looks", "\u266A", "65 BPM"]);
    expect(items[0]!.textContent).not.toMatch(/[\u00B7\u2022]/);
    expect(within(items[0]!).getByTestId("song-chip-bpm")).toHaveClass("border-edge-medium", "bg-surface-950", "font-mono");
    expect(within(items[1]!).getAllByRole("listitem").map((chip) => chip.textContent)).toEqual(["not captured"]);
    expect(within(items[1]!).queryByTestId("song-chip-bpm")).toBeNull();
    expect(items[0]!.querySelector("img")).toHaveAttribute("src", "/api/v1/playback-library/artwork/7?v=abc");
    expect(items[0]!.querySelector("img")).toHaveAttribute("alt", "");
    expect(within(items[0]!).getByTestId("library-bar").querySelectorAll(".library-seg")).toHaveLength(2);
    // The facts are still one hover away, and in the button's spoken name.
    expect(within(items[0]!).getByRole("button", { name: /Playback counts this song down to 6:29; the plan says 5:55/ })).toBeInTheDocument();
    // A song with no library data is the same card without cover or bar.
    expect(items[1]!.querySelector("img")).toBeNull();
    expect(within(items[1]!).queryByTestId("library-bar")).toBeNull();
  });
});
