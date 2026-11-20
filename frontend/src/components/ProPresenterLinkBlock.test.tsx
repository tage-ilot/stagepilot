import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { helperStatus, linkController, localStatus, noComputerStatus, presentation } from "../test/propresenterLinkFixtures";
import type { LinkStatus } from "../propresenterLink";
import { ProPresenterLinkBlock, type ProPresenterConnectionFields } from "./ProPresenterLinkBlock";
import { NO_SCAN, type ScanState } from "./sections";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  startProPresenterHelperInstall: vi.fn(),
  endProPresenterHelperInstall: vi.fn().mockResolvedValue({ ended: true }),
}));
afterEach(() => vi.clearAllMocks());

const block = (status: LinkStatus | null, over = presentation()) => render(<ProPresenterLinkBlock link={linkController(status)} propresenter={over} />);
const line = () => screen.getByTestId("propresenter-link-status");
/** The button in the strip that opens or closes the setup box (the box has a button of the same words inside). */
const disclosure = (name: RegExp) => screen.getAllByRole("button", { name }).find((button) => button.hasAttribute("aria-expanded")) as HTMLElement;

describe("ProPresenterLinkBlock: the two lines", () => {
  it("is a titled block with a pill for each line, like the Playback Live Link", () => {
    block(helperStatus("ready"));
    const region = screen.getByRole("region", { name: "ProPresenter Live Link" });
    expect(within(region).getByRole("heading", { name: "ProPresenter Live Link" })).toBeInTheDocument();
    expect(screen.getByTestId("propresenter-api-pill")).toHaveTextContent("ProPresenter API: Connected");
    expect(screen.getByTestId("propresenter-link-pill")).toHaveTextContent("Presenter Link: Connected");
    expect(screen.getByTestId("propresenter-banner")).toHaveTextContent("ProPresenter API: Connected to ProPresenter at 192.168.1.60.");
    expect(line()).toHaveTextContent("Presenter Link: Ready, version 0.1.0.");
  });

  it.each([
    ["Not installed", helperStatus("not_installed")],
    ["Update available, version 0.0.9", helperStatus("outdated")],
    ["Needs the command again", helperStatus("needs_command")],
    ["Not answering", helperStatus("not_answering")],
    ["Ready, version 0.1.0", helperStatus("ready")],
    ["Waiting for macOS", localStatus({ state: "waiting", can_import: false, message: "macOS may be asking for permission on this computer. Answer the prompt, then press Connect again." })],
    ["Ready, on this computer", localStatus()],
    ["Can add songs, cannot import missing songs (macOS Accessibility not allowed), on this computer", localStatus({ can_import: false, import_blocker: "Allow StagePilot in System Settings > Privacy & Security > Accessibility, then try again." })],
    ["Can add songs, cannot import missing songs (macOS Accessibility not allowed)", helperStatus("ready", { can_import: false, import_blocker: "Allow the Presenter Link in System Settings > Privacy & Security > Accessibility, then try again." })],
  ])("says %s", (words, status) => {
    block(status);
    expect(line()).toHaveTextContent(`Presenter Link: ${words}.`);
  });

  it("says the state with a dot AND words: the line carries its tone and every state has words", () => {
    block(helperStatus("needs_command"));
    expect(line()).toHaveAttribute("data-tone", "warning");
    expect(line().textContent).toMatch(/[A-Za-z]/);
    expect(screen.getByTestId("propresenter-link-pill")).toHaveAttribute("data-tone", "warning");
  });

  it("while macOS asks for permission the pill says Waiting for macOS and the Details open with the one sentence", () => {
    const message = "macOS may be asking for permission on this computer. Answer the prompt, then press Connect again.";
    block(localStatus({ state: "waiting", can_import: false, libraries: [], message }));
    expect(screen.getByTestId("propresenter-link-pill")).toHaveTextContent("Presenter Link: Waiting for macOS");
    expect(screen.getByTestId("propresenter-link-pill")).toHaveAttribute("data-tone", "info");
    expect(screen.getByTestId("propresenter-link-note")).toHaveTextContent(message);
    expect(screen.queryByTestId("propresenter-install")).toBeNull();
  });

  it("the lines are one polite live region", () => {
    block(helperStatus("ready"));
    const region = line().closest("[aria-live]");
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveAttribute("role", "status");
  });

  it("reads the existing connection: connected, not connected, error", () => {
    const { unmount } = block(helperStatus("ready"), presentation({ connection_status: "disconnected" }));
    expect(screen.getByTestId("propresenter-banner")).toHaveTextContent("ProPresenter API: Not connected to ProPresenter at 192.168.1.60.");
    unmount();
    block(helperStatus("ready"), presentation({ connection_status: "error" }));
    expect(screen.getByTestId("propresenter-banner")).toHaveTextContent("Cannot reach ProPresenter at 192.168.1.60");
  });
});

describe("ProPresenterLinkBlock: this computer or another one", () => {
  it("this computer: no setup button, says so, and shows the blocker with its fix", () => {
    block(localStatus({ can_import: false, import_blocker: "Allow StagePilot in System Settings > Privacy & Security > Accessibility, then try again." }));
    expect(screen.queryByRole("button", { name: /Set up the ProPresenter computer|Update the Presenter Link/ })).toBeNull();
    expect(screen.queryByTestId("propresenter-install")).toBeNull();
    expect(screen.getByTestId("propresenter-link-local")).toHaveTextContent("ProPresenter runs on this computer, so there is nothing to set up.");
    expect(screen.getByTestId("propresenter-link-note")).toHaveTextContent(/cannot load a missing song.*Allow StagePilot in System Settings > Privacy & Security > Accessibility, then try again\./);
  });

  it("this computer, nothing blocked: no note at all", () => {
    block(localStatus());
    expect(screen.queryByTestId("propresenter-link-note")).toBeNull();
    expect(screen.getByTestId("propresenter-link-local")).toBeInTheDocument();
  });

  it("another computer: the button Set up the ProPresenter computer opens the box (open by default while not set up)", async () => {
    block(helperStatus("not_installed"));
    const button = disclosure(/Presenter Link setup/);
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("propresenter-install")).toBeVisible();
    expect(screen.queryByTestId("propresenter-link-local")).toBeNull();
    await userEvent.setup().click(button);
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(button).toHaveAttribute("aria-controls");
  });

  it("another computer, ready: the setup box is closed (the button is still there for another computer)", () => {
    block(helperStatus("ready"));
    expect(disclosure(/Presenter Link setup/)).toHaveAttribute("aria-expanded", "false");
  });

  it("an outdated Link: the button is Update the Presenter Link; one that StagePilot updates by itself needs no command and says so", () => {
    const { unmount } = block(helperStatus("outdated"));
    expect(disclosure(/Update the Presenter Link/)).toBeInTheDocument();
    unmount();
    block(helperStatus("outdated", { update: { mode: "automatic", version: "0.1.0", message: "StagePilot updates the Presenter Link by itself when nothing is playing." } }));
    expect(screen.getByTestId("propresenter-link-update")).toHaveTextContent("StagePilot updates the Presenter Link by itself when nothing is playing.");
    expect(disclosure(/Presenter Link setup/)).toHaveAttribute("aria-expanded", "false");
  });

  it("the box makes the command only when its button is pressed", async () => {
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue({ command: "curl -fsS http://192.168.1.20:8000/propresenter-helper/abc/install.sh | sh", url: "x", expires_at: new Date(Date.now() + 1_800_000).toISOString(), lan_ip: "192.168.1.20", port: 8000, bundled_version: "0.1.0" });
    block(helperStatus("not_installed"));
    expect(api.startProPresenterHelperInstall).not.toHaveBeenCalled();
    await userEvent.setup().click(within(screen.getByTestId("propresenter-install")).getByRole("button", { name: "Set up the ProPresenter computer" }));
    expect(await screen.findByTestId("propresenter-install-command")).toBeInTheDocument();
  });

  it("an answer that is only a memory is said in a sentence", () => {
    block(helperStatus("ready", { stale: true, message: "The ProPresenter computer did not answer just now; this is what it said a moment ago." }));
    expect(screen.getByTestId("propresenter-link-note")).toHaveTextContent("this is what it said a moment ago");
  });
});

describe("ProPresenterLinkBlock: optional, never a warning", () => {
  it("no ProPresenter computer saved: grey 'Not installed', no setup button, no alert", () => {
    block(noComputerStatus(), presentation({ enabled: false }));
    expect(line()).toHaveTextContent("Presenter Link: Not installed.");
    expect(line()).toHaveAttribute("data-tone", "idle");
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("before the first answer it says Checking, still no alert and no button", () => {
    block(null);
    expect(line()).toHaveTextContent("Presenter Link: Checking…");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("a Link that is not installed is not an alert either", () => {
    block(helperStatus("not_installed"));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(line()).toHaveAttribute("data-tone", "idle");
  });

  it("a read that failed shows its own sentence as an alert, and nothing else changes", () => {
    render(<ProPresenterLinkBlock link={linkController(null, { error: "The Presenter Link could not be read." })} propresenter={presentation()} />);
    expect(screen.getByRole("alert")).toHaveTextContent("The Presenter Link could not be read.");
  });
});

describe("ProPresenterLinkBlock: the connection it holds", () => {
  const fields = (over: Partial<ProPresenterConnectionFields> = {}): ProPresenterConnectionFields => ({
    host: "192.168.1.60", port: "1025", onHost: vi.fn(), onPort: vi.fn(), held: { host: false, port: false }, errors: {}, busy: false, checksAgain: false, connectDisabled: false, connecting: false, onConnect: vi.fn(),
    scan: { state: NO_SCAN, start: vi.fn(), cancel: vi.fn(), choose: vi.fn() }, ...over,
  });
  const withFields = (connection: ProPresenterConnectionFields, status: LinkStatus | null = helperStatus("ready")) => render(<ProPresenterLinkBlock connection={connection} link={linkController(status)} propresenter={presentation()} />);

  it("draws Host, API, Scan and Connect above the status line and the setup box; without `connection` it draws none of them", () => {
    withFields(fields(), helperStatus("not_installed"));
    const row = screen.getByTestId("propresenter-live-link-line");
    // The connection row is on top (DOM order changed on purpose: it used to sit under the status lines).
    expect(row.compareDocumentPosition(screen.getByTestId("propresenter-live-link-sub")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(row.compareDocumentPosition(screen.getByTestId("propresenter-install")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(row).getByLabelText("Host")).toHaveValue("192.168.1.60");
    expect(within(row).getByLabelText("ProPresenter API port")).toHaveValue("1025");
    expect(within(row).getByRole("button", { name: "Search" })).toBeEnabled();
    expect(within(row).getByRole("button", { name: "Connect" })).toBeEnabled();
  });

  it("without `connection` the block is only the lines", () => {
    block(helperStatus("ready"));
    expect(screen.queryByTestId("propresenter-live-link-line")).toBeNull();
    expect(screen.queryByLabelText("Host")).toBeNull();
  });

  it("typing and Connect are reported to the panel, nothing is saved here", async () => {
    const connection = fields();
    withFields(connection);
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Host"), "5");
    await user.type(screen.getByLabelText("ProPresenter API port"), "6");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect(connection.onHost).toHaveBeenCalledWith("192.168.1.605");
    expect(connection.onPort).toHaveBeenCalledWith("10256");
    expect(connection.onConnect).toHaveBeenCalledTimes(1);
  });

  it("a held change says so in words next to the caption, with the warning border, and the box points at the words", () => {
    withFields(fields({ held: { host: true, port: false } }));
    const marker = document.querySelector("[data-held-marker]")!;
    expect(marker).toHaveTextContent("Not saved yet");
    expect(screen.getByLabelText("Host")).toHaveClass("!border-warning");
    expect(screen.getByLabelText("Host")).toHaveAttribute("aria-describedby", marker.id);
    expect(screen.getByLabelText("ProPresenter API port")).not.toHaveClass("!border-warning");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(1);
  });

  it("an error is an alert under the row, wired to its box", () => {
    withFields(fields({ errors: { host: "Enter a host." } }));
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Enter a host.");
    expect(screen.getByLabelText("Host")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Host")).toHaveAttribute("aria-describedby", alert.id);
  });

  it("a request in flight waits the boxes, the recent addresses and the buttons; Connect says Connecting…", () => {
    withFields(fields({ busy: true, connectDisabled: true, connecting: true }));
    expect(screen.getByLabelText("Host")).toBeDisabled();
    expect(screen.getByLabelText("ProPresenter API port")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Connecting…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Search" })).toBeDisabled();
  });
});

const found = (hosts: { label: string; host: string; port: number; hint?: string }[]): ScanState => ({ state: "found", candidates: hosts, current: 255, total: 255 });

describe("ProPresenterLinkBlock: the caption of the port box is API", () => {
  it("reads API, with the words of ProPresenter's network API as its hint, and keeps the accessible name specific", () => {
    render(<ProPresenterLinkBlock connection={{ host: "192.168.1.60", port: "1025", onHost: vi.fn(), onPort: vi.fn(), held: { host: false, port: false }, errors: {}, busy: false, checksAgain: false, connectDisabled: false, connecting: false, onConnect: vi.fn(), scan: { state: NO_SCAN, start: vi.fn(), cancel: vi.fn(), choose: vi.fn() } }} link={linkController(helperStatus("ready"))} propresenter={presentation()} />);
    const box = screen.getByLabelText("ProPresenter API port");
    expect(box).toHaveAttribute("title", "The port of ProPresenter's network API");
    const caption = box.closest("label")!;
    expect(caption.childNodes[0]?.textContent).toBe("API");
    expect(screen.queryByText("Port")).toBeNull();
  });
});

describe("ProPresenterLinkBlock: Scan and Connect, the pair every Live Link block ends its row with", () => {
  const fields = (over: Partial<ProPresenterConnectionFields> = {}): ProPresenterConnectionFields => ({
    host: "192.168.1.60", port: "1025", onHost: vi.fn(), onPort: vi.fn(), held: { host: false, port: false }, errors: {}, busy: false, checksAgain: false, connectDisabled: false, connecting: false, onConnect: vi.fn(),
    scan: { state: NO_SCAN, start: vi.fn(), cancel: vi.fn(), choose: vi.fn() }, ...over,
  });
  const draw = (connection: ProPresenterConnectionFields) => render(<ProPresenterLinkBlock connection={connection} link={linkController(helperStatus("ready"))} propresenter={presentation()} />);

  it("Scan starts the scan, then reads Scanning… (cancel) with the progress line, and cancel stops it", async () => {
    const connection = fields();
    const view = draw(connection);
    await userEvent.setup().click(screen.getByRole("button", { name: "Search" }));
    expect(connection.scan.start).toHaveBeenCalledTimes(1);
    view.rerender(<ProPresenterLinkBlock connection={{ ...connection, scan: { ...connection.scan, state: { state: "scanning", candidates: [], current: 64, total: 255, phase: "Looking for ProPresenter…", elapsed: 3 } } }} link={linkController(helperStatus("ready"))} propresenter={presentation()} />);
    expect(screen.getByTestId("scan-progress")).toHaveTextContent("Searching the network for ProPresenter… — Checked 64 of 255 addresses (3s)");
    await userEvent.setup().click(screen.getByRole("button", { name: "Searching… (cancel)" }));
    expect(connection.scan.cancel).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
  });

  it("one computer found: a line says so and tells to press Connect; no chooser", () => {
    draw(fields({ scan: { state: found([{ label: "Front MacBook", host: "192.168.1.70", port: 1025 }]), start: vi.fn(), cancel: vi.fn(), choose: vi.fn() } }));
    expect(screen.getByTestId("propresenter-scan-found")).toHaveTextContent("Found Front MacBook at 192.168.1.70:1025. It is in the Host and API boxes: press Connect to use it.");
    expect(screen.queryByRole("group", { name: /Choose one/ })).toBeNull();
  });

  it("several found: the same chooser Playback uses, and choosing one hands the host and the API port to the panel", async () => {
    const connection = fields({ scan: { state: found([{ label: "Front MacBook", host: "192.168.1.70", port: 1025, hint: "192.168.1.70:1025 · ProPresenter 19" }, { label: "Stage Mac", host: "192.168.1.71", port: 11025 }]), start: vi.fn(), cancel: vi.fn(), choose: vi.fn() } });
    draw(connection);
    await userEvent.setup().click(within(screen.getByRole("group", { name: "Found 2 ProPresenter computers. Choose one:" })).getByRole("button", { name: /Stage Mac/ }));
    expect(connection.scan.choose).toHaveBeenCalledWith("192.168.1.71", 11025);
    expect(connection.onConnect).not.toHaveBeenCalled();
  });

  it("found nothing: the checklist stays in view; macOS blocking says how to fix it", () => {
    const { unmount } = draw(fields({ scan: { state: { state: "not_found", candidates: [], current: 255, total: 255, reason: "No ProPresenter answered on this network.", hostsProbed: 255, hostsTotal: 255, networks: ["192.168.1.0/24"], elapsed: 12 }, start: vi.fn(), cancel: vi.fn(), choose: vi.fn() } }));
    expect(screen.getByText("Make sure:")).toBeVisible();
    expect(screen.getByText("ProPresenter is open on a computer")).toBeVisible();
    expect(screen.getByText(/Checked 255 of 255 addresses on 1 network \(192.168.1.0\/24\)/)).toBeVisible();
    unmount();
    draw(fields({ scan: { state: { state: "not_found", candidates: [], current: 0, total: 0, error: "permission_denied", reason: "macOS is blocking", details: "{}", settingsUrl: "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork" }, start: vi.fn(), cancel: vi.fn(), choose: vi.fn() } }));
    expect(screen.getByTestId("scan-error")).toHaveTextContent("macOS is blocking StagePilot's local-network access");
    expect(screen.getByRole("button", { name: "Open Local Network settings" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Copy diagnostic details" })).toBeVisible();
  });

  it("Connect on a connection that only needs checking says so in the status line; Connect with something to save does not", async () => {
    const checking = fields({ checksAgain: true });
    draw(checking);
    await userEvent.setup().click(screen.getByRole("button", { name: "Connect" }));
    expect(checking.onConnect).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("propresenter-recheck")).toHaveTextContent("Checked again.");
    expect(screen.getByTestId("propresenter-live-link-sub")).toContainElement(screen.getByTestId("propresenter-recheck"));
    document.body.innerHTML = "";
    draw(fields({ checksAgain: false }));
    await userEvent.setup().click(screen.getByRole("button", { name: "Connect" }));
    expect(screen.queryByTestId("propresenter-recheck")).toBeNull();
  });

  it("Connect says why it is off", () => {
    draw(fields({ connectDisabled: true, connectHint: "Waiting for the saved settings to load." }));
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Connect" }).closest("[data-part=connect-wrap]")).toHaveAttribute("title", "Waiting for the saved settings to load.");
  });
});
