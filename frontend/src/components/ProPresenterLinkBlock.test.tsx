import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { helperStatus, linkController, localStatus, noComputerStatus, presentation } from "../test/propresenterLinkFixtures";
import type { LinkStatus } from "../propresenterLink";
import { ProPresenterLinkBlock } from "./ProPresenterLinkBlock";

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
    expect(screen.getByTestId("propresenter-api-pill")).toHaveTextContent("ProPresenter: Connected");
    expect(screen.getByTestId("propresenter-link-pill")).toHaveTextContent("ProPresenter Link: Connected");
    expect(screen.getByTestId("propresenter-banner")).toHaveTextContent("ProPresenter: Connected to ProPresenter at 192.168.1.60.");
    expect(line()).toHaveTextContent("ProPresenter Link: Ready, version 0.1.0.");
  });

  it.each([
    ["Not installed", helperStatus("not_installed")],
    ["Update available, version 0.0.9", helperStatus("outdated")],
    ["Needs the command again", helperStatus("needs_command")],
    ["Not answering", helperStatus("not_answering")],
    ["Ready, version 0.1.0", helperStatus("ready")],
    ["Ready, on this computer", localStatus()],
    ["Can add songs, cannot import missing songs (macOS Accessibility not allowed), on this computer", localStatus({ can_import: false, import_blocker: "Allow StagePilot in System Settings > Privacy & Security > Accessibility, then try again." })],
    ["Can add songs, cannot import missing songs (macOS Accessibility not allowed)", helperStatus("ready", { can_import: false, import_blocker: "Allow the ProPresenter Link in System Settings > Privacy & Security > Accessibility, then try again." })],
  ])("says %s", (words, status) => {
    block(status);
    expect(line()).toHaveTextContent(`ProPresenter Link: ${words}.`);
  });

  it("says the state with a dot AND words: the line carries its tone and every state has words", () => {
    block(helperStatus("needs_command"));
    expect(line()).toHaveAttribute("data-tone", "warning");
    expect(line().textContent).toMatch(/[A-Za-z]/);
    expect(screen.getByTestId("propresenter-link-pill")).toHaveAttribute("data-tone", "warning");
  });

  it("the lines are one polite live region", () => {
    block(helperStatus("ready"));
    const region = line().closest("[aria-live]");
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveAttribute("role", "status");
  });

  it("reads the existing connection: connected, not connected, error", () => {
    const { unmount } = block(helperStatus("ready"), presentation({ connection_status: "disconnected" }));
    expect(screen.getByTestId("propresenter-banner")).toHaveTextContent("ProPresenter: Not connected to ProPresenter at 192.168.1.60.");
    unmount();
    block(helperStatus("ready"), presentation({ connection_status: "error" }));
    expect(screen.getByTestId("propresenter-banner")).toHaveTextContent("Cannot reach ProPresenter at 192.168.1.60");
  });
});

describe("ProPresenterLinkBlock: this computer or another one", () => {
  it("this computer: no setup button, says so, and shows the blocker with its fix", () => {
    block(localStatus({ can_import: false, import_blocker: "Allow StagePilot in System Settings > Privacy & Security > Accessibility, then try again." }));
    expect(screen.queryByRole("button", { name: /Set up the ProPresenter computer|Update the ProPresenter Link/ })).toBeNull();
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
    const button = disclosure(/Set up the ProPresenter computer/);
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("propresenter-install")).toBeVisible();
    expect(screen.queryByTestId("propresenter-link-local")).toBeNull();
    await userEvent.setup().click(button);
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(button).toHaveAttribute("aria-controls");
  });

  it("another computer, ready: the setup box is closed (the button is still there for another computer)", () => {
    block(helperStatus("ready"));
    expect(disclosure(/Set up the ProPresenter computer/)).toHaveAttribute("aria-expanded", "false");
  });

  it("an outdated Link: the button is Update the ProPresenter Link; one that StagePilot updates by itself needs no command and says so", () => {
    const { unmount } = block(helperStatus("outdated"));
    expect(disclosure(/Update the ProPresenter Link/)).toBeInTheDocument();
    unmount();
    block(helperStatus("outdated", { update: { mode: "automatic", version: "0.1.0", message: "StagePilot updates the ProPresenter Link by itself when nothing is playing." } }));
    expect(screen.getByTestId("propresenter-link-update")).toHaveTextContent("StagePilot updates the ProPresenter Link by itself when nothing is playing.");
    expect(disclosure(/Set up the ProPresenter computer/)).toHaveAttribute("aria-expanded", "false");
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
    expect(line()).toHaveTextContent("ProPresenter Link: Not installed.");
    expect(line()).toHaveAttribute("data-tone", "idle");
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("before the first answer it says Checking, still no alert and no button", () => {
    block(null);
    expect(line()).toHaveTextContent("ProPresenter Link: Checking…");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("a Link that is not installed is not an alert either", () => {
    block(helperStatus("not_installed"));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(line()).toHaveAttribute("data-tone", "idle");
  });

  it("a read that failed shows its own sentence as an alert, and nothing else changes", () => {
    render(<ProPresenterLinkBlock link={linkController(null, { error: "The ProPresenter Link could not be read." })} propresenter={presentation()} />);
    expect(screen.getByRole("alert")).toHaveTextContent("The ProPresenter Link could not be read.");
  });
});
