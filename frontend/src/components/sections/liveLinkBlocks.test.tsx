import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../../api";
import { libraryController } from "../../test/libraryFixtures";
import { playbackStatus } from "../../test/playbackFixtures";
import { helperStatus, linkController, localStatus, presentation } from "../../test/propresenterLinkFixtures";
import type { LightkeyStatus } from "../../types";
import { LightkeyStrip } from "../LightkeyStrip";
import { PlaybackLiveLink } from "../PlaybackLiveLink";
import { ProPresenterLinkBlock } from "../ProPresenterLinkBlock";
import { NO_SCAN } from "./scanState";

/**
 * The three Live Link blocks are ONE section type (`ConnectionSection`): whatever they show, the parts of the section come in the
 * same order. A change to that order is made in `sections/ConnectionSection.tsx` and these tests say it changed for all three.
 */
vi.mock("../../api", async (original) => ({
  ...(await original<typeof import("../../api")>()),
  getLightkeyStatus: vi.fn(),
  startLightkeyHelperInstall: vi.fn(),
  endLightkeyHelperInstall: vi.fn().mockResolvedValue({ ended: true }),
}));
beforeEach(() => {
  vi.mocked(api.getLightkeyStatus).mockResolvedValue({
    api_url: "http://192.0.2.10:8770", link: "connected", detail: null, active_count: 11, recording: false, recording_song: null, replay: true, replay_blocked: null, replaying_song: null, message: null,
  } as LightkeyStatus);
});
afterEach(() => vi.clearAllMocks());

const connection = { host: "192.0.2.10", onHost: vi.fn(), oscPort: "21600", onPort: vi.fn(), savedOscHost: "192.0.2.10", attempted: false, held: { host: false, port: false } };
const fields = { host: "192.168.1.60", port: "1025", onHost: vi.fn(), onPort: vi.fn(), held: { host: false, port: false }, errors: {}, busy: false, checksAgain: false, connectDisabled: false, connecting: false, onConnect: vi.fn(), scan: { state: NO_SCAN, start: vi.fn(), cancel: vi.fn(), choose: vi.fn() } };
/** The parts of the section in DOM order: the header, then every part the template names (boxes the quick actions open have no name). */
const parts = (section: HTMLElement) => Array.from(section.children).map((child, index) => (index === 0 ? "header" : child.getAttribute("data-part"))).filter(Boolean);
const follows = (a: Element, b: Element) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

describe("the Live Link blocks are all a ConnectionSection", () => {
  it("Presentation: title and pills, the connection row (Host, API, Scan, Connect), ONE status line, the Details button, then the details", () => {
    render(<ProPresenterLinkBlock connection={fields} link={linkController(helperStatus("ready"))} propresenter={presentation()} />);
    const section = screen.getByTestId("propresenter-live-link");
    expect(parts(section)).toEqual(["header", "connection", "status-row", "details"]);
    const heading = within(section).getByRole("heading", { name: "ProPresenter Live Link" });
    const row = screen.getByTestId("propresenter-live-link-line");
    const status = screen.getByTestId("propresenter-live-link-sub").querySelector("[data-part=status]") as HTMLElement;
    const button = screen.getByRole("button", { name: /Presenter Link setup/ });
    const details = screen.getByTestId("propresenter-live-link-details");
    const order = [heading, screen.getByTestId("propresenter-api-pill"), row, status, button, details];
    expect(order.every((node, index) => index === 0 || follows(order[index - 1] as Element, node))).toBe(true);
    // The row holds the address, the port and the pair of buttons; the status line is one line with both connections.
    expect(within(row).getByLabelText("Host")).toBeInTheDocument();
    expect(within(row).getByLabelText("ProPresenter API port")).toBeInTheDocument();
    expect(within(row).getAllByRole("button").filter((button) => button.tabIndex !== -1).map((button) => button.textContent)).toEqual(["Search", "Connect"]);
    expect(status).toHaveTextContent("ProPresenter API: Connected to ProPresenter at 192.168.1.60. \u00B7 Presenter Link: Ready, version 0.1.0.");
    // Collapsed: the setup box and the notes wait behind the button.
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(details).toHaveAttribute("hidden");
  });

  it("Presentation: with the Link not given there is no Presenter Link line, pill or Details button", () => {
    render(<ProPresenterLinkBlock connection={fields} propresenter={presentation()} />);
    expect(screen.queryByTestId("propresenter-link-pill")).toBeNull();
    expect(screen.queryByTestId("propresenter-link-status")).toBeNull();
    expect(screen.queryByRole("button", { name: /setup|Update the/ })).toBeNull();
    expect(screen.queryByTestId("propresenter-live-link-details")).toBeNull();
  });

  it("Presentation: this computer has a Details button too, holding the 'nothing to set up' line", async () => {
    render(<ProPresenterLinkBlock connection={fields} link={linkController(localStatus())} propresenter={presentation()} />);
    const button = screen.getByRole("button", { name: /Presenter Link setup/ });
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("propresenter-link-local")).not.toBeVisible();
    await userEvent.setup().click(button);
    expect(screen.getByTestId("propresenter-link-local")).toBeVisible();
  });

  it("Presentation: the details open by themselves when the Link needs setup, and stay closed when it only cannot import", () => {
    const { unmount } = render(<ProPresenterLinkBlock link={linkController(helperStatus("not_installed"))} propresenter={presentation()} />);
    expect(screen.getByRole("button", { name: /Presenter Link setup/ })).toHaveAttribute("aria-expanded", "true");
    unmount();
    render(<ProPresenterLinkBlock link={linkController(helperStatus("ready", { can_import: false, import_blocker: "Allow it in Accessibility." }))} propresenter={presentation()} />);
    expect(screen.getByRole("button", { name: /Presenter Link setup/ })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("propresenter-link-note")).toBeInTheDocument();
  });

  it("Playback: title and pills, the scan and Connect row, ONE status line, the Library Link setup button, then the details", () => {
    render(<PlaybackLiveLink library={libraryController("ready")} playback={{ status: playbackStatus({ reason: "Connected to Playback at 10.30.0.141.", host: "10.30.0.141", settings: { enabled: true, host: "10.30.0.141", port: 8080, auto_scan: true, fast_transport: true } }), events: [], error: null, message: null, pending: null, saveArrangement: vi.fn(), save: vi.fn(), selectSource: vi.fn(), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn() }} />);
    const section = screen.getByTestId("playback-live-link");
    expect(parts(section)).toEqual(["header", "connection", "status-row", "details"]);
    expect(within(screen.getByTestId("playback-live-link-line")).getAllByRole("button").filter((button) => button.tabIndex !== -1).map((button) => button.textContent)).toEqual(["Search", "Connect"]);
    expect(screen.getByTestId("live-link-sub").querySelector("[data-part=status]")).toHaveTextContent("Connected to Playback at 10.30.0.141. \u00B7 Library Link: Ready, version");
    expect(screen.getByRole("button", { name: /Library Link setup/ })).toHaveAttribute("aria-expanded", "false");
  });

  it("Lighting: title and pills, the Host/Port row, ONE status line with the quick actions and the Details button, then the details", async () => {
    render(<LightkeyStrip connection={connection} lightingEnabled testCue={<p>cue fields</p>} />);
    await screen.findByText("sACN: Connected");
    const section = screen.getByTestId("lightkey-strip");
    // The quick actions' boxes (test cue, address help) sit between the status row and the details, outside the collapsed area.
    expect(parts(section)).toEqual(["header", "connection", "status-row", "details"]);
    const order = [screen.getByTestId("live-link-sub"), screen.getByTestId("cue-address-help"), screen.getByTestId("test-cue-box"), screen.getByTestId("live-link-details")];
    expect(order.every((node, index) => index === 0 || follows(order[index - 1] as Element, node))).toBe(true);
    const actions = screen.getByTestId("live-link-actions");
    expect(within(actions).getAllByRole("button").map((button) => button.textContent?.replace(/[\u25B8\u25BE]/g, ""))).toEqual(["Release lights", "Read fixtures", "Send a test cue", "How to find a cue's address", "Lighting computer setup"]);
    expect(screen.getByRole("button", { name: /Lighting computer setup/ })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("test-cue-box")).not.toBeVisible();
    await userEvent.setup().click(screen.getByRole("button", { name: /Send a test cue/ }));
    await waitFor(() => expect(screen.getByTestId("test-cue-box")).toBeVisible());
    // Opening the test cue does not touch the Details.
    expect(screen.getByRole("button", { name: /Lighting computer setup/ })).toHaveAttribute("aria-expanded", "false");
  });

  it("Lighting: the details open by themselves while the helper is not set up", async () => {
    vi.mocked(api.getLightkeyStatus).mockResolvedValue({
      api_url: "http://192.0.2.10:8770", link: "unreachable", detail: "No answer.", active_count: null, recording: false, recording_song: null, replay: true, replay_blocked: null, replaying_song: null, message: null,
    } as LightkeyStatus);
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByText("sACN: Not connected");
    expect(screen.getByRole("button", { name: /Lighting computer setup/ })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "Set up the Lighting computer" })).toBeVisible();
  });
});

describe("the three rows end with the same two buttons", () => {
  const pairOf = (row: HTMLElement) => {
    const scan = row.querySelector<HTMLElement>("[data-part=scan]")!;
    const connect = row.querySelector<HTMLElement>("[data-part=connect]")!;
    return { scan, connect, labels: [scan.textContent, connect.textContent], follows: follows(scan, connect) };
  };

  it("Presentation, Playback and Lighting: Scan, then Connect, the same words, the same size, the same look", async () => {
    const playbackController = { status: playbackStatus({ reason: "Connected to Playback at 10.30.0.141.", host: "10.30.0.141", settings: { enabled: true, host: "10.30.0.141", port: 8080, auto_scan: true, fast_transport: true } }), events: [], error: null, message: null, pending: null, save: vi.fn(), saveArrangement: vi.fn(), selectSource: vi.fn(), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn() };
    const { unmount } = render(<ProPresenterLinkBlock connection={fields} propresenter={presentation()} />);
    const presentationPair = pairOf(screen.getByTestId("propresenter-live-link-line"));
    unmount();
    const playback = render(<PlaybackLiveLink playback={playbackController} />);
    const playbackPair = pairOf(screen.getByTestId("playback-live-link-line"));
    playback.unmount();
    render(<LightkeyStrip connection={connection} lightingEnabled />);
    await screen.findByText("sACN: Connected");
    const lightingPair = pairOf(screen.getByTestId("live-link-line"));
    for (const pair of [presentationPair, playbackPair, lightingPair]) {
      expect(pair.labels).toEqual(["Search", "Connect"]);
      expect(pair.follows).toBe(true);
      for (const button of [pair.scan, pair.connect]) {
        expect(button).toHaveClass("!h-10", "!min-h-10", "coarse:!h-11", "flex-none", "rounded-ds-lg");
        expect(button).toHaveAttribute("data-variant", "tinted");
      }
    }
    // No page-specific look: apart from the tone's own colours the three blocks draw the pair with the very same classes.
    const shared = (button: HTMLElement) => button.className.split(/\s+/).filter((name) => !/(lighting|playback|presentation)/.test(name)).sort().join(" ");
    for (const key of ["scan", "connect"] as const) {
      expect(shared(playbackPair[key])).toBe(shared(presentationPair[key]));
      expect(shared(lightingPair[key])).toBe(shared(presentationPair[key]));
      expect(lightingPair[key].className).not.toMatch(/!border|!bg|!text-ink/);
    }
    expect(presentationPair.scan).toHaveAttribute("data-tone", "presentation");
    expect(playbackPair.scan).toHaveAttribute("data-tone", "playback");
    expect(lightingPair.scan).toHaveAttribute("data-tone", "lighting");
  });
});
