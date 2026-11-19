import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { LightkeyStatus } from "../types";
import { HOST_HELP, HOST_LABEL, HOST_TITLE } from "./lightingHost";
import { LightkeyStrip } from "./LightkeyStrip";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getLightkeyStatus: vi.fn(),
  saveLightkeySettings: vi.fn(),
  searchLightkeyAgain: vi.fn(),
  setLightkeyAudioMeter: vi.fn().mockResolvedValue({ available: false, reason: "No input.", devices: [], selected: null, note: null, level: null, metering: false }),
}));

const status = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://10.30.0.211:8770", link: "connected", detail: null, active_count: 11, recording: false, recording_song: null, replay: true, replay_blocked: null, replaying_song: null, message: null, ...over,
});
const respond = (lightkey: LightkeyStatus, accepted = true) => ({ accepted, message: "ok", lightkey });
const connection = { host: "10.30.0.211", onHost: vi.fn(), oscPort: "21600", onPort: vi.fn(), savedOscHost: "10.30.0.211", attempted: false, held: { host: false, port: false } };

beforeEach(() => { vi.mocked(api.getLightkeyStatus).mockResolvedValue(status()); });
afterEach(() => { vi.clearAllMocks(); });

describe("Lighting Live Link: the Host caption", () => {
  it("says Host, keeps the long words as the box's hint, and keeps a specific accessible name", async () => {
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    const box = await screen.findByLabelText(HOST_LABEL);
    expect(HOST_LABEL).toBe("Lighting host or IP address");
    expect(box).toHaveAttribute("title", "Host or IP (OSC output + Live Link)");
    expect(HOST_TITLE).toBe("Host or IP (OSC output + Live Link)");
    const caption = box.closest("label")!;
    expect(caption.childNodes[0]?.textContent).toBe("Host");
    expect(screen.getByTestId("live-link-line")).not.toHaveTextContent(/Host or IP \(OSC output/);
  });

  it("says the same words in the Details, so the information is not lost", async () => {
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByLabelText(HOST_LABEL);
    expect(within(screen.getByTestId("live-link-details")).getByTestId("lightkey-host-help")).toHaveTextContent(HOST_HELP);
    expect(HOST_HELP).toContain("OSC output + Live Link");
  });

  it("is Host before Lighting has answered, too (the same row without a live link)", async () => {
    vi.mocked(api.getLightkeyStatus).mockRejectedValue(new Error("down"));
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    const box = await screen.findByLabelText(HOST_LABEL);
    expect(box.closest("label")!.childNodes[0]?.textContent).toBe("Host");
    expect(box).toHaveAttribute("title", HOST_TITLE);
    // Both buttons are there, switched off, and say why.
    const row = screen.getByTestId("live-link-line");
    expect(within(row).getByRole("button", { name: "Search" })).toBeDisabled();
    expect(within(row).getByRole("button", { name: "Connect" })).toBeDisabled();
    expect(within(row).getByRole("button", { name: "Connect" }).closest("[data-part=connect-wrap]")).toHaveAttribute("title", expect.stringContaining("has not answered yet"));
  });
});

/** The strip draws the same row, buttons switched off, until Lighting has answered: wait for the real one (the Mode toggle is part of it). */
const answered = () => screen.findByRole("radiogroup", { name: "Mode" });

describe("Lighting Live Link: Scan and Connect", () => {
  it("the row ends with Scan, Connect, then Mode, and no button is called Check again or Search again", async () => {
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await answered();
    const row = screen.getByTestId("live-link-line");
    const order = Array.from(row.querySelectorAll("[data-part=scan], [data-part=connect], [role=radiogroup]")).map((node) => node.getAttribute("data-part") ?? node.getAttribute("aria-label"));
    expect(order).toEqual(["scan", "connect", "Mode"]);
    expect(screen.queryByRole("button", { name: /Check again|Search again|Scan again|Test connection|Scan Network/ })).toBeNull();
  });

  it("Scan starts the existing search now, and is switched off only while Lighting is off", async () => {
    vi.mocked(api.searchLightkeyAgain).mockResolvedValue(respond(status({ link: "unreachable", discovery: { state: "searching", found: [], refusing: [] } })));
    const user = userEvent.setup();
    const { unmount } = render(<LightkeyStrip connection={connection} lightingEnabled />);
    await answered();
    await user.click(screen.getByRole("button", { name: "Search" }));
    await waitFor(() => expect(api.searchLightkeyAgain).toHaveBeenCalledTimes(1));
    unmount();
    render(<LightkeyStrip connection={connection} lightingEnabled={false} />);
    await answered();
    expect(screen.getByRole("button", { name: "Search" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Search" }).closest("[data-part=scan-wrap]")).toHaveAttribute("title", "Lighting is turned off. Turn it on first.");
  });

  it("Connect sends what it sent before: the host and the API port, in the same request", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "unreachable", detail: "No answer." }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status()));
    const user = userEvent.setup();
    render(<LightkeyStrip connection={{ ...connection, host: "10.30.0.99", savedOscHost: "10.30.0.211" }} lightingEnabled />);
    await answered();
    await user.clear(screen.getByLabelText("API port"));
    await user.type(screen.getByLabelText("API port"), "9000");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ api_url: "10.30.0.99:9000" }));
  });

  it("Connect on a block that is already connected, with nothing changed, only checks again, and the status line says so", async () => {
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status()));
    const user = userEvent.setup();
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await answered();
    await user.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledTimes(1));
    expect(await screen.findByTestId("lightkey-headline")).toHaveTextContent("Checked again.");
    // The same words go away with the next thing the person does.
    vi.mocked(api.searchLightkeyAgain).mockResolvedValue(respond(status(), false));
    await user.click(screen.getByRole("button", { name: "Search" }));
    await waitFor(() => expect(screen.getByTestId("lightkey-headline")).not.toHaveTextContent("Checked again."));
  });
});
