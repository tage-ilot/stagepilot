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
  releaseLightkeyLights: vi.fn(),
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
const captured = { song_title: "Holy Forever", looks: 14 };
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
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("Cannot reach the Lightkey API.");
    expect(screen.getByRole("radio", { name: "Record" })).toBeDisabled();
  });

  it("Connect with the shared host on the default API port leaves api_url empty so the backend derives it", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "unreachable", detail: "Cannot reach the Lightkey API." }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ link: "unreachable" })));
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByLabelText("Host or IP address");
    await userEvent.setup().click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ api_url: "" }));
  });

  it("Connect writes <host>:<apiPort> when the API port is not 8770", async () => {
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status()));
    render(<LightkeyStrip lightingEnabled />);
    const port = await screen.findByLabelText("API port");
    const user = userEvent.setup();
    await user.clear(port);
    await user.type(port, "9100");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ api_url: "10.30.0.211:9100" }));
  });

  it("refuses an API port outside 1-65535 and says so", async () => {
    render(<LightkeyStrip lightingEnabled />);
    const port = await screen.findByLabelText("API port");
    const user = userEvent.setup();
    await user.clear(port);
    await user.type(port, "70000");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Enter an API port from 1 to 65535.");
    expect(api.saveLightkeySettings).not.toHaveBeenCalled();
  });

  it("a saved API address on another host shows a notice naming it and keeps using it until accepted", async () => {
    const connection = { host: "192.0.2.10", onHost: vi.fn(), oscPort: "21600", onPort: vi.fn(), savedOscHost: "192.0.2.10", attempted: false, held: { host: false, port: false } };
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ api_url: "http://192.0.2.99:8770" }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ api_url: "http://192.0.2.99:8770" })));
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    expect(await screen.findByTestId("lightkey-host-notice")).toHaveTextContent("192.0.2.99");
    const user = userEvent.setup();
    // Connect never rewrites it: the saved address goes back unchanged.
    await user.click(screen.getByRole("button", { name: /Connect|Check again/ }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ api_url: "http://192.0.2.99:8770" }));
    vi.mocked(api.saveLightkeySettings).mockClear();
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ api_url: "http://192.0.2.10:8770" })));
    await user.click(screen.getByRole("button", { name: "Use the OSC host" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ api_url: "" }));
    await waitFor(() => expect(screen.queryByTestId("lightkey-host-notice")).toBeNull());
  });

  it("no notice when the API is on the OSC host, and the OSC UDP box edits the panel's port", async () => {
    const onPort = vi.fn();
    const connection = { host: "10.30.0.211", onHost: vi.fn(), oscPort: "21600", onPort, savedOscHost: "10.30.0.211", attempted: false, held: { host: false, port: false } };
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByLabelText("Host or IP address");
    expect(screen.queryByTestId("lightkey-host-notice")).toBeNull();
    await userEvent.setup().type(screen.getByLabelText("OSC UDP port"), "1");
    expect(onPort).toHaveBeenCalledWith("216001");
  });

  it("is one titled line: status, host, OSC UDP, API, Connect and Replay | Record share a single wrapping row", async () => {
    const connection = { host: "10.30.0.211", onHost: vi.fn(), oscPort: "21600", onPort: vi.fn(), savedOscHost: "10.30.0.211", attempted: false, held: { host: false, port: false } };
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByLabelText("API port");
    expect(screen.getByRole("heading", { name: "sACN DMX Live Link" })).toBeInTheDocument();
    const line = screen.getByTestId("live-link-line");
    expect(line.className).toContain("flex-wrap");
    for (const name of ["Host or IP address", "OSC UDP port", "API port"]) expect(line).toContainElement(screen.getByLabelText(name));
    expect(line).toContainElement(screen.getByRole("button", { name: /Connect|Check again/ }));
    expect(line).toContainElement(screen.getByRole("radiogroup", { name: "Mode" }));
    // The one state pill sits in the header beside the title, not in the field row.
    expect(line).not.toContainElement(screen.getByTestId("lightkey-link"));
    expect(screen.getByLabelText("API port")).toHaveValue("8770");
    // Release lights sits on the second line, outside the first.
    expect(line).not.toContainElement(screen.getByRole("button", { name: "Release lights" }));
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
    await waitFor(() => expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("Recording needs Lighting set to OSC."));
    expect(screen.getByRole("radio", { name: "Replay" })).toBeChecked();
  });

  it("one Replay | Record toggle replaces the two switches, Replay by default", async () => {
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByRole("radiogroup", { name: "Mode" })).toBeInTheDocument();
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
    await waitFor(() => expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("Cannot stop right now."));
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

  it("says how many captured looks the last saved song has, and no longer lists takes", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({
      fixtures: { link: "live", detail: null, fixtures: 66, lost_packets: 0, recording: false },
      
    }));
    render(<LightkeyStrip lightingEnabled looks={{ count: 190, edited: false }} />);
    expect(await screen.findByText("Connected \u00B7 66 fixtures")).toBeInTheDocument();
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("190 captured items (190 ramps/steps, 0 loops) on this song");
    expect(screen.queryByRole("list", { name: "Captured songs" })).toBeNull();
    expect(screen.queryByRole("button", { name: /Clear capture/ })).toBeNull();
  });

  it("says 1 captured item in the singular, and counts loops separately", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status());
    render(<LightkeyStrip lightingEnabled looks={{ count: 1, edited: false }} />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("1 captured item (1 ramps/steps, 0 loops) on this song.");
  });

  it("counts loops with the ramps and steps: N items = rows + loops", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status());
    render(<LightkeyStrip lightingEnabled looks={{ count: 546, loops: 6, edited: false }} />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("552 captured items (546 ramps/steps, 6 loops) on this song.");
  });

  it("has no Detail setting in any mode, and never sends detail_level", async () => {
    for (const recording of [false, true]) {
      vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ recording, detail_level: "normal" }));
      const { unmount } = render(<LightkeyStrip lightingEnabled />);
      await screen.findByTestId("lightkey-headline");
      expect(screen.queryByTestId("lightkey-detail-setting")).toBeNull();
      expect(screen.queryByRole("radiogroup", { name: "Detail" })).toBeNull();
      expect(screen.queryByRole("radio", { name: /^(Fine|Normal|Coarse)$/ })).toBeNull();
      unmount();
    }
    expect(api.saveLightkeySettings).not.toHaveBeenCalled();
  });

  it("Record asks first when the song has edited captured looks, and does nothing on Cancel", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "connected" }));
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({ recording: true })));
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<LightkeyStrip lightingEnabled looks={{ count: 12, edited: true }} />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("radio", { name: "Record" }));
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining("Recording replaces this song's captured looks and loops, including your edits"));
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
    expect(await screen.findByText("Connecting\u2026")).toBeInTheDocument();
    expect(screen.getByTestId("lightkey-link")).toHaveAttribute("title", expect.stringContaining("Colour and levels: connecting"));
  });

  it("says in words when the fixture stream stops, and says nothing for an older Lightkey API", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({
      fixtures: { link: "lost", detail: "No fixture data from the Lightkey API.", fixtures: 66, lost_packets: 2, recording: false },
    }));
    const { unmount } = render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByText("Connected \u00B7 not receiving")).toBeInTheDocument();
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("No fixture data from the Lightkey API.");
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
    expect(lightkeyHeadline(status(), { count: 1, edited: false })).toBe("1 captured item (1 ramps/steps, 0 loops) on this song");
    expect(lightkeyHeadline(status(), { count: 2, edited: false })).toBe("2 captured items (2 ramps/steps, 0 loops) on this song");
  });
});

describe("lightkeyLines (B3: no line twice, replay-paused line)", () => {
  const armed = { recording: true, replay: true, replay_blocked: "Paused while recording.", captured };

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
    expect(lightkeyLines(status({ captured }), null)).toEqual(["14 captured items on “Holy Forever”."]);
    expect(lightkeyLines(status({ captured, replaying_song: "Holy Forever" }), null)).toEqual(["Replaying “Holy Forever”."]);
  });

  it("Record off + Replay off: says replay is switched off", () => {
    expect(lightkeyLines(status({ replay: false, replay_blocked: "Replay is switched off.", captured }), null)).toEqual(["Replay is switched off."]);
  });

  it("a message that differs from the headline is kept next to it", () => {
    expect(lightkeyLines(status({ captured, message: "Saved 14 looks to Holy Forever's timeline." }), null)).toEqual([
      "Saved 14 looks to Holy Forever's timeline.",
      "14 captured items on “Holy Forever”.",
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
    vi.mocked(api.setLightkeyRecording).mockResolvedValue(respond(status({ captured, message: null })));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ captured, message: null })));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Replay" }));
    await waitFor(() => expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("14 captured items on “Holy Forever”."));
  });

  it("an expired message (backend sends null) falls back to the headline", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "unreachable", active_count: null, message: null }));
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByText("Not connected");
    expect(screen.getByTestId("lightkey-headline")).toHaveTextContent("No lights captured yet.");
  });
});

describe("LightkeyStrip operator override", () => {
  it("shows what the replay holds and what the operator has", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ holding_fixtures: 5, yielded_fixtures: 2 }));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByTestId("lightkey-holding")).toHaveTextContent("Holding 5 fixtures \u00B7 2 yielded to the operator");
  });

  it("uses the singular and hides the line when nothing is held or yielded", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ holding_fixtures: 1 }));
    const { unmount } = render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByTestId("lightkey-holding")).toHaveTextContent("Holding 1 fixture");
    unmount();
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status());
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.queryByTestId("lightkey-holding")).not.toBeInTheDocument();
  });

  it("Release lights is disabled when nothing is held", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ yielded_fixtures: 2 }));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByRole("button", { name: "Release lights" })).toBeDisabled();
  });

  it("Release lights calls the backend and shows the new status", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ holding_fixtures: 3 }));
    vi.mocked(api.releaseLightkeyLights).mockResolvedValue(respond(status(), true, "Released 3 fixtures."));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Release lights" }));
    await waitFor(() => expect(api.releaseLightkeyLights).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByTestId("lightkey-holding")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Release lights" })).toBeDisabled();
  });
});

describe("LightkeyStrip saved replay off and release after a restart", () => {
  it("F-02: shows a warning and a way back when the saved replay setting is off", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ replay: false, recording: false }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ replay: true })));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByTestId("lightkey-replay-off")).toHaveTextContent("Replay is off");
    await userEvent.setup().click(screen.getByRole("button", { name: "Turn Replay on" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ replay: true }));
    await waitFor(() => expect(screen.queryByTestId("lightkey-replay-off")).not.toBeInTheDocument());
  });

  it("no warning while replay is on or while recording", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ replay: true }));
    const { unmount } = render(<LightkeyStrip lightingEnabled />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.queryByTestId("lightkey-replay-off")).not.toBeInTheDocument();
    unmount();
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ replay: false, recording: true }));
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByTestId("lightkey-headline");
    expect(screen.queryByTestId("lightkey-replay-off")).not.toBeInTheDocument();
  });

  it("H-01: Release lights is usable when the looks name fixtures but nothing is held (after a restart)", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ known_fixtures: 4 }));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByRole("button", { name: "Release lights" })).toBeEnabled();
  });
});

describe("Auto | Replay | Record", () => {
  it("offers three options and shows the saved mode", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ mode: "auto" }));
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByRole("radio", { name: "Auto" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Replay" })).not.toBeChecked();
    expect(screen.getByRole("radio", { name: "Record" })).not.toBeChecked();
  });

  it("choosing Replay pins it and Auto un-pins it, through the settings", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ mode: "auto" }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ mode: "replay" })));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("radio", { name: "Replay" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ mode: "replay", replay: true }));
  });

  it("in Auto the headline is the decision and its reason; capturing names the song", () => {
    const auto = { capturing: false, reason: "Auto: replaying (fully captured)", song: "Holy Forever" };
    expect(lightkeyHeadline(status({ mode: "auto", auto }))).toBe("Auto: replaying (fully captured)");
    expect(lightkeyHeadline(status({ mode: "auto", recording_song: "Holy Forever", auto: { ...auto, capturing: true, reason: "Auto: capturing this song" } }))).toBe("Auto: capturing “Holy Forever”.");
    expect(lightkeyHeadline(status({ mode: "replay" }))).not.toContain("Auto");
  });

  it("the song header says what the rows are", () => {
    expect(lightkeyHeadline(status({ mode: "replay" }), { count: 12, edited: false, label: "fully_captured" })).toBe("Fully captured: 12 captured items (12 ramps/steps, 0 loops) on this song");
    expect(lightkeyHeadline(status({ mode: "replay" }), { count: 0, edited: false, label: "manually_cued" })).toBe("Manually cued on this song");
  });
});
