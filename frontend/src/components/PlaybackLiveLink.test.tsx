import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import { libraryController, libraryPlan, librarySongOf, libraryStatus } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
import type { HelperState } from "../types";
import { PlaybackApiPanel } from "./PlaybackApiPanel";
import { PlaybackLiveLink } from "./PlaybackLiveLink";

const scanOf = (state: "idle" | "scanning" | "found" | "not_found", extra = {}) => ({ state, candidates: [], reason: null, current: 0, total: 0, ...extra });
const controller = (overrides: Partial<PlaybackController> = {}): PlaybackController => ({
  status: playbackStatus({ reason: "Connected to Playback at 10.30.0.141.", host: "10.30.0.141", settings: { enabled: true, host: "10.30.0.141", port: 8080, auto_scan: true, fast_transport: true } }),
  events: [], error: null, message: null, pending: null, saveArrangement: vi.fn(async () => true),
  save: vi.fn().mockResolvedValue(true), selectSource: vi.fn().mockResolvedValue(true), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(), ...overrides,
});
const disconnected = (host: string | null = null, extra = {}) => controller({
  status: playbackStatus({
    connected: false, active_source: "none", reason: "Disconnected. Choose Scan Network to find Playback.",
    sources: { playback_api: { connected: false, reason: "Playback is disconnected." }, midi: { connected: false, reason: "MIDI is disconnected." } },
    settings: { enabled: true, host, port: 8080, auto_scan: true, fast_transport: true }, ...extra,
  }),
});
const block = () => screen.getByTestId("playback-live-link");
const line = (id: "playback-banner" | "network-script-status") => within(block()).getByTestId(id);

describe("Playback Live Link: the first block of the Playback API panel", () => {
  it("is titled Playback Live Link, in the bordered Live Link card with a state pill beside the title", () => {
    render(<PlaybackLiveLink library={libraryController("ready")} playback={controller()} />);
    expect(screen.getByRole("heading", { name: "Playback Live Link" })).toBeInTheDocument();
    expect(block()).toHaveAttribute("aria-label", "Playback Live Link");
    expect(block()).toHaveClass("live-link", "rounded-ds-xl", "border", "border-edge", "bg-surface-panel-strong");
    expect(screen.getByTestId("playback-live-link-pill")).toHaveTextContent("Playback API: Connected");
    expect(screen.getByTestId("playback-live-link-pill")).toHaveTextContent("Network script: Connected");
  });

  it("is the first thing in the Playback API panel, ahead of the song order", () => {
    const { container } = render(<PlaybackApiPanel library={libraryController("ready")} playback={controller()} />);
    expect(container.firstElementChild!.firstElementChild).toBe(block());
    expect(block().compareDocumentPosition(document.getElementById("song-order-heading")!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("says both statuses in the pill, each a dot AND words, and the full sentences in the quiet strip under the fields", () => {
    render(<PlaybackLiveLink library={libraryController("ready")} playback={controller()} />);
    const pill = screen.getByTestId("playback-live-link-pill");
    expect(pill).toHaveTextContent("Playback API: Connected");
    expect(pill).toHaveTextContent("Network script: Connected");
    expect(pill.querySelectorAll("[data-tone]").length).toBeGreaterThanOrEqual(2);
    expect(line("playback-banner")).toHaveTextContent("Connected to Playback at 10.30.0.141.");
    expect(line("network-script-status")).toHaveTextContent("Network script: Ready, version 0.3.0");
    expect(within(block()).getByTestId("live-link-sub")).toContainElement(line("playback-banner"));
  });

  it.each([
    ["ready", true, "Playback API: Connected", "Network script: Connected"],
    ["outdated", true, "Playback API: Connected", "Network script: Update available"],
    ["not_installed", true, "Playback API: Connected", "Network script: Not installed"],
    ["needs_key", true, "Playback API: Connected", "Network script: Needs setup"],
    ["ready", false, "Playback API: Not connected", "Network script: Connected"],
    ["not_installed", false, "Playback API: Not connected", "Network script: Not installed"],
  ] as [HelperState, boolean, string, string][])("the pill for a %s script with the API %s says its two statuses", (state, api, apiWords, scriptWords) => {
    render(<PlaybackLiveLink library={libraryController(state)} playback={api ? controller() : disconnected("10.30.0.141")} />);
    expect(screen.getByTestId("playback-live-link-pill")).toHaveTextContent(apiWords);
    expect(screen.getByTestId("playback-live-link-pill")).toHaveTextContent(scriptWords);
    expect(screen.getByTestId("playback-live-link-pill")).toHaveAttribute("aria-label", expect.stringContaining("Playback API:"));
  });

  it.each([
    ["not_installed", "Not installed"],
    ["outdated", "Update available, version 0.2.0"],
    ["needs_key", "Needs the command again"],
    ["unreachable", "Not answering"],
    ["forgotten", "Forgotten"],
  ] as [HelperState, string][])("a %s script reads %s", (state, words) => {
    render(<PlaybackLiveLink library={libraryController(state)} playback={controller()} />);
    expect(line("network-script-status")).toHaveTextContent(`Network script: ${words}`);
  });

  it("a computer with no network script is not a warning anywhere in the block", () => {
    render(<PlaybackLiveLink library={libraryController("not_installed")} playback={controller()} />);
    expect(line("network-script-status")).toHaveAttribute("data-tone", "idle");
    expect(within(block()).queryByRole("alert")).toBeNull();
    expect(within(block()).queryByTestId("network-script-note")).toBeNull();
    expect(block().querySelector(".text-warning-text, .text-danger-text")).toBeNull();
  });

  it("when the script answers but Playback's data cannot be read yet, its words stand under the lines", () => {
    const library = libraryController("ready");
    library.status = libraryStatus("ready", { state: "waiting", message: "The Playback library is running but not ready yet: macOS has to be told to allow it." });
    render(<PlaybackLiveLink library={library} playback={controller()} />);
    expect(screen.getByTestId("network-script-note")).toHaveTextContent("not ready yet");
  });

  it("without a library controller (a window that cannot configure) it is the Playback API block alone", () => {
    render(<PlaybackLiveLink playback={controller()} />);
    expect(screen.queryByTestId("network-script-status")).toBeNull();
    expect(screen.queryByRole("button", { name: "Advanced" })).toBeNull();
    expect(screen.queryByTestId("playback-install")).toBeNull();
  });
});

describe("the network script setup box in the block", () => {
  it.each(["not_installed", "needs_key", "unreachable", "forgotten"] as HelperState[])("is offered when the script is %s", (state) => {
    render(<PlaybackLiveLink library={libraryController(state)} playback={controller()} />);
    expect(screen.getByRole("button", { name: "Set up the Playback computer" })).toBeEnabled();
  });

  it("offers the update when the script is outdated, and nothing when it is ready", () => {
    const { unmount } = render(<PlaybackLiveLink library={libraryController("outdated")} playback={controller()} />);
    // Two buttons carry that name: the strip's opener and the one in the open box that makes the command.
    expect(within(screen.getByTestId("playback-install")).getByRole("button", { name: "Update the network script" })).toBeEnabled();
    unmount();
    render(<PlaybackLiveLink library={libraryController("ready")} playback={controller()} />);
    expect(screen.queryByTestId("playback-install")).toBeNull();
    expect(screen.queryByRole("button", { name: /Set up the Playback computer|Update the network script/ })).toBeNull();
  });

  it("waits to be offered until the script has answered once, and asks to choose the computer first when there is none", () => {
    const { unmount } = render(<PlaybackLiveLink library={libraryController("not_installed", { status: null })} playback={controller()} />);
    expect(screen.queryByTestId("playback-install")).toBeNull();
    unmount();
    render(<PlaybackLiveLink library={libraryController("not_installed", { target: "" })} playback={disconnected(null)} />);
    expect(screen.getByTestId("playback-install-need-computer")).toHaveTextContent("Choose the Playback computer above");
    expect(screen.queryByRole("button", { name: "Set up the Playback computer" })).toBeNull();
  });
});

describe("the computer field takes in the scan, with the old behaviour", () => {
  it("Scan Network is the first button, and the field is labelled Playback computer", () => {
    render(<PlaybackLiveLink library={libraryController("ready")} playback={disconnected()} />);
    const first = within(block()).getAllByRole("button")[0];
    expect(first).toBe(screen.getByRole("button", { name: "Scan Network" }));
    expect(screen.getByLabelText("Playback computer").tagName).toBe("SELECT");
  });

  it("choosing a computer sets the API host and the network script's address: an Advanced override is cleared", async () => {
    const playback = disconnected(null, { scan: scanOf("found", { candidates: [{ name: "Example Mac", host: "10.30.0.141" }, { name: "Other PC", host: "10.20.0.102" }] }) });
    const library = libraryController("not_installed", { address: "http://10.99.0.5" });
    render(<PlaybackLiveLink library={library} playback={playback} />);
    fireEvent.click(within(screen.getByRole("group", { name: "Choose which Playback" })).getByRole("button", { name: /Other PC/ }));
    expect(playback.scan).toHaveBeenCalledWith("10.20.0.102");
    expect(library.saveAddress).toHaveBeenCalledWith("");
  });

  it("Connect scans that host and leaves the script's address alone when there is no override", async () => {
    const playback = disconnected("10.30.0.141", { scan: scanOf("found", { candidates: [{ name: "Example Mac", host: "10.30.0.141" }] }) });
    const library = libraryController("not_installed");
    render(<PlaybackLiveLink library={library} playback={playback} />);
    await userEvent.setup().click(screen.getByRole("button", { name: "Connect" }));
    expect(playback.scan).toHaveBeenCalledWith("10.30.0.141");
    expect(library.saveAddress).not.toHaveBeenCalled();
  });
});

describe("Advanced: the address override and Forget the network script", () => {
  const openAdvanced = async () => userEvent.setup().click(screen.getByRole("button", { name: /Advanced/ }));

  it("saves an override of the script's address, and shows the saved one without http://", async () => {
    const library = libraryController("ready", { address: "http://10.20.0.102:9000" });
    render(<PlaybackLiveLink library={library} playback={controller()} />);
    await openAdvanced();
    const input = screen.getByLabelText("Network script address");
    expect(input).toHaveValue("10.20.0.102:9000");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    await userEvent.setup().clear(input);
    await userEvent.setup().type(input, "10.20.0.102");
    await userEvent.setup().click(screen.getByRole("button", { name: "Save" }));
    expect(library.saveAddress).toHaveBeenCalledWith("10.20.0.102");
  });

  it("asks before forgetting, says what it does, and forgets only on the confirm", async () => {
    const library = libraryController("ready");
    render(<PlaybackLiveLink library={library} playback={controller()} />);
    await openAdvanced();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Forget the network script" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Forget the network script?");
    expect(dialog).toHaveTextContent("keeps running on the Playback computer");
    expect(library.forget).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(library.forget).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Forget the network script" }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Forget the network script" }));
    expect(library.forget).toHaveBeenCalledTimes(1);
  });

  it("has nothing to forget when there is no key", async () => {
    render(<PlaybackLiveLink library={libraryController("forgotten")} playback={controller()} />);
    await openAdvanced();
    expect(screen.queryByRole("button", { name: "Forget the network script" })).toBeNull();
  });
});

describe("a computer with no network script looks like a build without the library", () => {
  const withoutBlock = (html: HTMLElement) => {
    const copy = html.cloneNode(true) as HTMLElement;
    copy.querySelector("[data-testid='playback-live-link']")?.remove();
    // React's generated ids (`_r_1_`) count up with every render; they are not part of the look.
    return copy.innerHTML.replace(/_r_[0-9a-z]+_/g, "_r_");
  };
  const panel = (library?: PlaybackLibraryController) => render(<PlaybackApiPanel library={library} playback={controller()} plan={null} />).container;

  it.each(["not_installed", "needs_key", "unreachable", "forgotten"] as HelperState[])("%s: everything outside the new block is the same", (state) => {
    const plain = withoutBlock(panel(undefined));
    document.body.innerHTML = "";
    const other = withoutBlock(panel(libraryController(state)));
    expect(other).toBe(plain);
    expect(screen.queryByTestId("library-songs")).toBeNull();
    expect(screen.queryByTestId("library-notices")).toBeNull();
    expect(screen.queryByTestId("playback-library")).toBeNull();
  });

  it("and only a ready or outdated script brings the song chips in", () => {
    const songs = libraryPlan([librarySongOf("Song One", 1)]);
    for (const state of ["ready", "outdated"] as HelperState[]) {
      const { unmount } = render(<PlaybackApiPanel library={libraryController(state, { plan: songs })} playback={controller()} plan={null} />);
      expect(screen.getByTestId("library-songs")).toBeInTheDocument();
      unmount();
    }
    // The backend never sends a plan for the others; even if a stale one were held, a state that is not active has none.
    render(<PlaybackApiPanel library={libraryController("not_installed")} playback={controller()} plan={null} />);
    expect(screen.queryByTestId("library-songs")).toBeNull();
  });
});
