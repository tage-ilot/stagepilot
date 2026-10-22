import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { LightkeyStatus } from "../types";
import { LightkeyStrip, lightkeyHeadline } from "./LightkeyStrip";

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
