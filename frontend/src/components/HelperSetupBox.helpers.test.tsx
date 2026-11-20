import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";

import * as api from "../api";
import { helperOf, libraryController } from "../test/libraryFixtures";
import { helperStatus, linkController } from "../test/propresenterLinkFixtures";
import type { HelperState, LibraryStatus, LightkeyHelperInstall, LightkeyStatus } from "../types";
import type { LinkState } from "../propresenterLink";
import { lightkeyBoxProps, playbackBoxProps, proPresenterBoxProps } from "./helperBoxProps";
import { countdownText } from "./lightkeyProject";
import { HelperSetupBox } from "./sections";

/**
 * The three helpers' setup boxes (Lightkey, Playback Library Link, Presenter Link) are ONE component, `HelperSetupBox`, with each
 * helper's words and status mapping in `helperBoxProps.tsx`. These are the tests of the three boxes it replaced, kept assertion for
 * assertion: the scenarios every helper shares are parametrised, and what only one helper does keeps its own block.
 */
vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  startLightkeyHelperInstall: vi.fn(),
  endLightkeyHelperInstall: vi.fn(),
  startPlaybackHelperInstall: vi.fn(),
  endPlaybackHelperInstall: vi.fn(),
  startProPresenterHelperInstall: vi.fn(),
  endProPresenterHelperInstall: vi.fn(),
}));

/** How a helper's status looks in each situation the shared scenarios need. */
type Situation = "not_installed" | "outdated" | "ready" | "unanswered";
interface Opts { hasComputer?: boolean; autoStart?: boolean }
interface Harness {
  name: string;
  /** The box's test id. */
  id: string;
  setup: string;
  update: string;
  command: string;
  done: string;
  waitingFor: string;
  label: string;
  start: Mock;
  end: Mock;
  made: (minutes?: number) => LightkeyHelperInstall;
  /** The box for a situation, and the `watch` spy (if the helper's controller has one). */
  box: (situation: Situation, opts?: Opts) => { ui: ReactElement; watch?: Mock };
  /** The helper has a computer to name before a command can be made. */
  needsComputer: boolean;
  /** Closing the box ends a command that was made on purpose. */
  endsOnClose: boolean;
}

const lightkeyStatus = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://192.168.1.20:8770", link: "unreachable", detail: "No answer.", active_count: null, recording: false, recording_song: null,
  replay: true, replay_blocked: null, replaying_song: null, message: null, ...over,
});
const connected = (over: Partial<LightkeyStatus> = {}) => lightkeyStatus({ link: "connected", detail: null, ...over });
const oldHelper = { outdated: true } as LightkeyStatus["helper"];
const newHelper = { outdated: false, version: "0.5.0" } as LightkeyStatus["helper"];
const lightkeyFor = (situation: Situation): LightkeyStatus => situation === "ready" ? connected({ helper: newHelper }) : situation === "outdated" ? connected({ helper: oldHelper }) : lightkeyStatus();

const made = (kind: string, version: string) => (minutes = 30): LightkeyHelperInstall => ({
  command: `curl -fsS http://192.168.1.20:8000/${kind}-helper/abc123/install.sh | sh`,
  url: `http://192.168.1.20:8000/${kind}-helper/abc123/install.sh`,
  expires_at: new Date(Date.now() + minutes * 60_000).toISOString(),
  lan_ip: "192.168.1.20",
  port: 8000,
  bundled_version: version,
});

const harnesses: [Harness, Harness, Harness] = [
  {
    name: "Lightkey helper", id: "lightkey-install", setup: "Set up the Lighting computer", update: "Update the helper", command: "Lighting computer command",
    done: "The Lighting computer is connected.", waitingFor: "the helper", label: "Capture Script for Lighting Computer (beta, only for Lightkey):",
    start: vi.mocked(api.startLightkeyHelperInstall), end: vi.mocked(api.endLightkeyHelperInstall), made: made("lightkey", "0.5.0"), needsComputer: false, endsOnClose: true,
    box: (situation, opts = {}) => ({ ui: <HelperSetupBox {...lightkeyBoxProps(lightkeyFor(situation))} autoStart={opts.autoStart ?? false} /> }),
  },
  {
    name: "Playback Library Link", id: "playback-install", setup: "Set up the Playback computer", update: "Update the Library Link", command: "Library Link command",
    done: "The Library Link is ready.", waitingFor: "the Library Link", label: "Library Link for the Playback computer:",
    start: vi.mocked(api.startPlaybackHelperInstall), end: vi.mocked(api.endPlaybackHelperInstall), made: made("playback", "0.3.0"), needsComputer: true, endsOnClose: false,
    box: (situation, opts = {}) => {
      const state: HelperState = situation === "unanswered" ? "not_installed" : situation;
      const library = libraryController(state, situation === "unanswered" ? { status: null } : {});
      return { ui: <HelperSetupBox {...playbackBoxProps(library, opts.hasComputer ?? true)} autoStart={opts.autoStart ?? false} />, watch: library.watch as Mock };
    },
  },
  {
    name: "Presenter Link", id: "propresenter-install", setup: "Set up the ProPresenter computer", update: "Update the Presenter Link", command: "Presenter Link command",
    done: "The Presenter Link is ready.", waitingFor: "the Presenter Link", label: "Presenter Link for the ProPresenter computer:",
    start: vi.mocked(api.startProPresenterHelperInstall), end: vi.mocked(api.endProPresenterHelperInstall), made: made("propresenter", "0.1.0"), needsComputer: true, endsOnClose: false,
    box: (situation, opts = {}) => {
      const link = linkController(situation === "unanswered" ? null : helperStatus(situation as LinkState));
      return { ui: <HelperSetupBox {...proPresenterBoxProps(link, opts.hasComputer ?? true)} autoStart={opts.autoStart ?? false} />, watch: link.watch as Mock };
    },
  },
];

beforeEach(() => {
  for (const h of harnesses) h.end.mockResolvedValue({ ended: true });
});
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); });

describe.each(harnesses)("HelperSetupBox: $name", (h) => {
  it("offers the setup button while it is not ready, and says it runs on the other computer without internet", () => {
    render(h.box("not_installed").ui);
    expect(screen.getByRole("region", { name: h.setup })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: h.setup })).toBeEnabled();
    expect(screen.getByTestId(h.id)).toHaveTextContent(/no internet/);
    expect(screen.getByTestId(h.id)).toHaveTextContent(/Terminal on the (Lighting|Playback|ProPresenter) computer/);
    expect(h.start).not.toHaveBeenCalled();
  });

  it("does not make a command just because it is on screen: only when opened on purpose", () => {
    render(h.box("not_installed").ui);
    expect(h.start).not.toHaveBeenCalled();
  });

  it("is called Update ... when the helper is behind the one StagePilot carries", () => {
    render(h.box("outdated").ui);
    expect(screen.getByRole("region", { name: h.update })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: h.update })).toBeEnabled();
    expect(screen.getByTestId(h.id)).toHaveTextContent("older than the one StagePilot carries");
  });

  it("shows nothing while everything works and nobody asked, and makes no command", () => {
    render(h.box("ready").ui);
    expect(screen.queryByTestId(h.id)).toBeNull();
    expect(h.start).not.toHaveBeenCalled();
  });

  it("makes the command, shows only its one-time address, the label, a countdown and the waiting line", async () => {
    h.start.mockResolvedValue(h.made());
    const { ui, watch } = h.box("not_installed");
    render(ui);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: h.setup }));
    const command = await screen.findByTestId(`${h.id}-command`);
    expect(command).toHaveTextContent(h.made().command);
    expect(command.tagName).toBe("CODE");
    expect(command.textContent).not.toContain("\n");
    expect(command.tabIndex).toBe(0);
    expect(h.start).toHaveBeenCalledWith();
    expect(screen.getByTestId(`${h.id}-label`)).toHaveTextContent(h.label);
    expect(screen.getByTestId(`${h.id}-waiting`)).toHaveTextContent(new RegExp(`Waiting for ${h.waitingFor}… The command works for (30:00|29:5\\d) more, on this network only\\.`));
    expect(screen.getByTestId(`${h.id}-ready`)).toHaveTextContent(/Terminal/);
    expect(screen.getByTestId(`${h.id}-ready`)).toHaveTextContent("xcode-select --install");
    expect(screen.getByTestId(`${h.id}-live`)).toHaveTextContent(`Waiting for ${h.waitingFor}…`);
    expect(screen.getByTestId(`${h.id}-live`)).toHaveAttribute("aria-live", "polite");
    // The page may look more often while it waits (the helpers that have a watch).
    if (watch) expect(watch).toHaveBeenLastCalledWith(true);
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(await navigator.clipboard.readText()).toBe(h.made().command);
    expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument();
  });

  it("says so when copying is not possible and keeps the command to copy by hand", async () => {
    h.start.mockResolvedValue(h.made());
    render(h.box("not_installed").ui);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: h.setup }));
    await screen.findByTestId(`${h.id}-command`);
    const spy = vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("no"));
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Copy did not work here. Select the command and copy it by hand.");
    expect(screen.getByTestId(`${h.id}-command`)).toBeInTheDocument();
    spy.mockRestore();
  });

  it("never shows a key: nothing on screen is the length of one", async () => {
    h.start.mockResolvedValue(h.made());
    render(h.box("not_installed").ui);
    await userEvent.setup().click(screen.getByRole("button", { name: h.setup }));
    await screen.findByTestId(`${h.id}-command`);
    // A key is 43 characters of base64url with no spaces; the command's own words have none that long except the URL.
    const long = (document.body.textContent ?? "").split(/\s+/).filter((word) => /^[A-Za-z0-9_-]{43}$/.test(word));
    expect(long).toEqual([]);
  });

  it("opened on purpose, it makes the command at once, even while everything works, and stays open", async () => {
    h.start.mockResolvedValue(h.made());
    render(h.box("ready", { autoStart: true }).ui);
    expect(await screen.findByTestId(`${h.id}-command`)).toHaveTextContent(h.made().command);
    expect(screen.getByRole("region", { name: h.command })).toBeInTheDocument();
    // Already ready: it is not "waiting", it does not close itself, and it is a single line.
    expect(screen.getByTestId(`${h.id}-waiting`)).toHaveTextContent(/^The command works for 30:00 more, on this network only\.$/);
    expect(screen.queryByTestId(`${h.id}-done`)).toBeNull();
    expect(h.end).not.toHaveBeenCalled();
    expect(h.start).toHaveBeenCalledTimes(1);
  });

  it("Cancel while opened on purpose ends the session and does not start another", async () => {
    h.start.mockResolvedValue(h.made());
    render(h.box("ready", { autoStart: true }).ui);
    await screen.findByTestId(`${h.id}-command`);
    await userEvent.setup().click(screen.getByRole("button", { name: "Cancel" }));
    expect(h.end).toHaveBeenCalledTimes(1);
    expect(h.start).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId(`${h.id}-command`)).toBeNull();
  });

  it("Cancel ends the session at once and goes back to the button", async () => {
    h.start.mockResolvedValue(h.made());
    const { ui, watch } = h.box("not_installed");
    render(ui);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: h.setup }));
    await screen.findByTestId(`${h.id}-command`);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(h.end).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId(`${h.id}-command`)).toBeNull();
    expect(screen.getByRole("button", { name: h.setup })).toBeInTheDocument();
    if (watch) expect(watch).toHaveBeenLastCalledWith(false);
  });

  it("ends itself when the helper answers: it announces that, closes the box and ends the session", async () => {
    h.start.mockResolvedValue(h.made());
    const { rerender } = render(h.box("not_installed").ui);
    await userEvent.setup().click(screen.getByRole("button", { name: h.setup }));
    await screen.findByTestId(`${h.id}-command`);
    rerender(h.box("ready").ui);
    await waitFor(() => expect(screen.getByTestId(`${h.id}-done`)).toBeInTheDocument());
    // The live region stays so the change is announced.
    expect(screen.getByTestId(`${h.id}-live`)).toHaveTextContent(h.done);
    expect(screen.getByTestId(`${h.id}-live`)).toHaveAttribute("aria-live", "polite");
    expect(h.end).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId(`${h.id}-command`)).toBeNull();
    expect(screen.queryByTestId(h.id)).toBeNull();
  });

  it("an update ends only when the helper is ready again, not while it is still outdated", async () => {
    h.start.mockResolvedValue(h.made());
    const { rerender } = render(h.box("outdated").ui);
    await userEvent.setup().click(screen.getByRole("button", { name: h.update }));
    await screen.findByTestId(`${h.id}-command`);
    rerender(h.box("outdated").ui);
    expect(screen.getByTestId(`${h.id}-command`)).toBeInTheDocument();
    expect(h.end).not.toHaveBeenCalled();
    rerender(h.box("ready").ui);
    await waitFor(() => expect(screen.getByTestId(`${h.id}-done`)).toBeInTheDocument());
    expect(h.end).toHaveBeenCalledTimes(1);
  });

  it("says when the command has expired and makes a new one", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    h.start.mockResolvedValueOnce(h.made(1)).mockResolvedValueOnce(h.made());
    render(h.box("not_installed").ui);
    await act(async () => { screen.getByRole("button", { name: h.setup }).click(); });
    await screen.findByTestId(`${h.id}-ready`);
    await act(async () => { await vi.advanceTimersByTimeAsync(61_000); });
    expect(screen.getByTestId(`${h.id}-problem`)).toHaveTextContent("That command has expired. Make a new one.");
    expect(screen.queryByTestId(`${h.id}-ready`)).not.toBeInTheDocument();
    await act(async () => { screen.getByRole("button", { name: "Make a new command" }).click(); });
    expect(await screen.findByTestId(`${h.id}-ready`)).toBeInTheDocument();
    expect(h.start).toHaveBeenCalledTimes(2);
  });

  it("counts down", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    h.start.mockResolvedValue(h.made(10));
    render(h.box("not_installed").ui);
    await act(async () => { screen.getByRole("button", { name: h.setup }).click(); });
    await screen.findByTestId(`${h.id}-ready`);
    expect(screen.getByTestId(`${h.id}-waiting`)).toHaveTextContent(/(10:00|9:5\d)/);
    await act(async () => { await vi.advanceTimersByTimeAsync(5 * 60_000); });
    expect(screen.getByTestId(`${h.id}-waiting`)).toHaveTextContent(/(5:00|4:5\d)/);
  });

  it("never asks for this computer's address: when none can be found it says so in one sentence, with Try again", async () => {
    h.start
      .mockRejectedValueOnce(new api.ApiError("Could not find this computer's address on the network", 409))
      .mockResolvedValueOnce(h.made());
    render(h.box("not_installed").ui);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: h.setup }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not find this computer's address on the network");
    expect(screen.queryByLabelText("This computer's address")).toBeNull();
    expect(screen.queryByRole("textbox", { name: "This computer's address" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Use this address" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(h.start).toHaveBeenLastCalledWith();
    expect(await screen.findByTestId(`${h.id}-ready`)).toBeInTheDocument();
  });

  it("another failure is one plain sentence with Try again", async () => {
    h.start.mockRejectedValueOnce(new api.ApiError("StagePilot hit an error.", 500)).mockResolvedValueOnce(h.made());
    render(h.box("not_installed").ui);
    await userEvent.setup().click(screen.getByRole("button", { name: h.setup }));
    expect(await screen.findByRole("alert")).toHaveTextContent("StagePilot hit an error. Try again.");
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByTestId(`${h.id}-ready`)).toBeInTheDocument();
  });

  if (h.needsComputer) {
    it("with no computer chosen it asks for that first instead of offering a command", () => {
      render(h.box("not_installed", { hasComputer: false }).ui);
      expect(screen.getByTestId(`${h.id}-need-computer`)).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: h.setup })).toBeNull();
      expect(h.start).not.toHaveBeenCalled();
    });
  }

  if (h.endsOnClose) {
    it("closing it (opened on purpose, then closed) ends the session and hides the box", async () => {
      h.start.mockResolvedValue(h.made());
      const { rerender } = render(h.box("ready", { autoStart: true }).ui);
      await screen.findByTestId(`${h.id}-command`);
      rerender(h.box("ready", { autoStart: false }).ui);
      await waitFor(() => expect(h.end).toHaveBeenCalledTimes(1));
      expect(screen.queryByTestId(h.id)).toBeNull();
    });
  }
});

describe("HelperSetupBox: Lightkey helper only", () => {
  const h = harnesses[0];

  it("shows the box for every link state except connected (and Off, which has nothing to set up yet)", () => {
    for (const link of ["unreachable", "lightkey_closed"] as const) {
      const { unmount } = render(<HelperSetupBox {...lightkeyBoxProps(lightkeyStatus({ link }))} />);
      expect(screen.getByTestId("lightkey-install")).toBeInTheDocument();
      unmount();
    }
    for (const over of [connected({ helper: newHelper }), connected(), lightkeyStatus({ link: "off" })]) {
      const { unmount } = render(<HelperSetupBox {...lightkeyBoxProps(over)} />);
      expect(screen.queryByTestId("lightkey-install")).not.toBeInTheDocument();
      unmount();
    }
  });

  it("says in one line what StagePilot does about a helper that is behind the bundled one, with no command", () => {
    for (const [mode, message] of [["automatic", "A newer version is ready."], ["updating", "Updating the helper to 0.6.0\u2026"], ["updated", "Updated to 0.6.0 automatically."]] as const) {
      const helper = { outdated: false, version: "0.5.0", updater: true, update: { mode, version: "0.6.0", message } } as LightkeyStatus["helper"];
      const { unmount } = render(<HelperSetupBox {...lightkeyBoxProps(connected({ helper }))} />);
      expect(screen.getByTestId("lightkey-update-line")).toHaveTextContent(message);
      expect(screen.queryByTestId("lightkey-install")).not.toBeInTheDocument();
      unmount();
    }
    const gaveUp = { outdated: false, version: "0.5.0", update: { mode: "manual", version: "0.6.0", message: "The helper could not update itself. Run the setup command on the Lighting computer to update it." } } as LightkeyStatus["helper"];
    render(<HelperSetupBox {...lightkeyBoxProps(connected({ helper: gaveUp }))} />);
    expect(screen.getByTestId("lightkey-update-note")).toHaveTextContent("could not update itself");
    expect(screen.queryByTestId("lightkey-update-line")).not.toBeInTheDocument();
  });

  it("shows the numbered steps: Lightkey first, then Terminal, then wait", async () => {
    h.start.mockResolvedValue(h.made());
    render(h.box("not_installed").ui);
    await userEvent.click(screen.getByRole("button", { name: h.setup }));
    const ready = await screen.findByTestId("lightkey-install-ready");
    const steps = ready.querySelectorAll("ol > li");
    expect(steps).toHaveLength(3);
    expect(steps[0]).toHaveTextContent("turn on sACN output in Lightkey and open your project");
    expect(steps[1]).toHaveTextContent("Open Terminal there and paste this command");
    expect(steps[2]).toHaveTextContent("Wait here. It connects by itself.");
    expect(ready).toHaveTextContent("Lightkey API");
    expect(ready).toHaveTextContent("local network");
    expect(ready).toHaveTextContent("Documents and Downloads");
    expect(ready).toHaveTextContent("Click Allow");
  });

  it("stays until the helper is current: still connected and old, or briefly lost, does not end the session", async () => {
    h.start.mockResolvedValue(h.made());
    const { rerender } = render(<HelperSetupBox {...lightkeyBoxProps(connected({ helper: oldHelper }))} />);
    await userEvent.click(screen.getByRole("button", { name: "Update the helper" }));
    await screen.findByTestId("lightkey-install-ready");
    rerender(<HelperSetupBox {...lightkeyBoxProps(connected({ helper: oldHelper }))} />);
    rerender(<HelperSetupBox {...lightkeyBoxProps(lightkeyStatus())} />);
    expect(screen.getByTestId("lightkey-install-ready")).toBeInTheDocument();
    expect(h.end).not.toHaveBeenCalled();
    rerender(<HelperSetupBox {...lightkeyBoxProps(connected({ helper: newHelper }))} />);
    await waitFor(() => expect(screen.queryByTestId("lightkey-install")).not.toBeInTheDocument());
    expect(h.end).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("lightkey-install-live")).toHaveTextContent("connected");
  });

  it("formats the countdown", () => {
    expect(countdownText(30 * 60_000)).toBe("30:00");
    expect(countdownText(61_000)).toBe("1:01");
    expect(countdownText(500)).toBe("0:01");
    expect(countdownText(-5)).toBe("0:00");
  });
});

describe("HelperSetupBox: Playback Library Link and Presenter Link", () => {
  it.each([
    ["Library Link", "playback-install", (state: string) => libraryController(({ not_installed: "not_installed", needs_command: "needs_key", not_answering: "unreachable", outdated: "outdated" } as Record<string, HelperState>)[state]), (c: ReturnType<typeof libraryController>) => <HelperSetupBox {...playbackBoxProps(c, true)} />, /cannot reach the Library Link/],
    ["Presenter Link", "propresenter-install", (state: string) => linkController(helperStatus(state as LinkState)), (c: ReturnType<typeof linkController>) => <HelperSetupBox {...proPresenterBoxProps(c, true)} />, /cannot reach the Presenter Link/],
  ] as const)("says why in plain words for each state of the %s", (_name, id, controller, draw, unreachable) => {
    const words: [string, RegExp][] = [["not_installed", /optional/], ["needs_command", /does not know this StagePilot/], ["not_answering", unreachable], ["outdated", /older than the one StagePilot carries/]];
    for (const [state, expected] of words) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const { unmount } = render(draw((controller as any)(state)));
      expect(screen.getByTestId(`${id}-why`)).toHaveTextContent(expected);
      unmount();
    }
  });

  it("offers no command for an older Library Link that StagePilot updates by itself, and offers it when StagePilot cannot", () => {
    const withUpdate = (update: NonNullable<LibraryStatus["helper"]["update"]>) => {
      const library = libraryController("outdated");
      return libraryController("outdated", { status: { ...library.status as LibraryStatus, helper: helperOf("outdated", { updater: true, update }) } });
    };
    for (const mode of ["automatic", "updating"] as const) {
      const { unmount } = render(<HelperSetupBox {...playbackBoxProps(withUpdate({ mode, version: "0.4.0", message: "StagePilot updates it." }), true)} />);
      expect(screen.queryByTestId("playback-install")).toBeNull();
      expect(screen.queryByRole("button", { name: "Update the Library Link" })).toBeNull();
      unmount();
    }
    render(<HelperSetupBox {...playbackBoxProps(withUpdate({ mode: "manual", version: "0.4.0", message: "The Library Link could not update itself. Run the setup command on the Playback computer to update it." }), true)} />);
    expect(screen.getByRole("button", { name: "Update the Library Link" })).toBeEnabled();
    expect(screen.getByTestId("playback-install-why")).toHaveTextContent(/could not update itself.*older than the one StagePilot carries/);
  });

  it("offers no command for an older Presenter Link that StagePilot updates by itself, and offers it when StagePilot cannot", () => {
    for (const mode of ["automatic", "updating"] as const) {
      const { unmount } = render(<HelperSetupBox {...proPresenterBoxProps(linkController(helperStatus("outdated", { update: { mode, version: "0.1.0", message: "StagePilot updates it." } })), true)} />);
      expect(screen.queryByTestId("propresenter-install")).toBeNull();
      unmount();
    }
    render(<HelperSetupBox {...proPresenterBoxProps(linkController(helperStatus("outdated", { update: { mode: "manual", version: "0.1.0", message: "The Presenter Link could not update itself. Run the setup command on the ProPresenter computer to update it." } })), true)} />);
    expect(screen.getByRole("button", { name: "Update the Presenter Link" })).toBeEnabled();
    expect(screen.getByTestId("propresenter-install-why")).toHaveTextContent(/could not update itself.*older than the one StagePilot carries/);
  });

  it("shows nothing before the helper has answered once, and (Presenter Link) while it is being looked at", () => {
    for (const library of [libraryController("not_installed", { status: null })]) {
      const { unmount } = render(<HelperSetupBox {...playbackBoxProps(library, true)} />);
      expect(screen.queryByTestId("playback-install")).toBeNull();
      unmount();
    }
    for (const link of [linkController(null), linkController(helperStatus("checking"))]) {
      const { unmount } = render(<HelperSetupBox {...proPresenterBoxProps(link, true)} />);
      expect(screen.queryByTestId("propresenter-install")).toBeNull();
      unmount();
    }
  });

  it("says what to save first when no ProPresenter computer is saved", () => {
    render(<HelperSetupBox {...proPresenterBoxProps(linkController(helperStatus("not_installed")), false)} />);
    expect(screen.getByTestId("propresenter-install-need-computer")).toHaveTextContent(/Save the Presentation connection above/);
  });

  it("explains the macOS permissions each helper needs", async () => {
    const library = libraryController("not_installed");
    vi.mocked(api.startPlaybackHelperInstall).mockResolvedValue(harnesses[1].made());
    const { unmount } = render(<HelperSetupBox {...playbackBoxProps(library, true)} autoStart />);
    expect(await screen.findByTestId("playback-install-ready")).toHaveTextContent(/needs no internet/);
    expect(screen.getByTestId("playback-install-ready")).toHaveTextContent("Playback Library read Playback's data");
    unmount();
    vi.mocked(api.startProPresenterHelperInstall).mockResolvedValue(harnesses[2].made());
    render(<HelperSetupBox {...proPresenterBoxProps(linkController(helperStatus("not_installed")), true)} autoStart />);
    expect(await screen.findByTestId("propresenter-install-ready")).toHaveTextContent(/Accessibility/);
  });

  it("error shapes: the server's sentence when it is a sentence, a plain fallback when it is not, then Try again", async () => {
    vi.mocked(api.startProPresenterHelperInstall)
      .mockRejectedValueOnce(new api.ApiError("Could not find this computer's address on the ProPresenter network", 409, "no_lan_address"))
      .mockRejectedValueOnce(new api.ApiError("{\"oops\":1}", 500))
      .mockRejectedValueOnce(new TypeError("Failed to fetch"))
      .mockResolvedValueOnce(harnesses[2].made());
    render(<HelperSetupBox {...proPresenterBoxProps(linkController(helperStatus("not_installed")), true)} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Set up the ProPresenter computer" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not find this computer's address on the ProPresenter network");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("StagePilot hit an error. Try again.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText(/Can't reach StagePilot/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByTestId("propresenter-install-command")).toBeInTheDocument();
  });
});
