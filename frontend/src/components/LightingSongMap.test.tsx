import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LibrarySection, SongLoudness } from "../types";
import { LightingTimeline } from "./LightingTimeline";
import { SONG_MAP_KEY } from "./lightingSongMap";
import type { TimelineMarker } from "./lightingTimelineMath";
import { arrangementLabel } from "./playbackLibrary";

const marker = (i: number): TimelineMarker => ({ id: `m${i}`, at: i * 10, label: `Look ${i}`, color: null, type: "look-captured", captured: true });
const base = { onMove: vi.fn(), onRemove: vi.fn(), playhead: null, songLength: 100, markers: [marker(1), marker(2)] };
const sections: LibrarySection[] = [
  { name: "Count Off", start: 0, end: 4 }, // 36 px at 9 px/s: "Co" fits
  { name: "Verse 1", start: 4, end: 24 }, // 180 px: "V1" fits
  { name: "Vamp", start: 24, end: 26 }, // 18 px (17 px after the gap): "Vp" (12 + 8) does not
  { name: "Bridge 1", start: 26, end: 130 }, // runs past the end of the song (100 s)
  { name: "Ending", start: 100, end: 110 }, // starts at the end: no block
];
const waveform = { step_s: 0.5, peak: Array.from({ length: 200 }, (_, i) => (i % 7) * 30) };
const measure = (text: string) => text.length * 6;
const loudness: SongLoudness = { step_seconds: 1, values: [0, 10, 200, 30] } as SongLoudness;

beforeEach(() => {
  window.localStorage.clear();
  // jsdom has no layout: a 900 px track, so 9 px per second at 100 s.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({ width: 900, height: 100, top: 0, left: 0, right: 900, bottom: 100, x: 0, y: 0, toJSON: () => ({}) } as DOMRect);
});
afterEach(() => { vi.restoreAllMocks(); window.localStorage.clear(); });

const mapProps = { arrangement: sections, waveform, measureLabel: measure };
const rows = () => screen.queryByTestId("lighting-song-map");
const blocks = () => screen.queryAllByTestId("lighting-arrangement-block");
const styleOf = (node: HTMLElement) => ({ left: node.style.left, width: node.style.width, color: node.style.getPropertyValue("--c") });

describe("the song map: the Playback library's arrangement bar and waveform on the timeline", () => {
  it("shows both rows between the ruler and the cue track, inside the same scrolling content", () => {
    render(<LightingTimeline {...base} {...mapProps} />);
    const map = screen.getByTestId("lighting-song-map");
    const ruler = screen.getByTestId("lighting-timeline-ruler");
    const track = screen.getByTestId("lighting-timeline-track");
    expect(map.parentElement).toBe(track.parentElement);
    expect(ruler.nextElementSibling).toBe(map);
    expect(map.nextElementSibling).toBe(track);
    expect(screen.getByTestId("lighting-timeline-scroll").contains(map)).toBe(true);
    expect(within(map).getByTestId("lighting-arrangement")).toBeInTheDocument();
    expect(within(map).getByTestId("lighting-waveform")).toBeInTheDocument();
  });

  it("puts nothing on the timeline for a song that is not paired, has no data, or has no usable section", () => {
    for (const props of [{}, { arrangement: null, waveform }, { arrangement: [], waveform }, { arrangement: [{ name: "Verse", start: 200, end: 210 }], waveform }]) {
      const { unmount } = render(<LightingTimeline {...base} {...props} />);
      expect(rows()).toBeNull();
      expect(screen.queryByTestId("lighting-arrangement")).toBeNull();
      expect(screen.queryByTestId("lighting-waveform")).toBeNull();
      expect(screen.queryByRole("button", { name: "Song map" })).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
      unmount();
    }
  });

  it("is byte-for-byte the timeline of today when there is no library data", () => {
    const loudnessProps = { loudness, markers: [marker(1), marker(2), marker(3)] };
    const plain = render(<LightingTimeline {...base} {...loudnessProps} />);
    // React numbers the ids of the radio group per render; everything else must match to the byte.
    const html = (root: HTMLElement) => root.innerHTML.replace(/_r_\d+_/g, "_r_");
    const before = html(plain.container);
    plain.unmount();
    for (const props of [{ arrangement: null, waveform: null }, { arrangement: [], waveform }, { waveform }]) {
      const again = render(<LightingTimeline {...base} {...loudnessProps} {...props} />);
      expect(html(again.container)).toBe(before);
      again.unmount();
    }
    // The ids of the existing parts are all still there, in the same order.
    const ids = render(<LightingTimeline {...base} {...loudnessProps} />).container.querySelectorAll("[data-testid]");
    expect([...ids].map((node) => node.getAttribute("data-testid")).filter((id, i, all) => all.indexOf(id) === i)).toEqual([
      "lighting-timeline", "cue-legend", "cue-legend-chip", "cue-legend-hidden", "lighting-timeline-scroll", "lighting-timeline-ruler",
      "lighting-timeline-track", "lighting-loudness", "lighting-marker", "lighting-marker-bar", "lighting-marker-label", "lighting-marker-glyph",
    ]);
  });

  it("leaves everything else as it was when the rows are there: the cue track, the recorded loudness and the markers", () => {
    const loudnessProps = { loudness, markers: [marker(1), marker(2), marker(3)] };
    const plain = render(<LightingTimeline {...base} {...loudnessProps} />);
    const track = plain.getByTestId("lighting-timeline-track").outerHTML;
    plain.unmount();
    render(<LightingTimeline {...base} {...loudnessProps} {...mapProps} />);
    expect(screen.getByTestId("lighting-timeline-track").outerHTML).toBe(track);
    expect(screen.getByTestId("lighting-loudness")).toBeInTheDocument();
    expect(screen.getAllByTestId("lighting-marker")).toHaveLength(3);
  });

  it("draws one block per section in playing order, placed and sized by its seconds on the timeline's scale, cut at the end", () => {
    render(<LightingTimeline {...base} {...mapProps} />);
    const drawn = blocks().map(styleOf);
    // The last section (Ending, 100 s) starts at the end of the song and gets no block.
    expect(drawn.map(({ left, width }) => [left, width])).toEqual([["0%", "4%"], ["4%", "20%"], ["24%", "2%"], ["26%", "74%"]]);
    const last = drawn[drawn.length - 1]!;
    expect(parseFloat(last.left) + parseFloat(last.width)).toBe(100);
    expect(blocks().map((block) => block.className)).toEqual(Array(4).fill("song-map-seg"));
  });

  it("tints each block like Playback: its own color (the stylesheet mixes 30 % of it into the panel with a 3 px top border)", () => {
    render(<LightingTimeline {...base} {...mapProps} />);
    expect(blocks().map((block) => styleOf(block).color)).toEqual(["#A0A0A0", "#7566FF", "#DB4439", "#E55E73"]);
  });

  it("writes Playback's short form on a block only when it fits (its width plus 8 px of padding), else no text", () => {
    render(<LightingTimeline {...base} {...mapProps} />);
    expect(blocks().map((block) => block.textContent)).toEqual(["Co", "V1", "", "B1"]);
  });

  it("gives every block its full name and times as the hover and long-press text", () => {
    render(<LightingTimeline {...base} {...mapProps} />);
    expect(blocks().map((block) => block.getAttribute("title"))).toEqual([
      "Count Off · 0:00–0:04", "Verse 1 · 0:04–0:24", "Vamp · 0:24–0:26", "Bridge 1 · 0:26–2:10",
    ]);
  });

  it("changes which labels fit with the width of the track", () => {
    vi.restoreAllMocks();
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({ width: 100, height: 100, top: 0, left: 0, right: 100, bottom: 100, x: 0, y: 0, toJSON: () => ({}) } as DOMRect);
    render(<LightingTimeline {...base} {...mapProps} />);
    // 1 px a second: Verse 1 is 19 px of block, and "V1" needs 12 + 8.
    expect(blocks().map((block) => block.textContent)).toEqual(["", "", "", "B1"]);
  });

  it("names the bar for a screen reader as one picture, and the waveform is decoration", () => {
    render(<LightingTimeline {...base} {...mapProps} />);
    const bar = screen.getByTestId("lighting-arrangement");
    expect(bar).toHaveAttribute("role", "img");
    expect(bar).toHaveAttribute("aria-label", arrangementLabel(sections));
    expect(screen.getByRole("img", { name: /^Arrangement: Count Off, Verse 1/ })).toBe(bar);
    expect(screen.getByTestId("lighting-waveform")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByTestId("lighting-waveform").getAttribute("role")).toBeNull();
  });

  it("draws the waveform as ONE path in a fixed 2400 x 100 box stretched over the whole timeline, in the Playback color at about 55 %", () => {
    render(<LightingTimeline {...base} {...mapProps} />);
    const svg = screen.getByTestId("lighting-waveform");
    expect(svg).toHaveAttribute("viewBox", "0 0 2400 100");
    expect(svg).toHaveAttribute("preserveAspectRatio", "none");
    expect(svg.querySelectorAll("path")).toHaveLength(1);
    expect(svg.querySelector("path")!.getAttribute("d")).toMatch(/^M0,\d/);
    expect(svg.querySelector("path")).toHaveAttribute("fill", "currentColor");
    expect(svg.getAttribute("class")).toContain("text-integration-playback");
    expect(svg.getAttribute("class")).toContain("opacity-[0.55]");
    expect(screen.getByTestId("lighting-waveform-row")).toHaveStyle({ height: "44px" });
    expect(screen.getByTestId("lighting-arrangement")).toHaveStyle({ height: "26px" });
  });

  it("shows the bar alone when the waveform is missing, and never an error or a placeholder", () => {
    render(<LightingTimeline {...base} {...mapProps} waveform={null} />);
    expect(screen.getByTestId("lighting-arrangement")).toBeInTheDocument();
    expect(screen.queryByTestId("lighting-waveform")).toBeNull();
    expect(screen.queryByTestId("lighting-waveform-row")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("is not a way to the keyboard: the rows hold nothing focusable and the Tab order is the same plus the Song map switch", () => {
    const focusables = (root: HTMLElement) => [...root.querySelectorAll<HTMLElement>("button, [href], input, select, textarea, [tabindex]")]
      .filter((node) => node.getAttribute("tabindex") !== "-1" && !node.hasAttribute("disabled"))
      .map((node) => node.getAttribute("aria-label") ?? node.textContent ?? node.tagName);
    const without = render(<LightingTimeline {...base} />);
    const before = focusables(without.container);
    without.unmount();
    const withMap = render(<LightingTimeline {...base} {...mapProps} />);
    const after = focusables(withMap.container);
    expect(after.filter((name) => name !== "Song map")).toEqual(before);
    expect(after.indexOf("Song map")).toBe(after.indexOf("Zoom in") + 1);
    const map = screen.getByTestId("lighting-song-map");
    expect(map.querySelectorAll("button, [href], input, select, textarea, [tabindex]")).toHaveLength(0);
  });

  it("has a Song map switch in the toolbar: on by default, off hides both rows, and the choice is remembered in this browser", async () => {
    const user = userEvent.setup();
    const first = render(<LightingTimeline {...base} {...mapProps} />);
    const toolbar = screen.getByRole("toolbar", { name: "Timeline tools" });
    const toggle = within(toolbar).getByRole("button", { name: "Song map" });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(toggle.className).toContain("!h-[34px]");
    expect(window.localStorage.getItem(SONG_MAP_KEY)).toBeNull();
    await user.click(toggle);
    expect(rows()).toBeNull();
    expect(screen.getByRole("button", { name: "Song map" })).toHaveAttribute("aria-pressed", "false");
    expect(window.localStorage.getItem(SONG_MAP_KEY)).toBe("false");
    first.unmount();
    // Another window of this browser starts the way it was left.
    render(<LightingTimeline {...base} {...mapProps} />);
    expect(rows()).toBeNull();
    await user.click(screen.getByRole("button", { name: "Song map" }));
    expect(rows()).not.toBeNull();
    expect(window.localStorage.getItem(SONG_MAP_KEY)).toBe("true");
  });

  it("can be driven by its panel (the panel decides whether the waveform is even asked for)", async () => {
    const user = userEvent.setup();
    const onSongMapChange = vi.fn();
    const { rerender } = render(<LightingTimeline {...base} {...mapProps} onSongMapChange={onSongMapChange} songMap={false} />);
    expect(rows()).toBeNull();
    await user.click(screen.getByRole("button", { name: "Song map" }));
    expect(onSongMapChange).toHaveBeenCalledWith(true);
    rerender(<LightingTimeline {...base} {...mapProps} onSongMapChange={onSongMapChange} songMap />);
    expect(rows()).not.toBeNull();
  });

  it("follows the zoom: the rows live in the same sized box as the track, so a wider track widens them", async () => {
    const user = userEvent.setup();
    render(<LightingTimeline {...base} {...mapProps} />);
    const box = screen.getByTestId("lighting-song-map").parentElement!;
    expect(box).toBe(screen.getByTestId("lighting-timeline-track").parentElement);
    await user.click(screen.getByRole("button", { name: "Zoom in" }));
    expect(box.style.width).toBe("600px"); // 100 s at 6 px/s
    expect(screen.getByTestId("lighting-song-map").parentElement).toBe(box);
  });

  it("uses the timeline's own length: a countdown shorter than the arrangement cuts the bar and the waveform", () => {
    render(<LightingTimeline {...base} {...mapProps} songLength={20} />);
    expect(blocks().map((block) => block.getAttribute("title"))).toEqual(["Count Off · 0:00–0:04", "Verse 1 · 0:04–0:24"]);
    expect(styleOf(blocks()[1]!).width).toBe("80%");
    const d = screen.getByTestId("lighting-waveform").querySelector("path")!.getAttribute("d")!;
    const xs = [...d.matchAll(/[ML](-?[\d.]+),/g)].map((m) => Number(m[1]));
    expect(Math.max(...xs)).toBe(2400);
  });
});
