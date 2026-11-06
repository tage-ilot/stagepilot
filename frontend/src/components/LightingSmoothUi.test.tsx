import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ApplicationState, LightingEffect, LightsStatusResponse, OscLightingCue, SettingsResponse, Song } from "../types";
import { LightingSetupPanel } from "./LightingSetupPanel";

const sources = import.meta.glob(["./Lighting*.{ts,tsx}", "./Lightkey*.{ts,tsx}", "./lighting*.{ts,tsx}", "./lightkey*.{ts,tsx}", "./liveLink.ts", "./SongsStrip.tsx", "!./**/*.test.*"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;

vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), getLightkeyStatus: vi.fn(() => Promise.reject(new Error("offline"))) }));

const song: Song = { id: "item-1", source_song_id: "song-1", title: "Holy Forever", duration_seconds: 300, order: 1, is_generic: false };
const base = { osc_host: "127.0.0.1", osc_port: 21600, enabled: true, output_name: null, channel: 1, pulse_ms: 100 };
const lights = { ...base, method: "osc", connection_status: "connected", detail: null, outputs: [], last_cue: null, last_osc_cue: null, last_cue_at: null } as LightsStatusResponse;
const state = { plan: { id: "p", title: "Sunday", songs: [song] }, current_song: song } as unknown as ApplicationState;
const dim = (n: string) => `/fixture/${n}/overrides/dimmer`;
const row = (id: string, at: number, address: string, value: number, fade = 0): OscLightingCue => ({
  id, at_seconds: at, address: "", args: [], label: "", source: "captured",
  messages: [{ address, args: [{ type: "float", value }] }], ...(fade ? { fade_seconds: fade } : {}),
});
const effect = (id = "e1", members = 16): LightingEffect => ({
  id, label: "", start_seconds: 60, end_seconds: 180, period_seconds: 4, source: "captured",
  members: Array.from({ length: members }, (_, i) => ({ fixture: `P${i + 1}`, offset_seconds: i * 0.25 })),
  cycle: [{ at_seconds: 0, property: "dimmer", value: [1], fade_seconds: 1.5 }, { at_seconds: 2, property: "dimmer", value: [0.2], fade_seconds: 1.5 }],
});
const settingsWith = (cues: OscLightingCue[], effects: LightingEffect[]) => ({
  planning_center_secret_saved: true, warning: null, restart_required: false,
  settings: { lights: { ...base, method: "osc", cue_maps: { "song-1": { song_key: "song-1", song_title: "Holy Forever", cues: [], osc_cues: cues, effects } } } },
}) as unknown as SettingsResponse;

const rows = () => [row("a", 10, dim("MH1"), 0.2), row("r", 30, dim("MH1"), 0.9, 3.2), row("i", 40, dim("MH2"), 0.5)];
const mount = (cues = rows(), effects = [effect()], onSaveCues = vi.fn()) => {
  render(<LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={onSaveCues} onSaveSettings={vi.fn()} onTest={vi.fn()} onTestOsc={vi.fn()} pendingOperation={null} settings={settingsWith(cues, effects)} state={state} />);
  return onSaveCues;
};

beforeEach(() => { window.localStorage.clear(); });
afterEach(() => vi.restoreAllMocks());

describe("ramps on the timeline", () => {
  it("draws a bar only for rows with a fade, and says so in the marker's name and tooltip", () => {
    mount();
    const markers = screen.getAllByTestId("lighting-marker");
    expect(markers).toHaveLength(3);
    const ramp = markers.find((m) => m.getAttribute("data-fade"))!;
    expect(ramp).toHaveAttribute("data-fade", "3.2");
    expect(ramp.getAttribute("aria-label")).toContain("ramp over 3.2 seconds");
    expect(ramp.getAttribute("title")).toBe("MH1: dimmer 0.2 → 0.9 over 3.2 s");
    expect(within(ramp).getByTestId("lighting-ramp-bar").className).toContain("bg-cue-look-captured");
    expect(screen.getAllByTestId("lighting-ramp-bar")).toHaveLength(1);
  });

  it("selecting a ramp shows its fixtures, from → to and duration below the timeline", async () => {
    mount();
    const ramp = screen.getAllByTestId("lighting-marker").find((m) => m.getAttribute("data-fade"))!;
    fireEvent.click(ramp); // a real pointer press also starts a drag, which jsdom (no layout) reads as a move to 0:00
    expect(screen.getByTestId("lighting-ramp-detail")).toHaveTextContent("Ramp +3.2 s. MH1: dimmer 0.2 → 0.9 over 3.2 s");
  });
});

describe("loops on the timeline", () => {
  it("draws one banded item per loop with the glyph and the label, and lists Effect loop in the legend", () => {
    mount();
    const band = screen.getByTestId("lighting-effect");
    expect(band).toHaveAttribute("data-lane", "0");
    expect(band.className).toContain("effect-band");
    expect(within(band).getByTestId("lighting-effect-label")).toHaveTextContent("4.0 s loop x 16 fixtures");
    expect(within(band).getByTestId("lighting-effect-glyph")).toHaveTextContent("↻");
    expect(band.getAttribute("aria-label")).toBe("Effect loop, 4.0 s loop x 16 fixtures, from 01:00 to 03:00");
    expect(screen.getByRole("button", { name: "Effect loop" })).toHaveAttribute("aria-pressed", "true");
  });

  it("stacks overlapping loops on lanes, at most three", () => {
    const loops = [effect("a"), { ...effect("b"), start_seconds: 100 }, { ...effect("c"), start_seconds: 110 }, { ...effect("d"), start_seconds: 120 }];
    mount(rows(), loops);
    const lanes = screen.getAllByTestId("lighting-effect").map((band) => Number(band.getAttribute("data-lane")));
    expect(Math.max(...lanes)).toBe(2);
    expect(new Set(lanes).size).toBe(3);
  });

  it("clicking a loop selects it and shows period, window, members with offsets, properties and a sparkline", async () => {
    mount();
    await userEvent.setup().click(screen.getByTestId("lighting-effect"));
    const popover = screen.getByTestId("lighting-effect-popover");
    expect(within(popover).getByTestId("effect-period")).toHaveTextContent("4.00 s");
    expect(within(popover).getByTestId("effect-window")).toHaveTextContent("01:00 to 03:00 (30 cycles)");
    expect(within(popover).getByTestId("effect-properties")).toHaveTextContent("dimmer");
    expect(within(popover).getByTestId("effect-members").children).toHaveLength(16);
    expect(within(popover).getByText(/P3/)).toHaveTextContent("P3 +0.50 s");
    expect(within(popover).getByTestId("effect-sparkline").querySelector("polyline")).not.toBeNull();
    expect(screen.getByTestId("lighting-effect")).toHaveAttribute("aria-pressed", "true");
  });

  it("the legend hides loops and shows them again; a loop can be picked with the keyboard", async () => {
    mount();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Effect loop" }));
    expect(screen.queryByTestId("lighting-effect")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Effect loop" }));
    const band = screen.getByTestId("lighting-effect");
    band.focus();
    await user.keyboard("{Enter}");
    expect(band).toHaveAttribute("aria-pressed", "true");
    await user.keyboard("{Escape}");
    expect(band).toHaveAttribute("aria-pressed", "false");
  });
});

describe("the cue rows list", () => {
  it("shows the duration column on ramps and one expandable row per loop that offers no edit fields", async () => {
    mount();
    const lookRows = screen.getAllByTestId("look-row");
    expect(within(lookRows[1]!).getByTestId("look-fade")).toHaveTextContent("Ramp +3.2 s");
    expect(within(lookRows[0]!).queryByTestId("look-fade")).toBeNull();
    const loop = screen.getByTestId("effect-row");
    expect(within(loop).queryAllByRole("textbox")).toHaveLength(0);
    expect(within(loop).queryByRole("checkbox")).toBeNull();
    await userEvent.setup().click(within(loop).getByRole("button", { name: /^Show loop/ }));
    expect(within(loop).getByTestId("effect-detail")).toHaveTextContent("not changed by editing the rows");
  });

  it("editing a ramp's duration saves it, marks the row edited and keeps the loop out of the save", async () => {
    const save = mount();
    const user = userEvent.setup();
    await user.click(within(screen.getAllByTestId("look-row")[1]!).getByRole("button", { name: /Expand look/ }));
    const box = screen.getByLabelText("Look duration (seconds, 0 = instant)");
    await user.clear(box);
    await user.type(box, "5{Enter}");
    const saved = save.mock.calls[0]![1] as OscLightingCue[];
    expect(saved.find((c) => c.id === "r")).toMatchObject({ fade_seconds: 5, edited: true });
    expect(save.mock.calls[0]![3]).toBeUndefined();
  });

  it("refuses a ramp edit the backend would refuse: negative, over 60 s, past the song end, not a number", async () => {
    const save = mount();
    const user = userEvent.setup();
    await user.click(within(screen.getAllByTestId("look-row")[1]!).getByRole("button", { name: /Expand look/ }));
    const box = screen.getByLabelText("Look duration (seconds, 0 = instant)");
    for (const [text, message] of [["-1", /number|negative/], ["61", /60 s/], ["abc", /number/]] as const) {
      await user.clear(box);
      await user.type(box, text);
      expect(screen.getAllByText(message).length).toBeGreaterThan(0);
      fireEvent.blur(box);
    }
    expect(save).not.toHaveBeenCalled();
  });
});

describe("removing a loop", () => {
  it("asks first, keeps the loop on Keep, and sends only that loop's id on Remove", async () => {
    const save = mount();
    const user = userEvent.setup();
    await user.click(within(screen.getByTestId("effect-row")).getByRole("button", { name: /^Remove loop/ }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Remove this loop?");
    expect(dialog).toHaveTextContent("The motion it plays is lost until you record again.");
    await user.click(within(dialog).getByRole("button", { name: "Keep loop" }));
    expect(save).not.toHaveBeenCalled();
    await user.click(within(screen.getByTestId("effect-row")).getByRole("button", { name: /^Remove loop/ }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Remove loop" }));
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0]![3]).toEqual({ removeEffectIds: ["e1"] });
    expect((save.mock.calls[0]![1] as OscLightingCue[]).map((c) => c.id)).toEqual(["a", "r", "i"]);
  });

  it("Delete on a picked loop asks the same question", async () => {
    mount();
    const user = userEvent.setup();
    screen.getByTestId("lighting-effect").focus();
    await user.keyboard("{Delete}");
    expect(screen.getByRole("dialog")).toHaveTextContent("Remove this loop?");
  });
});

describe("merge rules in the UI", () => {
  it("disables Merge for a ramp plus an instant row and says why", async () => {
    mount();
    const user = userEvent.setup();
    await user.click(screen.getByLabelText("Select look at 00:10"));
    await user.click(screen.getByLabelText("Select look at 00:30"));
    expect(screen.getByRole("button", { name: "Merge 2 rows" })).toBeDisabled();
    expect(screen.getByTestId("lighting-merge-blocked")).toHaveTextContent("A ramp cannot be merged with an instant row");
  });
});

describe("no thinning-era wording", () => {
  it("has no Detail, Fine, Coarse, Normal or Simplify control or text in the Lighting screen", () => {
    mount();
    const text = document.body.textContent ?? "";
    expect(text).not.toMatch(/\b(Detail|Fine|Coarse|Simplify|reduced detail)\b/);
    expect(screen.queryByRole("radio", { name: /^(Fine|Normal|Coarse)$/ })).toBeNull();
  });

  it("none of the Lighting components, their messages or DESIGN.md mention the Detail setting or Simplify (DESIGN.md is checked by grep in the PR)", () => {
    const entries = Object.entries(sources);
    expect(entries.length).toBeGreaterThan(10);
    for (const [name, source] of entries) {
      expect(source, name).not.toMatch(/["'`>]\s*(Fine|Coarse)\s*["'`<]|label="Detail"|detailLabels|Simplify this song|reduced detail|simplifyLightkeyLooks/);
    }
  });
});
