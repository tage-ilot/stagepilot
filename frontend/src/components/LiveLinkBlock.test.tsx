import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { ApplicationState, LightkeyStatus, LightsStatusResponse, SettingsResponse } from "../types";
import { LightingSetupPanel } from "./LightingSetupPanel";
import { LightkeyStrip } from "./LightkeyStrip";
import { CUE_ADDRESS_HELP, liveLinkPillState, shortLightingMessage } from "./liveLink";
import { statusTone } from "./ui/statusWord";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getLightkeyStatus: vi.fn(),
  saveLightkeySettings: vi.fn(),
  setLightkeyRecording: vi.fn(),
  releaseLightkeyLights: vi.fn(),
  setLightkeyAudioMeter: vi.fn().mockResolvedValue({ available: false, reason: "No input.", devices: [], selected: null, note: null, level: null, metering: false }),
}));

const status = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://192.0.2.10:8770", link: "connected", detail: null, active_count: 11, recording: false, recording_song: null,
  replay: true, replay_blocked: null, replaying_song: null, message: null, ...over,
});
const live = (fixtures = 66) => ({ link: "live" as const, detail: null, fixtures, lost_packets: 0, recording: false });
const connection = { host: "192.0.2.10", onHost: vi.fn(), oscPort: "21600", onPort: vi.fn(), savedOscHost: "192.0.2.10", attempted: false, held: { host: false, port: false } };
const down = status({ link: "unreachable", detail: "Lightkey API did not answer.", mode: "auto", auto: { capturing: false, reason: "Auto: replaying (no connection)", song: null } });

beforeEach(() => { vi.mocked(api.getLightkeyStatus).mockResolvedValue(status()); });
afterEach(() => { vi.clearAllMocks(); });

const pill = () => screen.getByTestId("lightkey-link");
const strip = () => screen.getByTestId("lightkey-strip");
const occurrences = (text: string, needle: string) => text.split(needle).length - 1;

describe("Live Link block: the state pill is the one statement of the connection", () => {
  it("says Connected \u00B7 N fixtures with a green dot, announced politely", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ fixtures: live(66) }));
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByText("Connected \u00B7 66 fixtures");
    expect(pill()).toHaveAttribute("role", "status");
    expect(pill()).toHaveAttribute("aria-live", "polite");
    expect(pill()).toHaveAttribute("data-tone", "success");
    expect(within(strip()).getByRole("heading", { name: "sACN DMX Live Link" }).parentElement).toContainElement(pill());
  });

  it("says Not connected (red) and puts the reason in the tooltip and accessible name, and ONCE in the strip text", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(down);
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByText("Not connected");
    expect(pill()).toHaveAttribute("data-tone", "danger");
    expect(pill()).toHaveAttribute("title", expect.stringContaining("Lightkey API did not answer."));
    expect(pill()).toHaveAccessibleName(expect.stringContaining("Not connected"));
    expect(pill()).toHaveAccessibleName(expect.stringContaining("Lightkey API did not answer."));
    const text = strip().textContent ?? "";
    expect(occurrences(text, "Not connected")).toBe(1);
    expect(occurrences(text, "Lightkey API did not answer")).toBe(1);
    expect(text).toContain("Auto: replaying (no connection)");
  });

  it("says Connecting\u2026 (amber) while the first fixture data is awaited, and the tooltip says why", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ fixtures: { link: "waiting", detail: null, fixtures: 0, lost_packets: 0, recording: false } }));
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByText("Connecting\u2026");
    expect(pill()).toHaveAttribute("data-tone", "warning");
    expect(pill()).toHaveAttribute("title", expect.stringContaining("Colour and levels: connecting"));
  });

  it("derives the colour from the same word table as the panel header's pill", () => {
    expect(liveLinkPillState(status({ fixtures: live() })).tone).toBe(statusTone.Connected);
    expect(liveLinkPillState(down).tone).toBe(statusTone.Error);
    expect(liveLinkPillState(status({ link: "lightkey_closed", detail: "Lightkey is not running." })).tone).toBe(statusTone.Connecting);
    expect(liveLinkPillState(status({ link: "off" })).tone).toBe(statusTone.Disconnected);
    // The pill always has words, so no state rests on colour alone.
    for (const link of ["off", "connected", "lightkey_closed", "unreachable"] as const) expect(liveLinkPillState(status({ link })).text.length).toBeGreaterThan(2);
  });

  it("shows no separate plain status word or dot text in the field row", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(down);
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByText("Not connected");
    expect(screen.getByTestId("live-link-line").textContent).not.toMatch(/Not connected|Connected|did not answer/);
  });
});

describe("Live Link block: Connect button", () => {
  it("reads Connect when not connected and Check again when connected, enabled and not dimmed", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(down);
    const { unmount } = render(<LightkeyStrip connection={connection} lightingEnabled />);
    const connect = await screen.findByRole("button", { name: "Connect" });
    expect(connect).toBeEnabled();
    expect(connect.className).not.toContain("opacity-40");
    unmount();
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ fixtures: live() }));
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    expect(await screen.findByRole("button", { name: "Check again" })).toBeEnabled();
  });

  it("shows a busy state (not a dimmed one) while a request is in flight", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(down);
    let release: (value: { accepted: boolean; message: string; lightkey: LightkeyStatus }) => void = () => undefined;
    vi.mocked(api.saveLightkeySettings).mockReturnValue(new Promise((resolve) => { release = resolve; }));
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Connect" }));
    const busy = await screen.findByRole("button", { name: "Checking\u2026" });
    expect(busy).toHaveAttribute("aria-busy", "true");
    expect(busy).not.toBeDisabled();
    expect(busy.className).not.toContain("opacity-40");
    release({ accepted: true, message: "ok", lightkey: down });
    await screen.findByRole("button", { name: "Connect" });
  });

  it("is disabled only when Lighting itself is off", async () => {
    render(<LightkeyStrip connection={connection} lightingEnabled={false} />);
    expect(await screen.findByRole("button", { name: /Check again|Connect/ })).toBeDisabled();
  });
});

describe("Live Link block: Mode and Detail", () => {
  it("labels Mode and Detail visibly and puts them after Connect, Mode first, on the field row", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ mode: "auto", fixtures: live() }));
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    const mode = await screen.findByRole("radiogroup", { name: "Mode" });
    const detail = screen.getByRole("radiogroup", { name: "Detail" });
    const line = screen.getByTestId("live-link-line");
    expect(line).toContainElement(mode);
    expect(line).toContainElement(detail);
    expect(within(mode).getAllByRole("radio").map((radio) => radio.getAttribute("value"))).toEqual(["auto", "replay", "record"]);
    for (const name of ["Mode", "Detail"]) {
      const legend = screen.getByText(name, { selector: "legend" });
      expect(legend.className).not.toContain("sr-only");
      expect(legend.className).toContain("uppercase");
    }
    // Mode precedes Detail in the DOM, so the keyboard reaches Mode first.
    expect(mode.compareDocumentPosition(detail) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("shows Detail in Auto and Record and hides it in Replay", async () => {
    for (const [mode, expected] of [["auto", true], ["replay", false], ["record", true]] as const) {
      vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ mode, fixtures: live() }));
      const { unmount } = render(<LightkeyStrip connection={connection} lightingEnabled />);
      await screen.findByRole("radiogroup", { name: "Mode" });
      expect(screen.queryByRole("radiogroup", { name: "Detail" }) !== null).toBe(expected);
      unmount();
    }
  });

  it("the Detail explanation is the toggle's tooltip and description, not a visible paragraph", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ mode: "auto", fixtures: live() }));
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    const detail = await screen.findByRole("radiogroup", { name: "Detail" });
    const sentence = "Finer detail replays motion smoother but makes more rows.";
    expect(detail).toHaveAttribute("title", sentence);
    expect(detail).toHaveAccessibleDescription(sentence);
    const visible = screen.getAllByText(sentence);
    expect(visible).toHaveLength(1);
    expect(visible[0]).toHaveClass("sr-only");
  });

  it("both toggles work from the keyboard (arrow keys change the choice)", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ mode: "auto", detail_level: "normal", fixtures: live() }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue({ accepted: true, message: "ok", lightkey: status({ mode: "auto", detail_level: "coarse", fixtures: live() }) });
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    const user = userEvent.setup();
    const normal = await screen.findByRole("radio", { name: "Normal" });
    normal.focus();
    await user.keyboard("{ArrowRight}");
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ detail_level: "coarse" }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue({ accepted: true, message: "ok", lightkey: status({ mode: "replay", fixtures: live() }) });
    screen.getByRole("radio", { name: "Auto" }).focus();
    await user.keyboard("{ArrowRight}");
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ mode: "replay", replay: true }));
  });
});

describe("Live Link block: the strip under the fields", () => {
  it("tab order is host, UDP, API, Connect, Mode, Detail, Release lights, Send a test cue, How to find", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ mode: "auto", holding_fixtures: 3, fixtures: live() }));
    render(<LightkeyStrip connection={connection} lightingEnabled testCue={<p>cue fields</p>} />);
    await screen.findByRole("radiogroup", { name: "Mode" });
    const user = userEvent.setup();
    const names: string[] = [];
    for (let step = 0; step < 9; step += 1) {
      await user.tab();
      const el = document.activeElement as HTMLElement;
      names.push(el.getAttribute("aria-label") ?? (el.getAttribute("type") === "radio" ? `radio:${el.getAttribute("value")}` : (el.textContent ?? "")));
    }
    expect(names).toEqual([
      "Host or IP address", "OSC UDP port", "API port", "Check again", "radio:auto", "radio:normal",
      "Release lights", "Send a test cue\u25B8", "How to find a cue's address\u25B8",
    ]);
  });

  it("Send a test cue uses the conventional arrow: right when closed, down when open, and opens a nested box", async () => {
    render(<LightkeyStrip connection={connection} lightingEnabled testCue={<p>cue fields</p>} />);
    await screen.findByRole("radiogroup", { name: "Mode" }); // the live status has arrived
    const toggle = screen.getByRole("button", { name: "Send a test cue" });
    const box = screen.getByTestId("test-cue-box");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle.querySelector('[data-part="arrow"]')).toHaveTextContent("\u25B8");
    expect(box).not.toBeVisible();
    await userEvent.setup().click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(toggle.querySelector('[data-part="arrow"]')).toHaveTextContent("\u25BE");
    expect(box).toBeVisible();
    expect(toggle).toHaveAttribute("aria-controls", box.id);
    expect(strip()).toContainElement(box);
  });

  it("the cue-address help is behind its own disclosure and is fully readable (not truncated)", async () => {
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByRole("radiogroup", { name: "Mode" });
    const toggle = screen.getByRole("button", { name: "How to find a cue's address" });
    const help = screen.getByTestId("cue-address-help");
    expect(help).toHaveTextContent(CUE_ADDRESS_HELP);
    expect(help.textContent).toBe("Enter the port your lighting app listens for OSC on. In Lightkey, control-click a cue and choose External Control, then Copy OSC Address, to get a cue's address.");
    expect(help.className).not.toMatch(/truncate|line-clamp|overflow-hidden/);
    expect(help).not.toBeVisible();
    await userEvent.setup().click(toggle);
    expect(help).toBeVisible();
    expect(toggle).toHaveAttribute("aria-expanded", "true");
  });

  it("Release lights stays in the strip and is disabled when nothing is held", async () => {
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    const release = await screen.findByRole("button", { name: "Release lights" });
    expect(release).toBeDisabled();
    expect(screen.getByTestId("live-link-sub")).toContainElement(release);
  });
});

describe("shortLightingMessage", () => {
  it("shortens the backend's ready note to one sentence pair and leaves other messages alone", () => {
    expect(shortLightingMessage("OSC ready for 192.0.2.10:21600. Nothing sent yet; UDP does not confirm delivery.")).toBe("OSC ready for 192.0.2.10:21600. UDP does not confirm delivery.");
    expect(shortLightingMessage("OSC output to 192.0.2.10:21600 reopened. UDP does not confirm delivery.")).toBe("OSC output to 192.0.2.10:21600 reopened. UDP does not confirm delivery.");
    expect(shortLightingMessage("Sent lighting test cue on MIDI note 60")).toBe("Sent lighting test cue on MIDI note 60");
  });
});

describe("Lighting panel: each status fact once", () => {
  const lights = {
    osc_host: "192.0.2.10", osc_port: 21600, enabled: true, output_name: null, channel: 1, pulse_ms: 100, method: "osc",
    connection_status: "ready", detail: "OSC ready for 192.0.2.10:21600. Nothing sent yet; UDP does not confirm delivery.", outputs: [], last_cue: null, last_osc_cue: null, last_cue_at: null,
  } as unknown as LightsStatusResponse;
  const settings = { planning_center_secret_saved: true, warning: null, restart_required: false, settings: { lights: { osc_host: "192.0.2.10", osc_port: 21600, enabled: true, method: "osc", channel: 1, pulse_ms: 100, output_name: null, cue_maps: {} } } } as unknown as SettingsResponse;
  const state = { plan: { id: "p", title: "Sunday", songs: [] } } as unknown as ApplicationState;
  const mount = (over: Partial<LightsStatusResponse> = {}, message: string | null = lights.detail) => render(
    <LightingSetupPanel error={null} lights={{ ...lights, ...over }} message={message} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onSavePads={vi.fn()} onPressPad={vi.fn()} onUndoCapture={vi.fn()} onTest={vi.fn()} onTestOsc={vi.fn()} pendingOperation={null} settings={settings} state={state} />,
  );

  it("shows the OSC-ready note once, short, as a neutral info line; the footer duplicate is gone", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(down);
    const { container } = mount();
    await screen.findByLabelText("API port");
    const text = container.textContent ?? "";
    expect(occurrences(text, "OSC ready for 192.0.2.10:21600")).toBe(1);
    expect(occurrences(text, "UDP does not confirm delivery")).toBe(1);
    expect(text).not.toContain("Nothing sent yet");
    expect(text).not.toContain("Ready, delivery not confirmed");
    expect(text).not.toContain("Use Send test cue to check");
    const note = screen.getByText(/^OSC ready for/).closest('[data-tone]')!;
    expect(note).toHaveAttribute("data-tone", "info");
    // Neutral: the plain info look, not the Lighting pink-red banner.
    expect(note.className).toContain("text-info-text");
    expect(note.className).not.toContain("integration-lighting");
    expect(screen.queryByTestId("osc-status")).toBeNull();
  });

  it("keeps the warning look and the words when the state really is an error", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(down);
    mount({ connection_status: "error", detail: "OSC send failed: network unreachable." }, null);
    await screen.findByLabelText("API port");
    const line = screen.getByTestId("osc-status");
    expect(line).toHaveTextContent("OSC send failed: network unreachable.");
    expect(line.className).toContain("text-danger-text");
  });

  it("says Not connected once and 'Lightkey API did not answer' once, and the help sentence is present once", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(down);
    const { container } = mount();
    await screen.findByText("Not connected");
    const text = container.textContent ?? "";
    expect(occurrences(text, "Not connected")).toBe(1);
    expect(occurrences(text, "Lightkey API did not answer")).toBe(1);
    expect(occurrences(text, "control-click a cue")).toBe(1);
  });

  it("the test cue opens as a nested box with the OSC address, the argument rows and Send test cue", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(down);
    mount();
    await screen.findByLabelText("API port");
    const box = screen.getByTestId("test-cue-box");
    expect(box).not.toBeVisible();
    await userEvent.setup().click(screen.getByRole("button", { name: "Send a test cue" }));
    const group = within(box).getByRole("group", { name: "OSC test cue" });
    expect(within(group).getByLabelText("OSC address")).toBeVisible();
    expect(within(group).getByRole("button", { name: "Add argument" })).toBeVisible();
    expect(within(group).getByRole("button", { name: "Send test cue" })).toBeVisible();
  });
});
