import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import { helperStatus, linkController } from "../test/propresenterLinkFixtures";
import type { LinkState } from "../propresenterLink";
import type { PlaybackHelperInstall } from "../types";
import { ProPresenterInstallBox } from "./ProPresenterInstallBox";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  startProPresenterHelperInstall: vi.fn(),
  endProPresenterHelperInstall: vi.fn(),
}));

const made = (minutes = 30): PlaybackHelperInstall => ({
  command: "curl -fsS http://192.168.1.20:8000/propresenter-helper/abc123/install.sh | sh",
  url: "http://192.168.1.20:8000/propresenter-helper/abc123/install.sh",
  expires_at: new Date(Date.now() + minutes * 60_000).toISOString(),
  lan_ip: "192.168.1.20",
  port: 8000,
  bundled_version: "0.1.0",
});
const box = (link: ProPresenterLinkController, hasComputer = true, autoStart = false) => <ProPresenterInstallBox autoStart={autoStart} hasComputer={hasComputer} link={link} />;

beforeEach(() => {
  vi.mocked(api.endProPresenterHelperInstall).mockResolvedValue({ ended: true });
});
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); });

describe("ProPresenterInstallBox", () => {
  it("offers the setup button while the Link is not ready, and says it runs on the ProPresenter computer without internet", () => {
    render(box(linkController(helperStatus("not_installed"))));
    expect(screen.getByRole("region", { name: "Set up the ProPresenter computer" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Set up the ProPresenter computer" })).toBeEnabled();
    expect(screen.getByTestId("propresenter-install")).toHaveTextContent(/Terminal on the ProPresenter computer/);
    expect(screen.getByTestId("propresenter-install")).toHaveTextContent(/no internet/);
    expect(api.startProPresenterHelperInstall).not.toHaveBeenCalled();
  });

  it.each([
    ["not_installed", /optional/],
    ["needs_command", /does not know this StagePilot/],
    ["not_answering", /cannot reach the ProPresenter Link/],
    ["outdated", /older than the one StagePilot carries/],
  ] as [LinkState, RegExp][])("says why in plain words for a %s Link", (state, words) => {
    render(box(linkController(helperStatus(state))));
    expect(screen.getByTestId("propresenter-install-why")).toHaveTextContent(words);
  });

  it("is called Update the ProPresenter Link when the Link is outdated", () => {
    render(box(linkController(helperStatus("outdated"))));
    expect(screen.getByRole("button", { name: "Update the ProPresenter Link" })).toBeEnabled();
  });

  it("offers no command for an older Link that StagePilot updates by itself, and offers it when StagePilot cannot", () => {
    for (const mode of ["automatic", "updating"] as const) {
      const { unmount } = render(box(linkController(helperStatus("outdated", { update: { mode, version: "0.1.0", message: "StagePilot updates it." } }))));
      expect(screen.queryByTestId("propresenter-install")).toBeNull();
      unmount();
    }
    render(box(linkController(helperStatus("outdated", { update: { mode: "manual", version: "0.1.0", message: "The ProPresenter Link could not update itself. Run the setup command on the ProPresenter computer to update it." } }))));
    expect(screen.getByRole("button", { name: "Update the ProPresenter Link" })).toBeEnabled();
    expect(screen.getByTestId("propresenter-install-why")).toHaveTextContent(/could not update itself.*older than the one StagePilot carries/);
  });

  it("shows nothing while it is ready, or has not answered yet, or is being looked at", () => {
    for (const link of [linkController(helperStatus("ready")), linkController(null), linkController(helperStatus("checking"))]) {
      const { unmount } = render(box(link));
      expect(screen.queryByTestId("propresenter-install")).toBeNull();
      unmount();
    }
  });

  it("says what to save first when no ProPresenter computer is saved", () => {
    render(box(linkController(helperStatus("not_installed")), false));
    expect(screen.getByTestId("propresenter-install-need-computer")).toHaveTextContent(/Save the Presentation connection above/);
    expect(screen.queryByRole("button", { name: "Set up the ProPresenter computer" })).toBeNull();
  });

  it("makes the command, shows only its one-time address, copies it, and counts down", async () => {
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue(made());
    const link = linkController(helperStatus("not_installed"));
    render(box(link));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Set up the ProPresenter computer" }));
    expect(await screen.findByTestId("propresenter-install-command")).toHaveTextContent("curl -fsS http://192.168.1.20:8000/propresenter-helper/abc123/install.sh | sh");
    expect(api.startProPresenterHelperInstall).toHaveBeenCalledWith();
    expect(screen.getByTestId("propresenter-install-label")).toHaveTextContent("ProPresenter Link for the ProPresenter computer:");
    expect(screen.getByTestId("propresenter-install-waiting")).toHaveTextContent(/Waiting for the ProPresenter Link… The command works for 30:00 more, on this network only\./);
    expect(screen.getByTestId("propresenter-install-ready")).toHaveTextContent(/Terminal/);
    expect(screen.getByTestId("propresenter-install-ready")).toHaveTextContent(/Accessibility/);
    expect(link.watch).toHaveBeenLastCalledWith(true);
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(await navigator.clipboard.readText()).toBe(made().command);
    expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument();
  });

  it("the command can be reached and read with the keyboard", async () => {
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue(made());
    render(box(linkController(helperStatus("not_installed"))));
    await userEvent.setup().click(screen.getByRole("button", { name: "Set up the ProPresenter computer" }));
    expect((await screen.findByTestId("propresenter-install-command")).tabIndex).toBe(0);
  });

  it("opened on purpose, it makes the command at once, even while the Link already works, and stays open", async () => {
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue(made());
    render(box(linkController(helperStatus("ready")), true, true));
    expect(await screen.findByTestId("propresenter-install-command")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "ProPresenter Link command" })).toBeInTheDocument();
    expect(screen.getByTestId("propresenter-install-waiting")).toHaveTextContent(/^The command works for 30:00 more, on this network only\.$/);
    expect(screen.queryByTestId("propresenter-install-done")).toBeNull();
  });

  it("does not make a command just because it is on screen: only when opened on purpose", () => {
    render(box(linkController(helperStatus("not_installed"))));
    expect(api.startProPresenterHelperInstall).not.toHaveBeenCalled();
  });

  it("never shows a key: nothing on screen is the length of one", async () => {
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue(made());
    render(box(linkController(helperStatus("not_installed"))));
    await userEvent.setup().click(screen.getByRole("button", { name: "Set up the ProPresenter computer" }));
    await screen.findByTestId("propresenter-install-command");
    const long = (document.body.textContent ?? "").split(/\s+/).filter((word) => /^[A-Za-z0-9_-]{43}$/.test(word));
    expect(long).toEqual([]);
  });

  it("Cancel ends the session at once and goes back to the button", async () => {
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue(made());
    const link = linkController(helperStatus("not_installed"));
    render(box(link));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Set up the ProPresenter computer" }));
    await screen.findByTestId("propresenter-install-command");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.endProPresenterHelperInstall).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("propresenter-install-command")).toBeNull();
    expect(screen.getByRole("button", { name: "Set up the ProPresenter computer" })).toBeInTheDocument();
    expect(link.watch).toHaveBeenLastCalledWith(false);
  });

  it("ends itself when the Link answers: it announces that, closes the box and ends the session", async () => {
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue(made());
    const { rerender } = render(box(linkController(helperStatus("not_installed"))));
    await userEvent.setup().click(screen.getByRole("button", { name: "Set up the ProPresenter computer" }));
    await screen.findByTestId("propresenter-install-command");
    rerender(box(linkController(helperStatus("ready"))));
    await waitFor(() => expect(screen.getByTestId("propresenter-install-done")).toBeInTheDocument());
    expect(screen.getByTestId("propresenter-install-live")).toHaveTextContent("The ProPresenter Link is ready.");
    expect(screen.getByTestId("propresenter-install-live")).toHaveAttribute("aria-live", "polite");
    expect(api.endProPresenterHelperInstall).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("propresenter-install-command")).toBeNull();
  });

  it("an update ends only when the Link is ready again, not while it is still outdated", async () => {
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue(made());
    const { rerender } = render(box(linkController(helperStatus("outdated"))));
    await userEvent.setup().click(screen.getByRole("button", { name: "Update the ProPresenter Link" }));
    await screen.findByTestId("propresenter-install-command");
    rerender(box(linkController(helperStatus("outdated"))));
    expect(screen.getByTestId("propresenter-install-command")).toBeInTheDocument();
    rerender(box(linkController(helperStatus("ready"))));
    await waitFor(() => expect(screen.getByTestId("propresenter-install-done")).toBeInTheDocument());
  });

  it("says when the command has expired and makes a new one", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValueOnce(made(1)).mockResolvedValueOnce(made());
    render(box(linkController(helperStatus("not_installed"))));
    await userEvent.setup({ advanceTimers: vi.advanceTimersByTime }).click(screen.getByRole("button", { name: "Set up the ProPresenter computer" }));
    await screen.findByTestId("propresenter-install-command");
    await act(async () => { await vi.advanceTimersByTimeAsync(61_000); });
    expect(screen.getByRole("alert")).toHaveTextContent("That command has expired. Make a new one.");
    await userEvent.setup({ advanceTimers: vi.advanceTimersByTime }).click(screen.getByRole("button", { name: "Make a new command" }));
    expect(await screen.findByTestId("propresenter-install-command")).toBeInTheDocument();
  });

  it("error shapes: the server's sentence when it is a sentence, a plain fallback when it is not, then Try again", async () => {
    vi.mocked(api.startProPresenterHelperInstall)
      .mockRejectedValueOnce(new api.ApiError("Could not find this computer's address on the ProPresenter network", 409, "no_lan_address"))
      .mockRejectedValueOnce(new api.ApiError("{\"oops\":1}", 500))
      .mockRejectedValueOnce(new TypeError("Failed to fetch"))
      .mockResolvedValueOnce(made());
    render(box(linkController(helperStatus("not_installed"))));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Set up the ProPresenter computer" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not find this computer's address on the ProPresenter network");
    expect(screen.queryByLabelText("This computer's address")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("StagePilot hit an error. Try again.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText(/Can't reach StagePilot/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByTestId("propresenter-install-command")).toBeInTheDocument();
  });

  it("a failed copy says to copy by hand", async () => {
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue(made());
    render(box(linkController(helperStatus("not_installed"))));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Set up the ProPresenter computer" }));
    await screen.findByTestId("propresenter-install-command");
    const spy = vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("no"));
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Copy did not work here. Select the command and copy it by hand.");
    spy.mockRestore();
  });
});
