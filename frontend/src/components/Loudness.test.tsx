import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { LightkeyAudio, LightkeyStatus, SongLoudness } from "../types";
import { LightingTimeline } from "./LightingTimeline";
import { LightkeyStrip } from "./LightkeyStrip";
import { meterPercent } from "./loudness";
import { LoudnessPicker } from "./LoudnessPicker";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getLightkeyStatus: vi.fn(),
  saveLightkeySettings: vi.fn(),
  setLightkeyRecording: vi.fn(),
  setLightkeyAudioMeter: vi.fn(),
}));

const audio = (over: Partial<LightkeyAudio> = {}): LightkeyAudio => ({
  available: true,
  reason: null,
  devices: ["Built-in Microphone", "Scarlett 2i2"],
  selected: null,
  note: null,
  level: { rms_db: -23.4, peak_db: -12, value: 160 },
  metering: true,
  ...over,
});

const status = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://192.0.2.10:8770",
  link: "connected",
  detail: null,
  active_count: 3,
  recording: true,
  recording_song: null,
  replay: false,
  replay_blocked: null,
  replaying_song: null,
  message: null,
  ...over,
});

beforeEach(() => {
  vi.mocked(api.getLightkeyStatus).mockResolvedValue(status());
  vi.mocked(api.setLightkeyAudioMeter).mockResolvedValue(audio());
});
afterEach(() => { vi.clearAllMocks(); });

describe("input picker and live meter", () => {
  it("lists the inputs, shows the level, and only appears in Record mode", async () => {
    render(<LightkeyStrip lightingEnabled loudness={{ present: false }} />);
    const select = await screen.findByRole("combobox", { name: "Audio input" });
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual(["System default input", "Built-in Microphone", "Scarlett 2i2"]);
    const meter = screen.getByRole("meter", { name: "Input level" });
    expect(meter).toHaveAttribute("aria-valuenow", String(meterPercent(160)));
    expect(meter).toHaveAttribute("aria-valuetext", "-23 dBFS");
    expect(screen.getByText(/Only levels are saved, never audio/)).toBeInTheDocument();
    expect(api.setLightkeyAudioMeter).toHaveBeenCalledWith(true);
  });

  it("is absent in Replay mode and never opens the meter", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: false, replay: true }));
    render(<LightkeyStrip lightingEnabled loudness={{ present: true, onClear: vi.fn() }} />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.queryByRole("combobox", { name: "Audio input" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Clear loudness" })).toBeNull();
    expect(api.setLightkeyAudioMeter).not.toHaveBeenCalledWith(true);
  });

  it("saves the chosen input, and the empty name means the system default", async () => {
    vi.mocked(api.saveLightkeySettings).mockResolvedValue({ accepted: true, message: "Saved.", lightkey: status() });
    render(<LoudnessPicker hasLoudness={false} />);
    const select = await screen.findByRole("combobox", { name: "Audio input" });
    await userEvent.setup().selectOptions(select, "Scarlett 2i2");
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ audio_device: "Scarlett 2i2" }));
    await userEvent.setup().selectOptions(select, "System default input");
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenLastCalledWith({ audio_device: "" }));
  });

  it("keeps a saved input that has gone missing visible and says the default is used", async () => {
    vi.mocked(api.setLightkeyAudioMeter).mockResolvedValue(audio({ selected: "Old Interface", note: "Old Interface is not available. Using the default input." }));
    render(<LoudnessPicker hasLoudness={false} />);
    const select = await screen.findByRole("combobox", { name: "Audio input" });
    expect(select).toHaveValue("Old Interface");
    expect(screen.getByRole("option", { name: "Old Interface (not found)" })).toBeInTheDocument();
    expect(screen.getByTestId("loudness-note")).toHaveTextContent("Using the default input");
  });

  it("hides the picker and shows one short line where audio is unavailable", async () => {
    vi.mocked(api.setLightkeyAudioMeter).mockResolvedValue(audio({ available: false, reason: "Loudness needs the desktop app: this build cannot listen to an audio input.", devices: [], level: null }));
    render(<LoudnessPicker hasLoudness={false} />);
    expect(await screen.findByTestId("loudness-unavailable")).toHaveTextContent("Loudness needs the desktop app");
    expect(screen.queryByRole("combobox")).toBeNull();
    expect(screen.queryByRole("meter")).toBeNull();
  });

  it("treats a failing request as unavailable instead of breaking the strip", async () => {
    vi.mocked(api.setLightkeyAudioMeter).mockRejectedValue(new Error("down"));
    render(<LoudnessPicker hasLoudness={false} />);
    expect(await screen.findByTestId("loudness-unavailable")).toHaveTextContent("Loudness is not available right now.");
  });

  it("offers Clear loudness only when the song has some, even where the picker is hidden", async () => {
    const onClear = vi.fn();
    vi.mocked(api.setLightkeyAudioMeter).mockResolvedValue(audio({ available: false, reason: "No audio input was found.", devices: [] }));
    const { unmount } = render(<LoudnessPicker hasLoudness onClear={onClear} />);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Clear loudness" }));
    expect(onClear).toHaveBeenCalledTimes(1);
    unmount();
    render(<LoudnessPicker hasLoudness={false} onClear={onClear} />);
    await screen.findByTestId("loudness-unavailable");
    expect(screen.queryByRole("button", { name: "Clear loudness" })).toBeNull();
  });

  it("switches the meter off when it goes away", async () => {
    const { unmount } = render(<LoudnessPicker hasLoudness={false} />);
    await screen.findByRole("meter");
    unmount();
    expect(api.setLightkeyAudioMeter).toHaveBeenLastCalledWith(false);
  });

  it("no signal reads as such", async () => {
    vi.mocked(api.setLightkeyAudioMeter).mockResolvedValue(audio({ level: { rms_db: null, peak_db: null, value: 1 } }));
    render(<LoudnessPicker hasLoudness={false} />);
    expect((await screen.findByRole("meter")).getAttribute("aria-valuetext")).toBe("No signal");
  });
});

describe("loudness behind the timeline", () => {
  const loudness: SongLoudness = { step_seconds: 0.25, values: Array.from({ length: 600 }, (_, i) => (i === 520 ? 200 : 40)), takes: 2 };
  const base = { markers: [{ id: "a", at: 12, label: "Look - 3 fixtures", color: null, captured: true }], onMove: vi.fn(), onRemove: vi.fn(), playhead: null, songLength: 150 };

  it("draws a faint filled envelope inside the track with a text alternative", () => {
    render(<LightingTimeline {...base} loudness={loudness} />);
    const picture = screen.getByRole("img", { name: "Loudness: peak at 02:10" });
    expect(picture).toBe(screen.getByTestId("lighting-loudness"));
    expect(screen.getByTestId("lighting-timeline-track")).toContainElement(picture);
    expect(picture.querySelector("path")?.getAttribute("fill")).toBe("currentColor");
    expect(picture.getAttribute("class")).toContain("pointer-events-none");
    expect(picture.getAttribute("class")).toContain("opacity-25");
    // The markers are still there and still reachable.
    expect(screen.getByRole("button", { name: /Look - 3 fixtures, at 00:12/ })).toBeInTheDocument();
  });

  it("scales to the song's own maximum", () => {
    const quiet: SongLoudness = { ...loudness, values: loudness.values.map((v) => Math.round(v / 10)) };
    const a = render(<LightingTimeline {...base} loudness={loudness} />);
    const loudPath = a.container.querySelector("path")?.getAttribute("d");
    a.unmount();
    const b = render(<LightingTimeline {...base} loudness={quiet} />);
    const quietPath = b.container.querySelector("path")?.getAttribute("d");
    expect(quietPath).toBeTruthy();
    expect(quietPath!.includes(",0 ")).toBe(true);
    expect(loudPath!.includes(",0 ")).toBe(true);
  });

  it("is on the same track as the markers, so zoom and scroll move them together", async () => {
    render(<LightingTimeline {...base} loudness={loudness} />);
    const track = screen.getByTestId("lighting-timeline-track");
    const before = track.parentElement!.getAttribute("style");
    await userEvent.setup().click(screen.getByRole("button", { name: "Zoom in" }));
    expect(track.parentElement!.getAttribute("style")).not.toBe(before);
    expect(track).toContainElement(screen.getByTestId("lighting-loudness"));
  });

  it("draws nothing without data", () => {
    const { rerender } = render(<LightingTimeline {...base} />);
    expect(screen.queryByTestId("lighting-loudness")).toBeNull();
    rerender(<LightingTimeline {...base} loudness={{ ...loudness, values: [0, 0, 0] }} />);
    expect(screen.queryByTestId("lighting-loudness")).toBeNull();
    rerender(<LightingTimeline {...base} loudness={null} />);
    expect(screen.queryByTestId("lighting-loudness")).toBeNull();
  });
});
