import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LightingTimeline } from "./LightingTimeline";
import type { TimelineMarker } from "./lightingTimelineMath";
import { autoTimelineHeight, HEIGHT_MODE_KEY } from "./timelineHeight";

const marker = (i: number, label = "A long recorded look name here"): TimelineMarker => ({ id: `m${i}`, at: i * 10, label, color: null, type: "look-captured", captured: true });
const base = { onMove: vi.fn(), onRemove: vi.fn(), playhead: null, songLength: 200 };

beforeEach(() => {
  window.localStorage.clear();
  // jsdom has no layout: give the track a real width so labels stack and clusters form.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({ width: 900, height: 100, top: 0, left: 0, right: 900, bottom: 100, x: 0, y: 0, toJSON: () => ({}) } as DOMRect);
});
afterEach(() => { vi.restoreAllMocks(); window.localStorage.clear(); });

const track = () => screen.getByTestId("lighting-timeline-track");
const rowsUsed = () => 1 + Math.max(0, ...screen.queryAllByTestId("lighting-marker").map((node) => Number(node.querySelector("[data-label-row]")?.getAttribute("data-label-row") ?? 0)));
const height = () => Number.parseInt(track().style.height, 10);

describe("timeline toolbar", () => {
  it("is one wrapping toolbar: the leading controls, then zoom and height on the right, then the type filter on its own line", () => {
    render(<LightingTimeline {...base} markers={[marker(1)]} toolbar={<button type="button">Add cue</button>} />);
    const bar = screen.getByRole("toolbar", { name: "Timeline tools" });
    expect(bar.className).toContain("flex-wrap");
    expect(within(bar).getByRole("button", { name: "Add cue" })).toBeInTheDocument();
    const zoom = within(bar).getByRole("group", { name: "Timeline zoom" });
    expect(within(zoom).getByRole("button", { name: "Zoom out" })).toBeInTheDocument();
    expect(within(zoom).getByRole("button", { name: "Fit" })).toBeInTheDocument();
    expect(within(zoom).getByRole("button", { name: "Zoom in" })).toBeInTheDocument();
    expect(within(bar).getByRole("radiogroup", { name: "Timeline height" })).toBeInTheDocument();
    expect(zoom.closest("[data-part=\"timeline-tools-end\"]")?.className).toContain("ml-auto");
    const legend = screen.getByTestId("cue-legend");
    expect(bar.contains(legend)).toBe(false);
    expect(legend.className).toContain("justify-end");
  });

  it("every toolbar control is 34 px (44 on touch)", () => {
    render(<LightingTimeline {...base} markers={[marker(1)]} />);
    for (const name of ["Zoom out", "Fit", "Zoom in"]) expect(screen.getByRole("button", { name }).className).toContain("!h-[34px]");
  });

  it("Fit returns to the fitted zoom", async () => {
    render(<LightingTimeline {...base} markers={[marker(1)]} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Zoom in" }));
    expect(screen.getByRole("button", { name: "Fit" })).toHaveAttribute("aria-pressed", "false");
    await user.click(screen.getByRole("button", { name: "Fit" }));
    expect(screen.getByRole("button", { name: "Fit" })).toHaveAttribute("aria-pressed", "true");
  });
});

describe("timeline height", () => {
  it("Auto is the default and follows the stacked label rows (200 px with nothing to stack)", () => {
    const { unmount } = render(<LightingTimeline {...base} markers={[]} />);
    expect(screen.getByRole("radio", { name: "Auto" })).toBeChecked();
    expect(height()).toBe(200);
    unmount();
    render(<LightingTimeline {...base} markers={Array.from({ length: 12 }, (_, i) => marker(i + 1))} />);
    expect(rowsUsed()).toBeGreaterThan(2);
    expect(height()).toBe(autoTimelineHeight(rowsUsed()));
    expect(height()).toBeGreaterThan(200);
  });

  it("S and M are fixed, and the choice is kept on this device", async () => {
    const many = Array.from({ length: 12 }, (_, i) => marker(i + 1));
    const { unmount } = render(<LightingTimeline {...base} markers={many} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("radio", { name: "S" }));
    expect(height()).toBe(200);
    expect(window.localStorage.getItem(HEIGHT_MODE_KEY)).toBe("s");
    await user.click(screen.getByRole("radio", { name: "M" }));
    expect(height()).toBe(320);
    unmount();
    render(<LightingTimeline {...base} markers={many} />);
    expect(screen.getByRole("radio", { name: "M" })).toBeChecked();
    expect(height()).toBe(320);
  });

  it("the waveform and the playhead start under the label rows, never above a label", () => {
    const loudness = { values: [10, 40, 80, 40], step_seconds: 50, scale: 1 } as never;
    render(<LightingTimeline {...base} loudness={loudness} markers={Array.from({ length: 8 }, (_, i) => marker(i + 1))} playhead={60} />);
    const zone = rowsUsed() * 22 + 4;
    expect(screen.getByTestId("lighting-playhead").style.top).toBe(`${zone}px`);
    expect(screen.getByTestId("lighting-loudness").getAttribute("style")).toContain(`top: ${zone}px`);
    for (const label of document.querySelectorAll("[data-label-row]")) {
      expect((Number(label.getAttribute("data-label-row")) + 1) * 22).toBeLessThanOrEqual(zone);
    }
  });
});

describe("ruler and labels stay inside the timeline", () => {
  it("the first ruler label starts at the left edge and the last one is right-aligned to the end, so neither is clipped", () => {
    render(<LightingTimeline {...base} markers={[marker(1)]} songLength={180} />);
    const ticks = [...screen.getByTestId("lighting-timeline-ruler").querySelectorAll("span")];
    expect(ticks[0]!.className).not.toContain("translate-x");
    expect(ticks[ticks.length - 1]!.className).toContain("-translate-x-full");
  });

  it("a label near the end is shortened to fit the track instead of running past it", () => {
    render(<LightingTimeline {...base} markers={[{ ...marker(1), at: 199, label: "A very long label near the end" }]} />);
    const label = screen.getByTestId("lighting-marker-label");
    expect(Number.parseInt(label.style.maxWidth, 10)).toBeLessThanOrEqual(900 - (199 / 200) * 900);
  });
});

describe("selected cue", () => {
  it("highlights the picked marker and shows one compact strip under the timeline", async () => {
    render(<LightingTimeline {...base} markers={[marker(1, "Verse"), marker(5, "Chorus")]} />);
    const first = screen.getAllByTestId("lighting-marker")[0]!;
    await userEvent.setup().click(first);
    expect(first).toHaveAttribute("data-selected", "true");
    expect(first.querySelector("[data-testid=lighting-marker-label]")?.className).toContain("ring");
    const strip = screen.getByTestId("lighting-marker-popover");
    expect(strip.className).toContain("flex-nowrap");
    const scroll = screen.getByTestId("lighting-timeline-scroll");
    expect(scroll.compareDocumentPosition(strip) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});
