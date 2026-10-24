import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { LightkeyStatus } from "../types";
import { LightkeyStrip, lightkeyHeadline, lightkeyLines, normaliseLine } from "./LightkeyStrip";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getLightkeyStatus: vi.fn(),
  saveLightkeySettings: vi.fn(),
  setLightkeyRecording: vi.fn(),
  clearLightkeyTake: vi.fn(),
}));

const status = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://10.30.0.211:8770",
  link: "connected",
  detail: null,
  active_count: 11,
  recording: false,
  recording_song: null,
  replay: true,
  replay_blocked: null,
  replaying_song: null,
  message: null,
  takes: [],
  ...over,
});
const take = { song_key: "s1", song_title: "Holy Forever", recorded_at: "2026-10-20T00:00:00Z", duration_seconds: 300, edge_count: 14, cue_count: 5 };
const respond = (lightkey: LightkeyStatus, accepted = true, message = "ok") => ({ accepted, message, lightkey });

beforeEach(() => { vi.mocked(api.getLightkeyStatus).mockResolvedValue(status()); });
afterEach(() => { vi.clearAllMocks(); });

describe("LightkeyStrip", () => {
  it("shows the link word and how many cues are on", async () => {
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText("Connected")).toBeInTheDocument();
    expect(screen.getByText(/11 cues on/)).toBeInTheDocument();
  });

  it("says why it is not connected, in words", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "unreachable", detail: "Cannot reach the Lightkey API.", active_count: null }));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText("Not connected")).toBeInTheDocument();
    expect(screen.getByText("Cannot reach the Lightkey API.")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Record from Lightkey" })).toBeDisabled();
  });

  it("Connect saves the typed address", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "unreachable", detail: "Cannot reach the Lightkey API." }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ link: "unreachable" })));
    render(<LightkeyStrip lightingEnabled />);
    const input = await screen.findByLabelText("Lightkey API address");
    const user = userEvent.setup();
    await user.clear(input);
    await user.type(input, "10.30.0.211:8770");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ api_url: "10.30.0.211:8770" }));
  });

  it("Record switch starts recording and shows the answer", async () => {
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({ recording: true, message: "Recording starts with the next song." })));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("switch", { name: "Record from Lightkey" }));
    await waitFor(() => expect(api.setLightkeyRecording).toHaveBeenCalledWith(true));
    expect(await screen.findByRole("switch", { name: "Record from Lightkey" })).toBeChecked();
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("Recording starts with the next song.");
  });

  it("a refused record request shows the reason and the switch stays off", async () => {
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status(), false, "Recording needs Lighting set to OSC."));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("switch", { name: "Record from Lightkey" }));
    expect(await screen.findByText("Recording needs Lighting set to OSC.")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Record from Lightkey" })).not.toBeChecked();
  });

  it("Replay switch is on by default and can be turned off", async () => {
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ replay: false, replay_blocked: "Replay is switched off." })));
    render(<LightkeyStrip lightingEnabled />);
    const toggle = await screen.findByRole("switch", { name: "Replay captured lights" });
    expect(toggle).toBeChecked();
    await userEvent.setup().click(toggle);
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ replay: false }));
    expect(await screen.findByRole("switch", { name: "Replay captured lights" })).not.toBeChecked();
  });

  it("lists captured songs and clears one", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ takes: [take] }));
    vi.mocked(api.clearLightkeyTake).mockResolvedValue(respond(status()));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText(/Holy Forever/)).toBeInTheDocument();
    expect(screen.getByText(/14 changes/)).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "Clear capture for Holy Forever" }));
    await waitFor(() => expect(api.clearLightkeyTake).toHaveBeenCalledWith("s1"));
    await waitFor(() => expect(screen.queryByText(/Holy Forever/)).not.toBeInTheDocument());
  });

  it("shows the fixture stream and what each song captured besides cues", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({
      fixtures: { link: "live", detail: null, fixtures: 66, lost_packets: 0, recording: false },
      takes: [{ ...take, fixture_count: 12, fixture_event_count: 131 }],
    }));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText("Colour and levels: 66 fixtures")).toBeInTheDocument();
    expect(screen.getByText(/131 colour\/level changes/)).toBeInTheDocument();
  });

  it("says connecting while the first fixture data is awaited", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ fixtures: { link: "waiting", detail: null, fixtures: 1, lost_packets: 0, recording: false } }));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText("Colour and levels: connecting")).toBeInTheDocument();
  });

  it("says in words when the fixture stream stops, and says nothing for an older Lightkey API", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({
      fixtures: { link: "lost", detail: "No fixture data from the Lightkey API.", fixtures: 66, lost_packets: 2, recording: false },
    }));
    const { unmount } = render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText("No fixture data from the Lightkey API.")).toBeInTheDocument();
    unmount();
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status());
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByText("Connected");
    expect(screen.queryByTestId("lightkey-fixtures")).not.toBeInTheDocument();
  });

  it("shows a load failure instead of a blank strip", async () => {
    vi.mocked(api.getLightkeyStatus).mockRejectedValue(new Error("boom"));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText(/unavailable|boom|Loading/i)).toBeInTheDocument();
  });
});

describe("lightkeyHeadline", () => {
  it("puts recording first, then replaying, then why replay is paused", () => {
    expect(lightkeyHeadline(status({ recording: true, recording_song: "Holy Forever" }))).toBe("Recording “Holy Forever”.");
    expect(lightkeyHeadline(status({ recording: true }))).toBe("Recording starts with the next song.");
    expect(lightkeyHeadline(status({ replaying_song: "Holy Forever", takes: [take] }))).toBe("Replaying “Holy Forever”.");
    expect(lightkeyHeadline(status({ replay_blocked: "Replay is switched off.", takes: [take] }))).toBe("Replay is switched off.");
  });
  it("tells a first-time operator what to do, then counts captures", () => {
    expect(lightkeyHeadline(status())).toMatch(/Turn on Record/);
    expect(lightkeyHeadline(status({ takes: [take] }))).toBe("1 song captured.");
    expect(lightkeyHeadline(status({ takes: [take, { ...take, song_key: "s2" }] }))).toBe("2 songs captured.");
  });
});

describe("lightkeyLines (B3: no line twice, replay-paused line)", () => {
  const armed = { recording: true, replay: true, replay_blocked: "Paused while recording.", takes: [take] };

  it("Record armed + Replay on: the backend message is not repeated, the paused line follows", () => {
    expect(lightkeyLines(status({ ...armed, message: "Recording starts with the next song." }), null)).toEqual([
      "Recording starts with the next song.",
      "Replay paused while recording.",
    ]);
    expect(lightkeyLines(status({ ...armed, message: null }), null)).toEqual([
      "Recording starts with the next song.",
      "Replay paused while recording.",
    ]);
  });

  it("recording a song: straight and curly quotes count as the same line", () => {
    expect(lightkeyLines(status({ ...armed, recording_song: "Holy Forever", message: 'Recording "Holy Forever".' }), null)).toEqual([
      'Recording "Holy Forever".',
      "Replay paused while recording.",
    ]);
    expect(lightkeyLines(status({ ...armed, recording_song: "Holy Forever" }), null)).toEqual([
      "Recording “Holy Forever”.",
      "Replay paused while recording.",
    ]);
  });

  it("Record armed + Replay off: no paused line", () => {
    expect(lightkeyLines(status({ recording: true, replay: false, replay_blocked: "Paused while recording.", message: "Recording starts with the next song." }), null)).toEqual([
      "Recording starts with the next song.",
    ]);
  });

  it("Record off + Replay on: no paused line, and it is gone once recording stops", () => {
    expect(lightkeyLines(status({ takes: [take] }), null)).toEqual(["1 song captured."]);
    expect(lightkeyLines(status({ takes: [take], replaying_song: "Holy Forever" }), null)).toEqual(["Replaying “Holy Forever”."]);
  });

  it("Record off + Replay off: says replay is switched off", () => {
    expect(lightkeyLines(status({ replay: false, replay_blocked: "Replay is switched off.", takes: [take] }), null)).toEqual(["Replay is switched off."]);
  });

  it("a message that differs from the headline is kept next to it", () => {
    expect(lightkeyLines(status({ takes: [take], message: "Saved \"Holy Forever\": 14 changes. Song finished." }), null)).toEqual([
      'Saved "Holy Forever": 14 changes. Song finished.',
      "1 song captured.",
    ]);
  });

  it("an error replaces the message line", () => {
    expect(lightkeyLines(status({ message: "Capture cleared." }), "Cannot reach the app.")[0]).toBe("Cannot reach the app.");
  });

  it("normaliseLine ignores quote style and spacing", () => {
    expect(normaliseLine("Recording  “A”.")).toBe(normaliseLine('Recording "A".'));
  });
});

describe("LightkeyStrip lines on screen (B3)", () => {
  it("armed with both switches on shows exactly two different lines", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true, replay: true, replay_blocked: "Paused while recording.", message: "Recording starts with the next song." }));
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.getAllByText("Recording starts with the next song.")).toHaveLength(1);
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("Recording starts with the next song.");
    expect(screen.getAllByTestId("lightkey-detail").map((n) => n.textContent)).toEqual(["Replay paused while recording."]);
  });

  it("recording a song shows the title once and the paused line", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true, recording_song: "Holy Forever", replay_blocked: "Paused while recording.", message: 'Recording "Holy Forever".' }));
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.getAllByText(/Recording .Holy Forever/)).toHaveLength(1);
    expect(screen.getByText("Replay paused while recording.")).toBeInTheDocument();
  });

  it("when recording stops the paused line goes and replay is described again", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true, replay_blocked: "Paused while recording." }));
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({ takes: [take], message: null })));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText("Replay paused while recording.")).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("switch", { name: "Record from Lightkey" }));
    await waitFor(() => expect(screen.queryByText("Replay paused while recording.")).not.toBeInTheDocument());
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("1 song captured.");
  });

  it("an expired message (backend sends null) falls back to the headline", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "unreachable", active_count: null, message: null }));
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByText("Not connected");
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("0 songs captured.");
  });
});
