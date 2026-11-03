import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";

import * as api from "../api";
import type { ApplicationState, LightkeyStatus, LightsStatusResponse, OscLightingCue, SettingsResponse, Song } from "../types";
import { LightingSetupPanel, LIGHTING_CUE_BOARD_OPEN_KEY } from "./LightingSetupPanel";

const songs: Song[] = [
  { id: "i1", source_song_id: "song-1", title: "Holy Forever", duration_seconds: 300, order: 1, is_generic: false },
  { id: "i2", source_song_id: "song-2", title: "Goodness", duration_seconds: 240, order: 2, is_generic: false },
  { id: "i3", source_song_id: "song-3", title: "Amazing Grace", duration_seconds: 200, order: 3, is_generic: false },
];
const base = { osc_host: "192.0.2.10", osc_port: 21600, enabled: true, output_name: null, channel: 1, pulse_ms: 100 };
const lights = { ...base, method: "osc", connection_status: "connected", detail: null, outputs: [], last_cue: null, last_osc_cue: null, last_cue_at: null } as LightsStatusResponse;
const look = (id: string, at: number): OscLightingCue => ({ id, at_seconds: at, address: "", args: [], label: "", source: "captured", messages: [{ address: "/fixture/MH1/overrides/dimmer", args: [{ type: "float", value: 1 }] }] });
const status = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://192.0.2.10:8770", link: "connected", detail: null, active_count: 2, recording: false, recording_song: null, replay: true, replay_blocked: null, replaying_song: null, message: null, ...over,
});
const settings = () => ({
  planning_center_secret_saved: true, warning: null, restart_required: false,
  settings: { lights: { ...base, method: "osc", cue_maps: {
    "song-1": { song_key: "song-1", song_title: "Holy Forever", cues: [], osc_cues: [look("a", 10), look("b", 20)], loudness: { values: [1, 2], step_seconds: 10 } },
    "song-3": { song_key: "song-3", song_title: "Amazing Grace", cues: [], osc_cues: [look("c", 30)] },
  } } },
}) as unknown as SettingsResponse;
const stateWith = (playing?: Song) => ({ plan: { id: "p", title: "Sunday", songs }, current_song: songs[0], playing_song: playing } as unknown as ApplicationState);

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getLightkeyStatus: vi.fn(),
  saveLightkeySettings: vi.fn(),
  simplifyLightkeyLooks: vi.fn(),
  setLightkeyAudioMeter: vi.fn().mockResolvedValue({ available: false, reason: "No input.", devices: [], selected: null, note: null, level: null, metering: false }),
}));

const mount = (props: { state?: ApplicationState; onSaveCues?: Mock; onTestOsc?: Mock } = {}) => {
  const onSaveCues = props.onSaveCues ?? vi.fn();
  const onSaveSettings = vi.fn();
  render(
    <LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={onSaveCues} onSaveSettings={onSaveSettings}
      onSavePads={vi.fn()} onPressPad={vi.fn()} onUndoCapture={vi.fn()} onTest={vi.fn()} onTestOsc={props.onTestOsc ?? vi.fn()} pendingOperation={null} settings={settings()} state={props.state ?? stateWith()} />,
  );
  return { onSaveCues, onSaveSettings };
};

beforeEach(() => {
  window.localStorage.clear();
  vi.mocked(api.getLightkeyStatus).mockResolvedValue(status());
});
afterEach(() => { vi.clearAllMocks(); vi.restoreAllMocks(); window.localStorage.clear(); });

describe("one shared host for OSC and the live-link API", () => {
  it("shows ONE host box, the OSC UDP port and the API port in a single titled line", async () => {
    mount();
    await screen.findByLabelText("API port");
    expect(screen.getAllByLabelText("Host or IP address")).toHaveLength(1);
    expect(screen.getByLabelText("Host or IP address")).toHaveValue("192.0.2.10");
    expect(screen.getByLabelText("OSC UDP port")).toHaveValue("21600");
    expect(screen.getByLabelText("API port")).toHaveValue("8770");
    expect(screen.queryByLabelText("Lightkey API address")).toBeNull();
    expect(screen.getAllByRole("heading", { name: "sACN DMX Live Link" })).toHaveLength(1);
  });

  it("editing the host edits the panel's saved OSC host draft (Save bar), not a second address", async () => {
    const { onSaveSettings } = mount();
    await screen.findByLabelText("API port");
    const host = screen.getByLabelText("Host or IP address");
    const user = userEvent.setup();
    await user.clear(host);
    await user.type(host, "192.0.2.77");
    expect(host).toHaveValue("192.0.2.77");
    await user.click(screen.getByRole("button", { name: /^Save/ }));
    expect(onSaveSettings).toHaveBeenCalledWith(expect.objectContaining({ method: "osc", osc_host: "192.0.2.77", osc_port: 21600 }));
    expect(api.saveLightkeySettings).not.toHaveBeenCalled();
  });

  it("a saved API host different from the OSC host shows the notice and 'Use the OSC host'", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ api_url: "http://192.0.2.99:8770" }));
    mount();
    const notice = await screen.findByTestId("lightkey-host-notice");
    expect(notice).toHaveTextContent("192.0.2.99");
    expect(notice).toHaveTextContent("192.0.2.10");
    expect(within(notice).getByRole("button", { name: "Use the OSC host" })).toBeInTheDocument();
    expect(api.saveLightkeySettings).not.toHaveBeenCalled(); // never rewritten silently
  });
});

describe("Send a test cue disclosure", () => {
  it("is collapsed under the connection, outside the timeline section, and keeps the OSC address checks and sending", async () => {
    const onTestOsc = vi.fn();
    mount({ onTestOsc });
    await screen.findByLabelText("API port");
    const toggle = screen.getByRole("button", { name: "Send a test cue" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    const timeline = screen.getByRole("button", { name: /^Song lighting timeline/ }).parentElement!;
    expect(timeline.contains(toggle)).toBe(false);
    const strip = screen.getByTestId("lightkey-strip");
    expect(strip.contains(toggle)).toBe(true);
    const user = userEvent.setup();
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const group = screen.getByRole("group", { name: "OSC test cue" });
    // The field is the cue's OSC path, not the host.
    expect(within(group).getByText(/The cue's OSC path/)).toBeInTheDocument();
    const send = within(group).getByRole("button", { name: "Send test cue" });
    expect(send).toBeDisabled();
    await user.type(within(group).getByLabelText("OSC address"), "no-slash");
    expect(send).toBeDisabled();
    await user.clear(within(group).getByLabelText("OSC address"));
    await user.type(within(group).getByLabelText("OSC address"), "/live/*/cue/Stars/activate");
    expect(send).toBeEnabled();
    await user.click(send);
    expect(onTestOsc).toHaveBeenCalledWith("/live/*/cue/Stars/activate", []);
  });

  it("the timeline section no longer has an OSC address input of its own for sending", async () => {
    mount();
    await screen.findByLabelText("API port");
    const timeline = screen.getByRole("button", { name: /^Song lighting timeline/ }).parentElement!;
    expect(within(timeline).queryByRole("heading", { name: "Test cue" })).toBeNull();
    expect(within(timeline).queryByRole("group", { name: "OSC test cue", hidden: true })).toBeNull();
  });
});

describe("songs strip in the panel", () => {
  it("one chip per plan song in plan order: captured coloured, others greyed, live dot on the playing song", async () => {
    mount({ state: stateWith(songs[1]) });
    const list = screen.getByRole("list", { name: "Songs in this service" });
    const items = within(list).getAllByRole("listitem");
    expect(items.map((i) => i.getAttribute("data-song"))).toEqual(["song-1", "song-2", "song-3"]);
    expect(items.map((i) => i.getAttribute("data-state"))).toEqual(["captured", "empty", "captured"]);
    expect(items[0]).toHaveTextContent("Incomplete capture");
    expect(within(items[0]!).getByTestId("song-chip-loudness")).toBeInTheDocument();
    expect(items[1]).toHaveTextContent("Not captured");
    expect(within(items[1]!).getByTestId("song-chip-live")).toBeInTheDocument();
    expect(screen.getByText(/Songs in this service - \d of 3 fully captured/)).toBeInTheDocument();
  });

  it("clicking a chip selects that song exactly like the Song menu, and Follow live returns", async () => {
    mount({ state: stateWith(songs[0]) });
    const user = userEvent.setup();
    const menu = screen.getByLabelText("Song") as HTMLSelectElement;
    expect(menu.value).toBe("song-1");
    await user.click(screen.getByRole("button", { name: /^Amazing Grace/ }));
    expect(menu.value).toBe("song-3");
    expect(screen.getByRole("button", { name: /^Amazing Grace/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("lighting-follow-status")).toHaveTextContent("Showing Amazing Grace");
    await user.click(screen.getByRole("button", { name: "Follow live" }));
    expect(menu.value).toBe("song-1");
  });

  it("the x opens the permanent-erase dialog for that song only; Erase permanently saves it empty", async () => {
    const { onSaveCues } = mount();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Clear cues for Amazing Grace..." }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Amazing Grace");
    expect(dialog).toHaveTextContent("1 look");
    expect(onSaveCues).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Erase permanently" }));
    expect(onSaveCues).toHaveBeenCalledTimes(1);
    expect(onSaveCues).toHaveBeenCalledWith(songs[2], [], "osc");
  });

  it("Clear all songs... uses the same dialog", async () => {
    const { onSaveCues } = mount();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Clear all songs..." }));
    expect(screen.getByRole("dialog")).toHaveTextContent("permanently erased");
    await user.click(screen.getByRole("button", { name: "Keep cues" }));
    expect(onSaveCues).not.toHaveBeenCalled();
  });
});

describe("song lighting timeline toolbar", () => {
  it("is one wrapping toolbar with Song, Add cue, Clear song, Merge, Simplify, zoom and height; no old bottom Clear buttons", async () => {
    mount();
    const bar = screen.getByRole("toolbar", { name: "Timeline tools" });
    expect(bar.className).toContain("flex-wrap");
    expect(within(bar).getByLabelText("Song")).toBeInTheDocument();
    for (const name of ["Add cue", "Clear song...", "Merge rows", "Simplify this song", "Zoom out", "Fit", "Zoom in"]) {
      expect(within(bar).getByRole("button", { name })).toBeInTheDocument();
    }
    expect(within(bar).getByRole("radiogroup", { name: "Timeline height" })).toBeInTheDocument();
    expect(within(bar).getByRole("button", { name: "Add cue" }).className).toContain("!h-[34px]");
    expect(screen.queryByRole("button", { name: "Clear all" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();
  });

  it("Merge rows needs two selected rows and Simplify needs captured looks", async () => {
    mount();
    expect(screen.getByRole("button", { name: "Merge rows" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Simplify this song" })).toBeEnabled();
    await userEvent.setup().click(screen.getByRole("button", { name: /^Amazing Grace/ }));
    await userEvent.setup().click(screen.getByRole("button", { name: /^Goodness/ }));
    expect(screen.getByRole("button", { name: "Simplify this song" })).toBeDisabled();
  });
});

describe("Cue board", () => {
  it("is collapsible, open by default, and remembers how it was left on this device", async () => {
    mount();
    const toggle = screen.getByRole("button", { name: "Cue board" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await userEvent.setup().click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(window.localStorage.getItem(LIGHTING_CUE_BOARD_OPEN_KEY)).toBe("false");
  });

  it("opens folded when it was left folded", () => {
    window.localStorage.setItem(LIGHTING_CUE_BOARD_OPEN_KEY, "false");
    mount();
    expect(screen.getByRole("button", { name: "Cue board" })).toHaveAttribute("aria-expanded", "false");
  });
});

