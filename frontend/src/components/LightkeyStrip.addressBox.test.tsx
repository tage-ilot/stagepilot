import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { rememberAddress } from "../hooks/useRecentAddresses";
import type { LightkeyStatus } from "../types";
import { HOST_LABEL } from "./lightingHost";
import { LightkeyStrip } from "./LightkeyStrip";
import { OscConnectionFields } from "./LightingOscParts";

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
const respond = (lightkey: LightkeyStatus) => ({ accepted: true, message: "ok", lightkey });

/** The panel's draft host, as the real panel holds it. */
function Strip({ host = "10.30.0.211", held = false, attempted = false }: { host?: string; held?: boolean; attempted?: boolean }) {
  const [value, setValue] = useState(host);
  return <LightkeyStrip connection={{ host: value, onHost: setValue, oscPort: "21600", onPort: vi.fn(), savedOscHost: "10.30.0.211", attempted, held: { host: held, port: false } }} lightingEnabled />;
}
const answered = () => screen.findByRole("radiogroup", { name: "Mode" });
const box = () => screen.getByRole("combobox", { name: HOST_LABEL });
const arrow = () => screen.getByRole("button", { name: "Show recent and found addresses" });

beforeEach(() => { window.localStorage.clear(); vi.mocked(api.getLightkeyStatus).mockResolvedValue(status()); });
afterEach(() => { vi.clearAllMocks(); });

describe("Lighting Live Link: the Host address box", () => {
  it("is the one address box (a text box with the arrow inside it), under the caption Host, with no chip row under the row", async () => {
    rememberAddress("lighting", "10.30.0.50");
    render(<Strip />);
    await answered();
    expect(box().tagName).toBe("INPUT");
    expect(box().closest("label")!.childNodes[0]?.textContent).toBe("Host");
    expect(screen.getByTestId("live-link-line")).toContainElement(arrow());
    expect(screen.queryByRole("group", { name: "Recent addresses" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Use / })).toBeNull();
  });

  it("the arrow lists the recent addresses; picking one fills Host and connects nothing", async () => {
    rememberAddress("lighting", "10.30.0.50");
    const user = userEvent.setup();
    render(<Strip />);
    await answered();
    await user.click(arrow());
    expect(within(screen.getByRole("group", { name: "Recent" })).getAllByRole("option").map((o) => o.textContent?.replace("(in the box now)", ""))).toEqual(["10.30.0.211", "10.30.0.50"]);
    await user.click(screen.getByRole("option", { name: "10.30.0.50" }));
    expect(box()).toHaveValue("10.30.0.50");
    expect(api.saveLightkeySettings).not.toHaveBeenCalled();
  });

  it("computers the search found are under Scanned; one can be chosen, then edited, and Connect uses what the box holds", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue(status({ link: "unreachable", detail: "No answer.", discovery: { state: "choose", found: [{ address: "10.30.0.141", lightkey_running: true }, { address: "10.30.0.152", lightkey_running: false }], refusing: [] } }));
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status()));
    const user = userEvent.setup();
    render(<Strip />);
    await answered();
    await user.click(arrow());
    expect(within(screen.getByRole("group", { name: "Found" })).getAllByRole("option").map((o) => o.textContent)).toEqual(["10.30.0.141", "10.30.0.152"]);
    await user.click(screen.getByRole("option", { name: "10.30.0.141" }));
    await user.type(box(), "5");
    expect(box()).toHaveValue("10.30.0.1415");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledWith({ api_url: "10.30.0.1415:8770" }));
  });

  it("Enter in the box with the list closed is Connect, as before; with the list open it chooses and does not connect", async () => {
    rememberAddress("lighting", "10.30.0.50");
    vi.mocked(api.saveLightkeySettings).mockResolvedValue(respond(status()));
    const user = userEvent.setup();
    render(<Strip host="10.30.0.99" />);
    await answered();
    await user.click(box());
    // The connected computer is remembered first, then the older one.
    await user.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(box()).toHaveValue("10.30.0.50");
    expect(api.saveLightkeySettings).not.toHaveBeenCalled();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(api.saveLightkeySettings).toHaveBeenCalledTimes(1));
  });

  it("the address that is connected is remembered, so it is in the list afterwards", async () => {
    render(<Strip />);
    await answered();
    expect(JSON.parse(window.localStorage.getItem("stagepilot.recent-addresses.lighting") ?? "[]")).toEqual(["10.30.0.211"]);
  });

  it("a held change keeps the warning border; an invalid host is marked invalid", async () => {
    const view = render(<Strip held />);
    await answered();
    expect(box()).toHaveClass("!border-warning");
    view.unmount();
    render(<Strip attempted host="bad host" />);
    await answered();
    expect(box()).toBeInvalid();
  });
});

describe("Lighting Live Link before Lighting has answered: the same box", () => {
  it("is the same combobox with its arrow, and its list holds the recent ones", async () => {
    rememberAddress("lighting", "10.30.0.50");
    const user = userEvent.setup();
    render(<OscConnectionFields attempted={false} held={{ host: false, port: false }} host="" onHost={vi.fn()} onPort={vi.fn()} port="21600" />);
    expect(box()).toBeInTheDocument();
    await user.click(arrow());
    expect(screen.getByRole("option", { name: "10.30.0.50" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Recent addresses" })).toBeNull();
  });
});
