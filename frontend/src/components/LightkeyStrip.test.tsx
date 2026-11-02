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
  setLightkeyAudioMeter: vi.fn().mockResolvedValue({ available: false, reason: "No input.", devices: [], selected: null, note: null, level: null, metering: false }),
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
  ...over,
});
const respond = (lightkey: LightkeyStatus, accepted = true, message = "ok") => ({ accepted, message, lightkey });

beforeEach(() => { vi.mocked(api.getLightkeyStatus).mockResolvedValue(status()); });
afterEach(() => { vi.clearAllMocks(); });

describe("LightkeyStrip", () => {
  it("shows the link word", async () => {
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText("Connected")).toBeInTheDocument();
    expect(screen.queryByText(/cues on/)).not.toBeInTheDocument();
  });

  it("says why it is not connected, in words", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "unreachable", detail: "Cannot reach the Lightkey API.", active_count: null }));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText("Not connected")).toBeInTheDocument();
    expect(screen.getByText("Cannot reach the Lightkey API.")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Record" })).toBeDisabled();
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

  it("Record mode starts recording and shows the answer", async () => {
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({ recording: true, message: "Recording starts with the next song." })));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Record" }));
    await waitFor(() => expect(api.setLightkeyRecording).toHaveBeenCalledWith(true));
    await waitFor(() => expect(screen.getByRole("radio", { name: "Record" })).toBeChecked());
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("Recording starts with the next song.");
  });

  it("a refused record request shows the reason and the toggle stays on Replay", async () => {
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status(), false, "Recording needs Lighting set to OSC."));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Record" }));
    expect(await screen.findByText("Recording needs Lighting set to OSC.")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Replay" })).toBeChecked();
  });

  it("one Replay | Record toggle replaces the two switches, Replay by default", async () => {
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByRole("radiogroup", { name: "Lightkey mode" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Replay" })).toBeChecked();
    expect(screen.queryByRole("switch")).toBeNull();
  });

  it("Record back to Replay: recording off first, then replay on; errors revert and show the message", async () => {
    const calls: string[] = [];
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true, replay: false }));
    vi.mocked(api.setLightkeyRecording).mockImplementation(async () => { calls.push("recording-off"); return respond(status({ replay: false })); });
    vi.mocked(api.saveLightkeySettings).mockImplementation(async () => { calls.push("replay-on"); return respond(status()); });
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Replay" }));
    await waitFor(() => expect(calls).toEqual(["recording-off", "replay-on"]));
    expect(api.setLightkeyRecording).toHaveBeenCalledWith(false);
    expect(api.saveLightkeySettings).toHaveBeenCalledWith({ replay: true });
    await waitFor(() => expect(screen.getByRole("radio", { name: "Replay" })).toBeChecked());
  });

  it("Record back to Replay: a refused stop keeps Record and shows why", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true }));
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({ recording: true }), false, "Cannot stop right now."));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Replay" }));
    expect(await screen.findByText("Cannot stop right now.")).toBeInTheDocument();
    expect(api.saveLightkeySettings).not.toHaveBeenCalled();
    expect(screen.getByRole("radio", { name: "Record" })).toBeChecked();
  });

  it("Record back to Replay: a failed replay-on shows the message and re-reads the real state", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true, replay: false }));
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({ replay: false })));
    vi.mocked(api.saveLightkeySettings).mockRejectedValue(new Error("Network down"));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Replay" }));
    expect(await screen.findByText(/Network down|That did not work/)).toBeInTheDocument();
    expect(vi.mocked(api.getLightkeyStatus).mock.calls.length).toBeGreaterThan(1);
  });

  it("shows the fixture stream and says how many looks the song holds, with no take list", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({
      fixtures: { link: "live", detail: null, fixtures: 66, lost_packets: 0, recording: false },
      
    }));
    render(<LightkeyStrip lightingEnabled looks={{ count: 190, edited: false }} />);
    expect(await screen.findByText("Colour and levels: 66 fixtures")).toBeInTheDocument();
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("190 captured looks on this song");
    expect(screen.queryByRole("list", { name: "Captured songs" })).toBeNull();
    expect(screen.queryByRole("button", { name: /Clear capture/ })).toBeNull();
  });

  it("says 1 captured look in the singular", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status());
    render(<LightkeyStrip lightingEnabled looks={{ count: 1, edited: false }} />);
    expect(await screen.findByText("1 captured look on this song")).toBeInTheDocument();
  });

  it("shows the Detail setting only for Record, with its one-line explanation", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: false }));
    const { unmount } = render(<LightkeyStrip lightingEnabled />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.queryByTestId("lightkey-detail-setting")).toBeNull();
    unmount();
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true, detail_level: "normal" }));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByTestId("lightkey-detail-setting")).toHaveTextContent("Finer detail replays motion smoother but makes more rows.");
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ recording: true, detail_level: "fine" })));
    await userEvent.setup().click(screen.getByRole("radio", { name: "Fine" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ detail_level: "fine" }));
  });

  it("Record asks first when the song has edited captured looks, and does nothing on Cancel", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "connected" }));
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({ recording: true })));
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<LightkeyStrip lightingEnabled looks={{ count: 12, edited: true }} />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("radio", { name: "Record" }));
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining("Recording replaces this song's captured looks, including your edits"));
    expect(api.setLightkeyRecording).not.toHaveBeenCalled();
    confirm.mockReturnValue(true);
    await user.click(screen.getByRole("radio", { name: "Record" }));
    await waitFor(() => expect(api.setLightkeyRecording).toHaveBeenCalledWith(true));
    confirm.mockRestore();
  });

  it("Record does not ask when no captured look was edited", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "connected" }));
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({ recording: true })));
    const confirm = vi.spyOn(window, "confirm");
    render(<LightkeyStrip lightingEnabled looks={{ count: 12, edited: false }} />);
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Record" }));
    await waitFor(() => expect(api.setLightkeyRecording).toHaveBeenCalledWith(true));
    expect(confirm).not.toHaveBeenCalled();
    confirm.mockRestore();
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
    expect(lightkeyHeadline(status({ replaying_song: "Holy Forever" }), { count: 1, edited: false })).toBe("Replaying “Holy Forever”.");
    expect(lightkeyHeadline(status({ replay_blocked: "Replay is switched off." }), { count: 1, edited: false })).toBe("Replay is switched off.");
  });
  it("tells a first-time operator what to do, then counts captures", () => {
    expect(lightkeyHeadline(status())).toMatch(/Switch to Record/);
    expect(lightkeyHeadline(status(), { count: 1, edited: false })).toBe("1 captured look on this song");
    expect(lightkeyHeadline(status(), { count: 2, edited: false })).toBe("2 captured looks on this song");
  });
});

describe("lightkeyLines (B3: no line twice, replay-paused line)", () => {
  const armed = { recording: true, replay: true, replay_blocked: "Paused while recording." };

  it("Record armed: the backend message is not repeated and no paused line is added", () => {
    expect(lightkeyLines(status({ ...armed, message: "Recording starts with the next song." }), null)).toEqual([
      "Recording starts with the next song.",
    ]);
    expect(lightkeyLines(status({ ...armed, message: null }), null)).toEqual([
      "Recording starts with the next song.",
    ]);
  });

  it("recording a song: straight and curly quotes count as the same line", () => {
    expect(lightkeyLines(status({ ...armed, recording_song: "Holy Forever", message: 'Recording "Holy Forever".' }), null)).toEqual([
      'Recording "Holy Forever".',
    ]);
    expect(lightkeyLines(status({ ...armed, recording_song: "Holy Forever" }), null)).toEqual([
      "Recording “Holy Forever”.",
    ]);
  });

  it("Record armed + Replay off: no paused line", () => {
    expect(lightkeyLines(status({ recording: true, replay: false, replay_blocked: "Paused while recording.", message: "Recording starts with the next song." }), null)).toEqual([
      "Recording starts with the next song.",
    ]);
  });

  it("Record off + Replay on: no paused line, and it is gone once recording stops", () => {
    expect(lightkeyLines(status(), null, { count: 1, edited: false })).toEqual(["1 captured look on this song"]);
    expect(lightkeyLines(status({ replaying_song: "Holy Forever" }), null, { count: 1, edited: false })).toEqual(["Replaying “Holy Forever”."]);
  });

  it("Record off + Replay off: says replay is switched off", () => {
    expect(lightkeyLines(status({ replay: false, replay_blocked: "Replay is switched off." }), null, { count: 1, edited: false })).toEqual(["Replay is switched off."]);
  });

  it("a message that differs from the headline is kept next to it", () => {
    expect(lightkeyLines(status({ message: "Saved \"Holy Forever\": 14 fixture changes. Song finished." }), null, { count: 1, edited: false })).toEqual([
      'Saved "Holy Forever": 14 fixture changes. Song finished.',
      "1 captured look on this song",
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
  it("armed shows the one headline and no paused line", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true, replay: true, replay_blocked: "Paused while recording.", message: "Recording starts with the next song." }));
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.getAllByText("Recording starts with the next song.")).toHaveLength(1);
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("Recording starts with the next song.");
    expect(screen.queryAllByTestId("lightkey-detail")).toHaveLength(0);
  });

  it("recording a song shows the title once", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true, recording_song: "Holy Forever", replay_blocked: "Paused while recording.", message: 'Recording "Holy Forever".' }));
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.getAllByText(/Recording .Holy Forever/)).toHaveLength(1);
    expect(screen.queryByText("Replay paused while recording.")).toBeNull();
  });

  it("when recording stops the headline describes the captures again", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording: true, replay_blocked: "Paused while recording." }));
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({  message: null })));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({  message: null })));
    render(<LightkeyStrip lightingEnabled looks={{ count: 1, edited: false }} />);
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Replay" }));
    await waitFor(() => expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("1 captured look on this song"));
  });

  it("an expired message (backend sends null) falls back to the headline", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "unreachable", active_count: null, message: null }));
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByText("Not connected");
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("0 captured looks on this song");
  });
});
