import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import { helperOf, libraryController } from "../test/libraryFixtures";
import type { HelperState, LibraryStatus, PlaybackHelperInstall } from "../types";
import { PlaybackInstallBox } from "./PlaybackInstallBox";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  startPlaybackHelperInstall: vi.fn(),
  endPlaybackHelperInstall: vi.fn(),
}));

const made = (minutes = 30): PlaybackHelperInstall => ({
  command: "curl -fsS http://192.168.1.20:8000/playback-helper/abc123/install.sh | sh",
  url: "http://192.168.1.20:8000/playback-helper/abc123/install.sh",
  expires_at: new Date(Date.now() + minutes * 60_000).toISOString(),
  lan_ip: "192.168.1.20",
  port: 8000,
  bundled_version: "0.3.0",
});
const box = (library: PlaybackLibraryController, hasComputer = true, autoStart = false) => <PlaybackInstallBox autoStart={autoStart} hasComputer={hasComputer} library={library} />;

beforeEach(() => {
  vi.mocked(api.endPlaybackHelperInstall).mockResolvedValue({ ended: true });
});
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); });

describe("PlaybackInstallBox", () => {
  it("offers the setup button while the script is not ready, and says it runs on the Playback computer without internet", () => {
    render(box(libraryController("not_installed")));
    expect(screen.getByRole("region", { name: "Set up the Playback computer" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Set up the Playback computer" })).toBeEnabled();
    expect(screen.getByTestId("playback-install")).toHaveTextContent(/Terminal on the Playback computer/);
    expect(screen.getByTestId("playback-install")).toHaveTextContent(/no internet/);
    expect(api.startPlaybackHelperInstall).not.toHaveBeenCalled();
  });

  it.each([
    ["not_installed", /optional/],
    ["needs_key", /does not know this StagePilot/],
    ["unreachable", /cannot reach the Library Link/],
    ["outdated", /older than the one StagePilot carries/],
  ] as [HelperState, RegExp][])("says why in plain words for a %s script", (state, words) => {
    render(box(libraryController(state)));
    expect(screen.getByTestId("playback-install")).toHaveTextContent(words);
  });

  it("is called Update the Library Link when the script is outdated", () => {
    render(box(libraryController("outdated")));
    expect(screen.getByRole("button", { name: "Update the Library Link" })).toBeEnabled();
  });

  it("offers no command for an older script that StagePilot updates by itself, and offers it when StagePilot cannot", () => {
    const withUpdate = (update: NonNullable<LibraryStatus["helper"]["update"]>) => {
      const library = libraryController("outdated");
      return libraryController("outdated", { status: { ...library.status as LibraryStatus, helper: helperOf("outdated", { updater: true, update }) } });
    };
    for (const mode of ["automatic", "updating"] as const) {
      const { unmount } = render(box(withUpdate({ mode, version: "0.4.0", message: "StagePilot updates it." })));
      expect(screen.queryByTestId("playback-install")).toBeNull();
      expect(screen.queryByRole("button", { name: "Update the Library Link" })).toBeNull();
      unmount();
    }
    render(box(withUpdate({ mode: "manual", version: "0.4.0", message: "The Library Link could not update itself. Run the setup command on the Playback computer to update it." })));
    expect(screen.getByRole("button", { name: "Update the Library Link" })).toBeEnabled();
    expect(screen.getByTestId("playback-install-why")).toHaveTextContent(/could not update itself.*older than the one StagePilot carries/);
  });

  it("shows nothing when the script is ready or has not answered yet", () => {
    for (const library of [libraryController("ready"), libraryController("not_installed", { status: null })]) {
      const { unmount } = render(box(library));
      expect(screen.queryByTestId("playback-install")).toBeNull();
      unmount();
    }
  });

  it("makes the command, shows only its one-time address, copies it, and counts down", async () => {
    vi.mocked(api.startPlaybackHelperInstall).mockResolvedValue(made());
    const library = libraryController("not_installed");
    render(box(library));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Set up the Playback computer" }));
    expect(await screen.findByTestId("playback-install-command")).toHaveTextContent("curl -fsS http://192.168.1.20:8000/playback-helper/abc123/install.sh | sh");
    expect(api.startPlaybackHelperInstall).toHaveBeenCalledWith();
    expect(screen.getByTestId("playback-install-label")).toHaveTextContent("Library Link for the Playback computer:");
    expect(screen.getByTestId("playback-install-waiting")).toHaveTextContent(/Waiting for the Library Link… The command works for 30:00 more, on this network only\./);
    expect(screen.getByTestId("playback-install-ready")).toHaveTextContent(/Terminal/);
    expect(screen.getByTestId("playback-install-ready")).toHaveTextContent(/needs no internet/);
    // The window looks every few seconds while it waits.
    expect(library.watch).toHaveBeenLastCalledWith(true);
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(await navigator.clipboard.readText()).toBe(made().command);
    expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument();
  });

  it("opened on purpose, it makes the command at once, even while the Library Link already works, and stays open", async () => {
    vi.mocked(api.startPlaybackHelperInstall).mockResolvedValue(made());
    const library = libraryController("ready");
    render(box(library, true, true));
    expect(await screen.findByTestId("playback-install-command")).toHaveTextContent("curl -fsS http://192.168.1.20:8000/playback-helper/abc123/install.sh | sh");
    expect(screen.getByRole("region", { name: "Library Link command" })).toBeInTheDocument();
    // Already ready: it is not "waiting", it does not close itself, and it is a single line.
    expect(screen.getByTestId("playback-install-waiting")).toHaveTextContent(/^The command works for 30:00 more, on this network only\.$/);
    expect(screen.queryByTestId("playback-install-done")).toBeNull();
    expect(screen.getByTestId("playback-install-command").textContent).not.toContain("\n");
    await userEvent.setup().click(screen.getByRole("button", { name: "Copy" }));
    expect(await navigator.clipboard.readText()).toBe(made().command);
    expect(api.startPlaybackHelperInstall).toHaveBeenCalledTimes(1);
  });

  it("does not make a command just because it is on screen: only when opened on purpose", () => {
    render(box(libraryController("not_installed")));
    expect(api.startPlaybackHelperInstall).not.toHaveBeenCalled();
  });

  it("Cancel while opened on purpose ends the session and does not start another", async () => {
    vi.mocked(api.startPlaybackHelperInstall).mockResolvedValue(made());
    render(box(libraryController("ready"), true, true));
    await screen.findByTestId("playback-install-command");
    await userEvent.setup().click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.endPlaybackHelperInstall).toHaveBeenCalledTimes(1);
    expect(api.startPlaybackHelperInstall).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("playback-install-command")).toBeNull();
  });

  it("never shows a key: nothing on screen is the length of one", async () => {
    vi.mocked(api.startPlaybackHelperInstall).mockResolvedValue(made());
    render(box(libraryController("not_installed")));
    await userEvent.setup().click(screen.getByRole("button", { name: "Set up the Playback computer" }));
    await screen.findByTestId("playback-install-command");
    // A key is 43 characters of base64url with no spaces; the command's own words have none that long except the URL.
    const long = (document.body.textContent ?? "").split(/\s+/).filter((word) => /^[A-Za-z0-9_-]{43}$/.test(word));
    expect(long).toEqual([]);
  });

  it("Cancel ends the session at once and goes back to the button", async () => {
    vi.mocked(api.startPlaybackHelperInstall).mockResolvedValue(made());
    const library = libraryController("not_installed");
    render(box(library));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Set up the Playback computer" }));
    await screen.findByTestId("playback-install-command");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.endPlaybackHelperInstall).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("playback-install-command")).toBeNull();
    expect(screen.getByRole("button", { name: "Set up the Playback computer" })).toBeInTheDocument();
    expect(library.watch).toHaveBeenLastCalledWith(false);
  });

  it("ends itself when the script answers: it announces that, closes the box and ends the session", async () => {
    vi.mocked(api.startPlaybackHelperInstall).mockResolvedValue(made());
    const { rerender } = render(box(libraryController("not_installed")));
    await userEvent.setup().click(screen.getByRole("button", { name: "Set up the Playback computer" }));
    await screen.findByTestId("playback-install-command");
    rerender(box(libraryController("ready")));
    await waitFor(() => expect(screen.getByTestId("playback-install-done")).toBeInTheDocument());
    expect(screen.getByTestId("playback-install-live")).toHaveTextContent("The Library Link is ready.");
    expect(api.endPlaybackHelperInstall).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("playback-install-command")).toBeNull();
  });

  it("an update ends only when the script is ready again, not while it is still outdated", async () => {
    vi.mocked(api.startPlaybackHelperInstall).mockResolvedValue(made());
    const { rerender } = render(box(libraryController("outdated")));
    await userEvent.setup().click(screen.getByRole("button", { name: "Update the Library Link" }));
    await screen.findByTestId("playback-install-command");
    rerender(box(libraryController("outdated")));
    expect(screen.getByTestId("playback-install-command")).toBeInTheDocument();
    rerender(box(libraryController("ready")));
    await waitFor(() => expect(screen.getByTestId("playback-install-done")).toBeInTheDocument());
  });

  it("says when the command has expired and makes a new one", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(api.startPlaybackHelperInstall).mockResolvedValueOnce(made(1)).mockResolvedValueOnce(made());
    render(box(libraryController("not_installed")));
    await userEvent.setup({ advanceTimers: vi.advanceTimersByTime }).click(screen.getByRole("button", { name: "Set up the Playback computer" }));
    await screen.findByTestId("playback-install-command");
    await act(async () => { await vi.advanceTimersByTimeAsync(61_000); });
    expect(screen.getByRole("alert")).toHaveTextContent("That command has expired. Make a new one.");
    await userEvent.setup({ advanceTimers: vi.advanceTimersByTime }).click(screen.getByRole("button", { name: "Make a new command" }));
    expect(await screen.findByTestId("playback-install-command")).toBeInTheDocument();
  });

  it("never asks for this computer's address: when none can be found it says so in one sentence, with Try again", async () => {
    vi.mocked(api.startPlaybackHelperInstall)
      .mockRejectedValueOnce(new api.ApiError("Could not find this computer's address on the Playback network", 409))
      .mockResolvedValueOnce(made());
    render(box(libraryController("not_installed")));
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Set up the Playback computer" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not find this computer's address on the Playback network");
    expect(screen.queryByLabelText("This computer's address")).toBeNull();
    expect(screen.queryByRole("button", { name: "Use this address" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(api.startPlaybackHelperInstall).toHaveBeenLastCalledWith();
  });

  it("another failure is one plain sentence with Try again, and no address field", async () => {
    vi.mocked(api.startPlaybackHelperInstall).mockRejectedValueOnce(new api.ApiError("The secure credential store is not answering.", 503));
    render(box(libraryController("not_installed")));
    await userEvent.setup().click(screen.getByRole("button", { name: "Set up the Playback computer" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.queryByLabelText("This computer's address")).toBeNull();
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
  });

  it("with no Playback computer chosen it asks for that first instead of offering a command", () => {
    render(box(libraryController("not_installed", { target: "" }), false));
    expect(screen.getByTestId("playback-install-need-computer")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Set up the Playback computer" })).toBeNull();
  });
});
