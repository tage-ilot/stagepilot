import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { LightkeyDiscovery, LightkeyStatus } from "../types";
import { LightkeyDiscoveryNotice } from "./LightkeyDiscoveryNotice";
import { LightkeyStrip } from "./LightkeyStrip";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getLightkeyStatus: vi.fn(),
  saveLightkeySettings: vi.fn(),
  searchLightkeyAgain: vi.fn(),
  switchToLightkeyComputer: vi.fn(),
  keepWaitingForLightkey: vi.fn(),
  setLightkeyAudioMeter: vi.fn().mockResolvedValue({ available: false, reason: "No input.", devices: [], selected: null, note: null, level: null, metering: false }),
}));

const discovery = (over: Partial<LightkeyDiscovery> = {}): LightkeyDiscovery => ({ state: "none", found: [], refusing: [], ...over });
const status = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://10.30.0.211:8770",
  link: "unreachable",
  detail: "Cannot reach the Lightkey API.",
  active_count: null,
  recording: false,
  recording_song: null,
  replay: true,
  replay_blocked: null,
  replaying_song: null,
  message: null,
  ...over,
});
const respond = (lightkey: LightkeyStatus) => ({ accepted: true, message: "ok", lightkey });

const noop = { busy: false, onUse: vi.fn(), onKeepWaiting: vi.fn(), onChoose: vi.fn() };
afterEach(() => { vi.clearAllMocks(); });

describe("LightkeyDiscoveryNotice: every message and button", () => {
  it("says nothing without a discovery block, or when the search is off", () => {
    const { container, rerender } = render(<LightkeyDiscoveryNotice {...noop} discovery={undefined} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<LightkeyDiscoveryNotice {...noop} discovery={discovery({ state: "off" })} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("looks: 'Looking for a Lighting computer on this network…'", () => {
    render(<LightkeyDiscoveryNotice {...noop} discovery={discovery({ state: "searching" })} />);
    expect(screen.getByTestId("lightkey-discovery")).toHaveTextContent("Looking for a Lighting computer on this network\u2026");
  });

  it("found one and connected: the sentence, and the Capture/Replay line when they differ", () => {
    render(<LightkeyDiscoveryNotice {...noop} discovery={discovery({ state: "connected", message: "Found a Lighting computer at 10.30.0.211 and connected.", note: "Capture is from 10.30.0.211; Replay sends to 10.30.0.99." })} />);
    expect(screen.getByTestId("lightkey-discovery-message")).toHaveTextContent("Found a Lighting computer at 10.30.0.211 and connected.");
    expect(screen.getByTestId("lightkey-discovery-note")).toHaveTextContent("Capture is from 10.30.0.211; Replay sends to 10.30.0.99.");
  });

  it("chosen and connected: nothing is said at all", () => {
    render(<LightkeyDiscoveryNotice {...noop} discovery={discovery({ state: "connected", chosen: "10.30.0.211" })} />);
    expect(screen.getByTestId("lightkey-discovery")).toBeEmptyDOMElement();
  });

  it("several found: 'Found <n> Lighting computers. Choose one:' and a button each, in the chooser every scan uses", async () => {
    const onChoose = vi.fn();
    render(<LightkeyDiscoveryNotice {...noop} onChoose={onChoose} discovery={discovery({
      state: "choose",
      found: [
        { address: "10.30.0.211", lightkey_running: true, version: "0.5.0" },
        { address: "10.30.0.212:9000", lightkey_running: false },
      ],
    })} />);
    const group = screen.getByRole("group", { name: "Found 2 Lighting computers. Choose one:" });
    expect(within(group).getByText("Found 2 Lighting computers. Choose one:")).toBeInTheDocument();
    const buttons = within(group).getAllByRole("button");
    expect(buttons.map((button) => button.textContent)).toEqual(["10.30.0.211Lightkey running", "10.30.0.212:9000Lightkey not running"]);
    await userEvent.setup().click(buttons[1] as HTMLElement);
    // Choosing puts the computer in the Host box; nothing connects and nothing is sent until Connect.
    expect(onChoose).toHaveBeenCalledWith("10.30.0.212:9000");
    expect(noop.onUse).not.toHaveBeenCalled();
  });

  it("a chosen address that is down: the offer with 'Use <ip>' and 'Keep waiting'", async () => {
    const onUse = vi.fn();
    const onKeepWaiting = vi.fn();
    render(<LightkeyDiscoveryNotice {...noop} onKeepWaiting={onKeepWaiting} onUse={onUse} discovery={discovery({
      state: "offer",
      chosen: "10.30.0.50",
      found: [{ address: "10.30.0.211", lightkey_running: true }],
    })} />);
    expect(screen.getByTestId("lightkey-discovery-offer")).toHaveTextContent("10.30.0.50 isn't answering. A Lighting computer answered at 10.30.0.211. Use it?");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Use 10.30.0.211" }));
    expect(onUse).toHaveBeenCalledWith("10.30.0.211");
    await user.click(screen.getByRole("button", { name: "Keep waiting" }));
    expect(onKeepWaiting).toHaveBeenCalledWith("10.30.0.211");
  });

  it("nothing found: an ordinary line (no alert, no error colour) and no button of its own: Scan in the row looks again", () => {
    render(<LightkeyDiscoveryNotice {...noop} discovery={discovery({ state: "none" })} />);
    const line = screen.getByTestId("lightkey-discovery-none");
    expect(line).toHaveTextContent("No Lighting computer found on this network.");
    expect(screen.getByTestId("lightkey-discovery")).not.toHaveAttribute("role", "alert");
    expect(screen.getByTestId("lightkey-discovery").innerHTML).not.toMatch(/danger/);
    expect(within(line).queryByRole("button")).toBeNull();
    expect(screen.queryByRole("button", { name: /Search again|Scan/ })).toBeNull();
  });

  it("a refusing computer: its own line, beside whatever else is said", () => {
    render(<LightkeyDiscoveryNotice {...noop} discovery={discovery({ state: "none", refusing: ["10.30.0.9", "10.30.0.10"] })} />);
    const lines = screen.getAllByTestId("lightkey-discovery-refusing");
    expect(lines[0]).toHaveTextContent("A Lighting computer at 10.30.0.9 is refusing StagePilot. Run the setup command on it again.");
    expect(lines[1]).toHaveTextContent("A Lighting computer at 10.30.0.10 is refusing StagePilot. Run the setup command on it again.");
    expect(screen.getByTestId("lightkey-discovery-none")).toBeInTheDocument();
  });

  it("buttons are off while a request is running", () => {
    render(<LightkeyDiscoveryNotice {...noop} busy discovery={discovery({ state: "offer", chosen: "a", found: [{ address: "10.0.0.2", lightkey_running: true }] })} />);
    expect(screen.getByRole("button", { name: "Use 10.0.0.2" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Keep waiting" })).toBeDisabled();
  });

  it("never uses the sACN, OSC or port words in operator text", () => {
    const { container } = render(<LightkeyDiscoveryNotice {...noop} discovery={discovery({ state: "none", refusing: ["10.30.0.9"] })} />);
    expect(container.textContent).not.toMatch(/sACN|OSC|8770|port/i);
  });
});

describe("LightkeyStrip with the search", () => {
  beforeEach(() => { vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ discovery: discovery({ state: "none" }) })); });

  it("shows the line and keeps the setup box when nothing is found", async () => {
    render(<LightkeyStrip lightingEnabled />);
    expect(await screen.findByTestId("lightkey-discovery-none")).toHaveTextContent("No Lighting computer found on this network.");
    expect(screen.getByTestId("lightkey-install")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Set up the Lighting computer" })).toBeInTheDocument();
  });

  it("Scan starts the search now; Keep waiting calls the backend and shows its answer", async () => {
    const user = userEvent.setup();
    vi.mocked(api.searchLightkeyAgain).mockResolvedValue(respond(status({ discovery: discovery({ state: "searching" }) })));
    render(<LightkeyStrip lightingEnabled />);
    await user.click(await screen.findByRole("button", { name: "Scan" }));
    await waitFor(() => expect(api.searchLightkeyAgain).toHaveBeenCalledTimes(1));
    expect(await screen.findByTestId("lightkey-discovery-searching")).toHaveTextContent("Looking for a Lighting computer on this network\u2026");
    expect(screen.getByRole("button", { name: "Scanning\u2026 (cancel)" })).toBeEnabled();

    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ discovery: discovery({ state: "offer", chosen: "10.30.0.50", found: [{ address: "10.30.0.211", lightkey_running: true }] }) }));
    vi.mocked(api.keepWaitingForLightkey).mockResolvedValue(respond(status({ discovery: discovery({ state: "none", chosen: "10.30.0.50" }) })));
    vi.mocked(api.switchToLightkeyComputer).mockResolvedValue(respond(status({ link: "connected", discovery: discovery({ state: "connected" }) })));
    await user.click(await screen.findByRole("button", { name: "Keep waiting" }, { timeout: 4000 }));
    await waitFor(() => expect(api.keepWaitingForLightkey).toHaveBeenCalledWith("10.30.0.211"));
    expect(await screen.findByTestId("lightkey-discovery-none")).toBeInTheDocument();
  });

  it("Scanning… (cancel) stops showing the search (it keeps its own schedule in the background), and Scan starts it again", async () => {
    const user = userEvent.setup();
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ discovery: discovery({ state: "searching" }) }));
    vi.mocked(api.searchLightkeyAgain).mockResolvedValue(respond(status({ discovery: discovery({ state: "searching" }) })));
    render(<LightkeyStrip lightingEnabled />);
    await user.click(await screen.findByRole("button", { name: "Scanning\u2026 (cancel)" }));
    expect(screen.getByRole("button", { name: "Scan" })).toBeEnabled();
    expect(screen.queryByTestId("lightkey-discovery-searching")).toBeNull();
    expect(api.searchLightkeyAgain).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Scan" }));
    await waitFor(() => expect(api.searchLightkeyAgain).toHaveBeenCalledTimes(1));
    expect(await screen.findByTestId("lightkey-discovery-searching")).toBeInTheDocument();
  });

  it("Scan on a computer that is already connected is answered in the line (the backend says it is not searching)", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "connected", detail: null, discovery: discovery({ state: "connected" }) }));
    vi.mocked(api.searchLightkeyAgain).mockResolvedValue({ accepted: false, message: "A Lighting computer is already connected.", lightkey: status({ link: "connected", detail: null, discovery: discovery({ state: "connected" }) }) });
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Scan" }));
    expect(await screen.findByText(/A Lighting computer is already connected/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Scan" })).toBeEnabled();
  });

  it("choosing one of several puts it in Host (and so in the helper's address); nothing is sent until Connect", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ discovery: discovery({ state: "choose", found: [{ address: "10.30.0.211", lightkey_running: true }, { address: "10.30.0.212", lightkey_running: true }] }) }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status({ discovery: discovery({ state: "choose" }) })));
    const user = userEvent.setup();
    render(<LightkeyStrip lightingEnabled />);
    await user.click(await screen.findByRole("button", { name: /10\.30\.0\.212/ }));
    expect(screen.getByLabelText("Lighting host or IP address")).toHaveValue("10.30.0.212");
    expect(api.switchToLightkeyComputer).not.toHaveBeenCalled();
    expect(api.saveLightkeySettings).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ api_url: expect.stringContaining("10.30.0.212") }));
  });

  it("'Use <ip>' sends that address", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ discovery: discovery({ state: "offer", chosen: "10.30.0.50", found: [{ address: "10.30.0.211", lightkey_running: false }] }) }));
    vi.mocked(api.switchToLightkeyComputer).mockResolvedValue(respond(status({ link: "connected", discovery: discovery({ state: "connected" }) })));
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Use 10.30.0.211" }));
    await waitFor(() => expect(api.switchToLightkeyComputer).toHaveBeenCalledWith("10.30.0.211"));
  });

  it("a refused 'use' is shown in plain words and nothing else changes", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ discovery: discovery({ state: "offer", chosen: "10.30.0.50", found: [{ address: "10.30.0.211", lightkey_running: true }] }) }));
    vi.mocked(api.switchToLightkeyComputer).mockResolvedValue({ accepted: false, message: "That Lighting computer was not found by the last search. Press Scan.", lightkey: status({ discovery: discovery({ state: "none" }) }) });
    render(<LightkeyStrip lightingEnabled />);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Use 10.30.0.211" }));
    expect(await screen.findByText(/was not found by the last search. Press Scan./)).toBeInTheDocument();
  });

  it("in Automatic the old 'API is set to another host' warning gives way to the Capture/Replay line", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "connected", detail: null, discovery: discovery({ state: "connected", note: "Capture is from 10.30.0.211; Replay sends to 10.30.0.99." }) }));
    render(
      <LightkeyStrip
        connection={{ host: "10.30.0.99", onHost: vi.fn(), oscPort: "21600", onPort: vi.fn(), savedOscHost: "10.30.0.99", attempted: false, held: { host: false, port: false } }}
        lightingEnabled
      />,
    );
    expect(await screen.findByTestId("lightkey-discovery-note")).toBeInTheDocument();
    expect(screen.queryByTestId("lightkey-host-notice")).not.toBeInTheDocument();
  });

  it("a chosen address that differs from the Replay host keeps the old warning", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "connected", detail: null, discovery: discovery({ state: "connected", chosen: "10.30.0.211" }) }));
    render(
      <LightkeyStrip
        connection={{ host: "10.30.0.99", onHost: vi.fn(), oscPort: "21600", onPort: vi.fn(), savedOscHost: "10.30.0.99", attempted: false, held: { host: false, port: false } }}
        lightingEnabled
      />,
    );
    expect(await screen.findByTestId("lightkey-host-notice")).toBeInTheDocument();
  });

  it("an older backend without the block shows no search line", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status());
    render(<LightkeyStrip lightingEnabled />);
    await screen.findByTestId("lightkey-install");
    expect(screen.queryByTestId("lightkey-discovery")).not.toBeInTheDocument();
  });
});
