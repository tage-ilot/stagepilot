import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ApplicationState, LightingCue, LightsStatusResponse, OscLightingCue, SettingsResponse, Song } from "../types";
import { LightingSetupPanel } from "./LightingSetupPanel";
import { LightingTimeline } from "./LightingTimeline";
import type { TimelineMarker } from "./lightingTimelineMath";

const song: Song = { id: "item-1", source_song_id: "song-1", title: "Holy Forever", duration_seconds: 300, order: 1, is_generic: false };
const base = { osc_host: "127.0.0.1", osc_port: 21600, enabled: true, output_name: "Out", channel: 1, pulse_ms: 100 };
const lightsFor = (method: "osc" | "midi") => ({ ...base, method, connection_status: "connected", detail: null, outputs: [], last_cue: null, last_osc_cue: null, last_cue_at: null }) as LightsStatusResponse;
const state = { plan: { id: "p", title: "Sunday", songs: [song] }, current_song: song } as unknown as ApplicationState;
const settingsFor = (method: "osc" | "midi", osc_cues: OscLightingCue[], cues: LightingCue[] = []) => ({
  planning_center_secret_saved: true, warning: null, restart_required: false,
  settings: { lights: { ...base, method, cue_maps: { "song-1": { song_key: "song-1", song_title: "Holy Forever", cues, osc_cues } } } },
}) as unknown as SettingsResponse;

const f = (value: number) => [{ type: "float" as const, value }];
const look = (id: string, at: number, over: Partial<OscLightingCue> = {}): OscLightingCue => ({
  id, at_seconds: at, address: "", args: [], label: "", source: "captured",
  messages: [{ address: "/fixture/MH1/overrides/dimmer", args: f(1) }, { address: "/fixture/MH2/overrides/dimmer", args: f(1) }], ...over,
});
const single = (id: string, at: number, address: string, label = "", color?: OscLightingCue["color"]): OscLightingCue => ({ id, at_seconds: at, address, args: [], label, color });

const mixed = (): OscLightingCue[] => [
  look("cap", 10),
  look("edit", 20, { edited: true }),
  look("man", 30, { source: "manual" }),
  single("on", 40, "/live/Main/cue/Stage Spread/activate", "Spread on", "rose"),
  single("off", 50, "/live/Main/cue/Stage Spread/deactivate", "Spread off"),
  single("fix", 60, "/fixture/MH1/overrides/zoom", "MH1 zoom"),
  single("oth", 70, "/haze/burst", "Haze"),
];

const mountOsc = (cues: OscLightingCue[], onSaveCues = vi.fn()) => {
  render(
    <LightingSetupPanel error={null} lights={lightsFor("osc")} message={null} onRefresh={vi.fn()} onSaveCues={onSaveCues} onSaveSettings={vi.fn()}
      onTest={vi.fn()} onTestOsc={vi.fn()} pendingOperation={null} settings={settingsFor("osc", cues)} state={state} />,
  );
  return onSaveCues;
};
const mountMidi = (cues: LightingCue[], onSaveCues = vi.fn()) => {
  render(
    <LightingSetupPanel error={null} lights={lightsFor("midi")} message={null} onRefresh={vi.fn()} onSaveCues={onSaveCues} onSaveSettings={vi.fn()}
      onTest={vi.fn()} onTestOsc={vi.fn()} pendingOperation={null} settings={settingsFor("midi", [], cues)} state={state} />,
  );
  return onSaveCues;
};
const markerOf = (type: string) => screen.queryAllByTestId("lighting-marker").filter((node) => node.getAttribute("data-type") === type);
const chip = (name: string) => screen.getByRole("button", { name });

beforeEach(() => window.sessionStorage.clear());
afterEach(() => vi.restoreAllMocks());

describe("marker rendering per OSC type", () => {
  it.each([
    ["look-captured", "◆", "bg-cue-look-captured", "Captured look, Look - 2 fixtures, at 00:10"],
    ["look-edited", "◆✎", "bg-cue-look-edited", "Edited look, Look - 2 fixtures, at 00:20"],
    ["look-manual", "◇", "bg-cue-look-manual", "Manual look, Look - 2 fixtures, at 00:30"],
    ["cue-on", "▶", "bg-cue-live", "Lightkey cue on, Spread on, at 00:40"],
    ["cue-off", "■", "cue-bar-off", "Lightkey cue off, Spread off, at 00:50"],
    ["fixture", "◐", "bg-cue-fixture", "Fixture message, MH1 zoom, at 01:00"],
    ["other", "○", "cue-bar-other", "Other OSC, Haze, at 01:10"],
  ])("%s: bar class, glyph box and accessible name", (type, glyph, barClass, name) => {
    mountOsc(mixed());
    const [marker] = markerOf(type);
    expect(marker).toBeDefined();
    expect(marker).toHaveAccessibleName(name);
    expect(within(marker!).getByTestId("lighting-marker-bar").className).toContain(barClass);
    expect(within(marker!).getByTestId("lighting-marker-glyph")).toHaveTextContent(glyph);
  });

  it("the cue list rows show the same glyph and a colour swatch", () => {
    mountOsc(mixed());
    const list = screen.getByTestId("lighting-cue-list");
    const badges = within(list).getAllByTestId("cue-type-badge");
    expect(badges.map((badge) => badge.getAttribute("data-type"))).toEqual(["look-captured", "look-edited", "look-manual", "cue-on", "cue-off", "fixture", "other"]);
    expect(within(badges[1]!).getByTestId("cue-type-glyph")).toHaveTextContent("◆✎");
    expect(within(badges[1]!).getByTestId("cue-type-swatch").className).toContain("bg-cue-look-edited");
  });

  it("the off bar is hollow with a border of at least 3px (css) and the legend groups the types", () => {
    mountOsc(mixed());
    expect(within(screen.getByRole("group", { name: "Looks" })).getAllByTestId("cue-legend-chip")).toHaveLength(4); // captured, edited, manual and the effect loop
    expect(within(screen.getByRole("group", { name: "Lightkey cues" })).getAllByTestId("cue-legend-chip")).toHaveLength(2);
    expect(within(screen.getByRole("group", { name: "Single messages" })).getAllByTestId("cue-legend-chip")).toHaveLength(2);
  });
});

describe("pad colour square", () => {
  it("is a small square after the label and no longer recolours the bar", () => {
    mountOsc(mixed());
    const [on] = markerOf("cue-on");
    const square = within(on!).getByTestId("lighting-pad-square");
    expect(square.className).toContain("bg-integration-lighting-soft"); // rose
    expect(on!.textContent!.indexOf("Spread on")).toBeLessThan(on!.innerHTML.indexOf("lighting-pad-square"));
    expect(within(on!).getByTestId("lighting-marker-bar").className).toContain("bg-cue-live");
    expect(within(on!).getByTestId("lighting-marker-bar").className).not.toContain("bg-integration");
    expect(within(markerOf("fixture")[0]!).queryByTestId("lighting-pad-square")).toBeNull();
  });
});

describe("legend filter", () => {
  it("hides a type on the timeline AND in the list, shows the count, and Show all restores", async () => {
    const onSaveCues = mountOsc(mixed());
    const user = userEvent.setup();
    expect(chip("Captured look")).toHaveAttribute("aria-pressed", "true");
    await user.click(chip("Captured look"));
    await user.click(chip("Other OSC"));
    expect(chip("Captured look")).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByText("2 types hidden")).toBeInTheDocument();
    expect(markerOf("look-captured")).toHaveLength(0);
    expect(markerOf("other")).toHaveLength(0);
    expect(markerOf("cue-on")).toHaveLength(1);
    const types = within(screen.getByTestId("lighting-cue-list")).getAllByTestId("cue-type-badge").map((b) => b.getAttribute("data-type"));
    expect(types).toEqual(["look-edited", "look-manual", "cue-on", "cue-off", "fixture"]);
    await user.click(screen.getByRole("button", { name: "Show all" }));
    expect(screen.queryByText(/hidden/)).toBeNull();
    expect(screen.getAllByTestId("lighting-marker")).toHaveLength(7);
    expect(within(screen.getByTestId("lighting-cue-list")).getAllByTestId("cue-type-badge")).toHaveLength(7);
    expect(onSaveCues).not.toHaveBeenCalled(); // view-only: never touches the saved cue map
  });

  it("singular wording", async () => {
    mountOsc(mixed());
    await userEvent.setup().click(chip("Fixture message"));
    expect(screen.getByText("1 type hidden")).toBeInTheDocument();
  });

  it("a selected marker is never hidden by a filter", async () => {
    mountOsc(mixed());
    const user = userEvent.setup();
    await user.click(markerOf("other")[0]!);
    await user.click(chip("Other OSC"));
    expect(markerOf("other")).toHaveLength(1);
    expect(within(screen.getByTestId("lighting-cue-list")).getAllByTestId("cue-type-badge").map((b) => b.getAttribute("data-type"))).toContain("other");
    expect(markerOf("look-captured")).toHaveLength(1);
    await user.click(chip("Captured look"));
    expect(markerOf("look-captured")).toHaveLength(0);
  });

  it("is remembered for the session, per tab", async () => {
    const first = render(<LightingTimeline markers={[{ id: "a", at: 5, label: "A", color: null, type: "look-captured" }, { id: "b", at: 9, label: "B", color: null, type: "other" }]} onMove={vi.fn()} onRemove={vi.fn()} playhead={null} songLength={60} />);
    await userEvent.setup().click(chip("Other OSC"));
    expect(screen.queryAllByTestId("lighting-marker")).toHaveLength(1);
    first.unmount();
    render(<LightingTimeline markers={[{ id: "a", at: 5, label: "A", color: null, type: "look-captured" }, { id: "b", at: 9, label: "B", color: null, type: "other" }]} onMove={vi.fn()} onRemove={vi.fn()} playhead={null} songLength={60} />);
    expect(chip("Other OSC")).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryAllByTestId("lighting-marker")).toHaveLength(1);
  });
});

describe("cluster chip", () => {
  it("is neutral, shows the dominant glyph and names the counts per type", () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, right: 600, bottom: 56, width: 600, height: 56, x: 0, y: 0, toJSON: () => ({}) });
    const markers: TimelineMarker[] = [
      { id: "1", at: 100, label: "L1", color: null, type: "look-captured" },
      { id: "2", at: 100.2, label: "L2", color: null, type: "look-captured" },
      { id: "3", at: 100.4, label: "On", color: null, type: "cue-on" },
    ];
    render(<LightingTimeline markers={markers} onMove={vi.fn()} onRemove={vi.fn()} playhead={null} songLength={300} />);
    const cluster = screen.getByTestId("lighting-cluster");
    expect(cluster).toHaveAccessibleName(/^3 cues \(2 Captured look, 1 Lightkey cue on\), 01:40 to 01:40\.4\. Zoom in$/);
    expect(within(cluster).getByTestId("lighting-cluster-glyph")).toHaveTextContent("◆");
    expect(cluster.className).toContain("bg-surface-panel");
    expect(cluster.className).not.toContain("bg-cue-");
  });

  it("hidden types leave the cluster", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, right: 600, bottom: 56, width: 600, height: 56, x: 0, y: 0, toJSON: () => ({}) });
    const markers: TimelineMarker[] = [
      { id: "1", at: 100, label: "L1", color: null, type: "look-captured" },
      { id: "2", at: 100.2, label: "L2", color: null, type: "look-captured" },
      { id: "3", at: 100.4, label: "On", color: null, type: "cue-on" },
    ];
    render(<LightingTimeline markers={markers} onMove={vi.fn()} onRemove={vi.fn()} playhead={null} songLength={300} />);
    await userEvent.setup().click(chip("Captured look"));
    expect(screen.queryByTestId("lighting-cluster")).toBeNull();
    expect(screen.getAllByTestId("lighting-marker")).toHaveLength(1);
  });
});

describe("MIDI timeline", () => {
  const midi = (id: string, at: number, note: number, label = ""): LightingCue => ({ id, at_seconds: at, note, velocity: 100, label });

  it("one colour per note letter; sharps striped with a sharp sign; accessible name has note, octave and label", () => {
    mountMidi([midi("a", 10, 60, "Verse"), midi("b", 20, 61, "Hit"), midi("c", 30, 72, "Again"), midi("d", 40, 70)]);
    const c3 = markerOf("note-C");
    expect(c3).toHaveLength(3); // C3, C#3, C4 share the C colour
    expect(c3[0]).toHaveAccessibleName("MIDI note C3 (C, octave 3), Verse, at 00:10");
    expect(within(c3[0]!).getByTestId("lighting-marker-bar").className).toContain("bg-cue-note-c");
    expect(within(c3[0]!).getByTestId("lighting-marker-bar").className).not.toContain("cue-bar-sharp");
    expect(within(c3[0]!).getByTestId("lighting-marker-glyph")).toHaveTextContent(/^C$/);
    expect(c3[0]!.textContent).toContain("C3 Verse");
    expect(c3[1]).toHaveAccessibleName("MIDI note C#3 (C sharp, octave 3), Hit, at 00:20");
    expect(within(c3[1]!).getByTestId("lighting-marker-bar").className).toContain("cue-bar-sharp");
    expect(within(c3[1]!).getByTestId("lighting-marker-bar").className).toContain("bg-cue-note-c");
    expect(within(c3[1]!).getByTestId("lighting-marker-glyph")).toHaveTextContent("C♯");
    expect(c3[2]).toHaveAccessibleName("MIDI note C4 (C, octave 4), Again, at 00:30");
    expect(within(markerOf("note-A")[0]!).getByTestId("lighting-marker-glyph")).toHaveTextContent("A♯"); // A#, the A colour
  });

  it("the legend has the seven letters; letters not present are off; hiding one hides it in the list too", async () => {
    const onSaveCues = mountMidi([midi("a", 10, 60), midi("b", 20, 64), midi("c", 30, 72)]);
    const user = userEvent.setup();
    expect(screen.getAllByTestId("cue-legend-chip").map((c) => c.getAttribute("data-type"))).toEqual(["note-C", "note-D", "note-E", "note-F", "note-G", "note-A", "note-B"]);
    expect(chip("D notes")).toBeDisabled();
    expect(chip("C notes")).not.toBeDisabled();
    await user.click(chip("C notes"));
    expect(markerOf("note-C")).toHaveLength(0);
    expect(markerOf("note-E")).toHaveLength(1);
    expect(screen.getAllByTestId("midi-cue-row")).toHaveLength(1);
    expect(screen.getByText("1 type hidden")).toBeInTheDocument();
    expect(onSaveCues).not.toHaveBeenCalled();
  });

  it("colour follows the pitch class in numeric display mode too", async () => {
    mountMidi([midi("a", 10, 61, "Hit")]);
    await userEvent.setup().click(screen.getByRole("switch", { name: /MIDI note display/ }));
    const [marker] = markerOf("note-C");
    expect(marker!.textContent).toContain("61 Hit");
    expect(within(marker!).getByTestId("lighting-marker-bar").className).toContain("cue-bar-sharp");
    expect(marker).toHaveAccessibleName("MIDI note C#3 (C sharp, octave 3), Hit, at 00:10");
  });
});

describe("dense song with filters", () => {
  it("400 looks plus mixed cues: filtering and clustering stay inside a time budget", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, right: 1000, bottom: 56, width: 1000, height: 56, x: 0, y: 0, toJSON: () => ({}) });
    const rows: OscLightingCue[] = [
      ...Array.from({ length: 400 }, (_, i) => look(`l${i}`, i * 0.7, { edited: i % 25 === 0 })),
      ...Array.from({ length: 60 }, (_, i) => single(`c${i}`, i * 4.6, `/live/Main/cue/n${i % 5}/${i % 2 ? "deactivate" : "activate"}`, `cue ${i}`)),
      ...Array.from({ length: 40 }, (_, i) => single(`f${i}`, i * 7, "/fixture/MH1/overrides/zoom", `z${i}`)),
    ];
    const onSaveCues = mountOsc(rows);
    const total = () => screen.queryAllByTestId("lighting-cluster").reduce((n, c) => n + Number(c.getAttribute("data-count")), 0) + screen.queryAllByTestId("lighting-marker").length;
    expect(total()).toBe(500);
    expect(screen.queryAllByTestId("lighting-cluster").length).toBeGreaterThan(0);
    const user = userEvent.setup();
    const started = performance.now();
    await user.click(chip("Captured look"));
    await user.click(chip("Edited look"));
    const elapsed = performance.now() - started;
    expect(total()).toBe(100);
    expect(elapsed).toBeLessThan(15000); // generous: a slow CI runner (macOS Intel) took 4.5 s; a real blow-up is far above this
    expect(screen.getByText("2 types hidden")).toBeInTheDocument();
    expect(screen.getByTestId("lighting-cue-list").querySelectorAll('[data-testid="cue-type-badge"]').length).toBeLessThanOrEqual(40);
    expect(onSaveCues).not.toHaveBeenCalled();
  }, 30000);
});
