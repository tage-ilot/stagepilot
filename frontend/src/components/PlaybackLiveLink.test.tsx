import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import { helperOf, libraryController, libraryPlan, librarySongOf, libraryStatus } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
import type { HelperState, LibraryStatus } from "../types";
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
const line = (id: "playback-banner" | "library-link-status") => within(block()).getByTestId(id);

describe("Playback Live Link: the first block of the Playback API panel", () => {
  it("is titled Playback Live Link, in the bordered Live Link card with two status pills beside the title", () => {
    render(<PlaybackLiveLink library={libraryController("ready")} playback={controller()} />);
    expect(screen.getByRole("heading", { name: "Playback Live Link" })).toBeInTheDocument();
    expect(block()).toHaveAttribute("aria-label", "Playback Live Link");
    expect(block()).toHaveClass("live-link", "rounded-ds-xl", "border", "border-edge", "bg-surface-panel-strong");
    expect(screen.getByTestId("playback-api-pill")).toHaveTextContent("Playback API: Connected");
    expect(screen.getByTestId("library-link-pill")).toHaveTextContent("Library Link: Connected");
  });

  it("is the first thing in the Playback API panel, ahead of the song order", () => {
    const { container } = render(<PlaybackApiPanel library={libraryController("ready")} playback={controller()} />);
    expect(container.firstElementChild!.firstElementChild).toBe(block());
    expect(block().compareDocumentPosition(document.getElementById("song-order-heading")!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("says each status in its own pill, a dot AND words, and the full sentences in the quiet strip under the fields", () => {
    render(<PlaybackLiveLink library={libraryController("ready")} playback={controller()} />);
    const apiPill = screen.getByTestId("playback-api-pill");
    const scriptPill = screen.getByTestId("library-link-pill");
    expect(apiPill).toHaveTextContent("Playback API: Connected");
    expect(scriptPill).toHaveTextContent("Library Link: Connected");
    expect(apiPill).toHaveAttribute("data-tone", "success");
    expect(scriptPill).toHaveAttribute("data-tone", "success");
    expect(apiPill.querySelector("[data-tone]")).not.toBeNull();
    expect(scriptPill.querySelector("[data-tone]")).not.toBeNull();
    expect(apiPill).not.toContainElement(scriptPill);
    expect(line("playback-banner")).toHaveTextContent("Connected to Playback at 10.30.0.141.");
    expect(line("library-link-status")).toHaveTextContent("Library Link: Ready, version 0.3.0");
    expect(within(block()).getByTestId("live-link-sub")).toContainElement(line("playback-banner"));
  });

  it.each([
    ["ready", true, "Playback API: Connected", "Library Link: Connected"],
    ["outdated", true, "Playback API: Connected", "Library Link: Update available"],
    ["not_installed", true, "Playback API: Connected", "Library Link: Not installed"],
    ["needs_key", true, "Playback API: Connected", "Library Link: Needs setup"],
    ["ready", false, "Playback API: Not connected", "Library Link: Connected"],
    ["not_installed", false, "Playback API: Not connected", "Library Link: Not installed"],
  ] as [HelperState, boolean, string, string][])("the pills for a %s script with the API %s say the two statuses", (state, api, apiWords, scriptWords) => {
    render(<PlaybackLiveLink library={libraryController(state)} playback={api ? controller() : disconnected("10.30.0.141")} />);
    expect(screen.getByTestId("playback-api-pill")).toHaveTextContent(apiWords);
    expect(screen.getByTestId("library-link-pill")).toHaveTextContent(scriptWords);
    expect(screen.getByTestId("playback-api-pill")).toHaveAttribute("aria-label", expect.stringContaining("Playback API:"));
    expect(screen.getByTestId("library-link-pill")).toHaveAttribute("aria-label", expect.stringContaining("Library Link:"));
  });

  it.each([
    ["not_installed", "Not installed"],
    ["outdated", "Update available, version 0.2.0"],
    ["needs_key", "Needs the command again"],
    ["unreachable", "Not answering"],
  ] as [HelperState, string][])("a %s script reads %s", (state, words) => {
    render(<PlaybackLiveLink library={libraryController(state)} playback={controller()} />);
    expect(line("library-link-status")).toHaveTextContent(`Library Link: ${words}`);
  });

  it("a computer with no Library Link is not a warning anywhere in the block", () => {
    render(<PlaybackLiveLink library={libraryController("not_installed")} playback={controller()} />);
    expect(line("library-link-status")).toHaveAttribute("data-tone", "idle");
    expect(within(block()).queryByRole("alert")).toBeNull();
    expect(within(block()).queryByTestId("library-link-note")).toBeNull();
    expect(block().querySelector(".text-warning-text, .text-danger-text")).toBeNull();
  });

  it("when the script answers but Playback's data cannot be read yet, its words stand under the lines", () => {
    const library = libraryController("ready");
    library.status = libraryStatus("ready", { state: "waiting", message: "The Playback library is running but not ready yet: macOS has to be told to allow it." });
    render(<PlaybackLiveLink library={library} playback={controller()} />);
    expect(screen.getByTestId("library-link-note")).toHaveTextContent("not ready yet");
  });

  it("without a library controller (a window that cannot configure) it is the Playback API block alone", () => {
    render(<PlaybackLiveLink playback={controller()} />);
    expect(screen.queryByTestId("library-link-status")).toBeNull();
    expect(screen.queryByTestId("library-link-pill")).toBeNull();
    expect(screen.getByTestId("playback-api-pill")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Advanced" })).toBeNull();
    expect(screen.queryByTestId("playback-install")).toBeNull();
  });
});

describe("the Library Link setup box in the block", () => {
  it.each(["not_installed", "needs_key", "unreachable"] as HelperState[])("is offered when the script is %s", (state) => {
    render(<PlaybackLiveLink library={libraryController(state)} playback={controller()} />);
    expect(screen.getByRole("button", { name: "Set up the Playback computer" })).toBeEnabled();
  });

  it("offers the update when the script is outdated, and nothing when it is ready", () => {
    const { unmount } = render(<PlaybackLiveLink library={libraryController("outdated")} playback={controller()} />);
    // Two buttons carry that name: the strip's opener and the one in the open box that makes the command.
    expect(within(screen.getByTestId("playback-install")).getByRole("button", { name: "Update the Library Link" })).toBeEnabled();
    unmount();
    render(<PlaybackLiveLink library={libraryController("ready")} playback={controller()} />);
    expect(screen.queryByTestId("playback-install")).toBeNull();
    expect(screen.queryByRole("button", { name: /Set up the Playback computer|Update the Library Link/ })).toBeNull();
  });

  it("says in one line that StagePilot updates an older script by itself, keeps the setup box closed, and says once that it did", () => {
    const base = libraryController("outdated");
    const make = (state: HelperState, update: NonNullable<LibraryStatus["helper"]["update"]>) => libraryController(state, {
      status: { ...(base.status as LibraryStatus), helper: helperOf(state, { updater: true, update }) },
    });
    const { unmount } = render(<PlaybackLiveLink library={make("outdated", { mode: "automatic", version: "0.4.0", message: "A newer version is ready. StagePilot will update the Library Link by itself when nothing is playing." })} playback={controller()} />);
    expect(screen.getByTestId("library-link-update")).toHaveTextContent("StagePilot will update the Library Link by itself when nothing is playing.");
    expect(screen.queryByRole("button", { name: "Update the Library Link" })).toBeNull();
    expect(screen.getByTestId("library-link-pill")).toHaveTextContent("Library Link: Update available");
    unmount();
    render(<PlaybackLiveLink library={make("ready", { mode: "updated", version: "0.4.0", message: "Updated to 0.4.0 automatically." })} playback={controller()} />);
    expect(screen.getByTestId("library-link-update")).toHaveTextContent("Updated to 0.4.0 automatically.");
  });

  it("says nothing about updates for a script that is up to date", () => {
    render(<PlaybackLiveLink library={libraryController("ready")} playback={controller()} />);
    expect(screen.queryByTestId("library-link-update")).toBeNull();
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
  it("Scan is the first button, then Connect, and the field is labelled Playback computer", () => {
    render(<PlaybackLiveLink library={libraryController("ready")} playback={disconnected()} />);
    // The little arrow inside the address box is not a tab stop (tabindex -1): the buttons a person reaches are Scan, then Connect.
    const buttons = within(block()).getAllByRole("button").filter((button) => button.tabIndex !== -1);
    expect(buttons[0]).toBe(screen.getByRole("button", { name: "Scan" }));
    expect(buttons[1]).toBe(screen.getByRole("button", { name: "Connect" }));
    expect(screen.getByRole("combobox", { name: "Playback computer" }).tagName).toBe("INPUT");
    expect(screen.queryByRole("option", { name: /Other computer/ })).toBeNull();
  });

  it("choosing a computer sets the API host and the Library Link's address: an Advanced override is cleared", async () => {
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

describe("recent computers: a computer that connected before is one choice and Connect, with no scan", () => {
  beforeEach(() => window.localStorage.clear());
  afterEach(() => window.localStorage.clear());

  it("remembers the computer it is connected to", () => {
    render(<PlaybackLiveLink library={libraryController("not_installed")} playback={controller()} />);
    expect(JSON.parse(window.localStorage.getItem("stagepilot.recent-addresses.playback") ?? "[]")).toEqual(["10.30.0.141"]);
  });

  it("does not remember a computer it could not connect to", () => {
    render(<PlaybackLiveLink library={libraryController("not_installed")} playback={disconnected("10.30.0.9")} />);
    expect(window.localStorage.getItem("stagepilot.recent-addresses.playback")).toBeNull();
  });

  it("lists the scanned ones, then the recent ones (once each), and Connect on a recent one connects to it directly", async () => {
    window.localStorage.setItem("stagepilot.recent-addresses.playback", JSON.stringify(["10.20.0.102", "10.30.0.141", "10.30.0.7"]));
    const playback = disconnected("10.30.0.141", { scan: scanOf("found", { candidates: [{ name: "Example Mac", host: "10.30.0.7" }] }) });
    render(<PlaybackLiveLink library={libraryController("not_installed")} playback={playback} />);
    const user = userEvent.setup();
    expect(screen.getByRole("combobox", { name: "Playback computer" })).toHaveValue("10.30.0.141");
    await user.click(screen.getByRole("button", { name: "Show recent and scanned addresses" }));
    const groups = within(screen.getByRole("listbox")).getAllByRole("group");
    expect(groups.map((group) => within(group).getAllByRole("option").map((option) => option.textContent?.replace("(in the box now)", "")))).toEqual([["Example Mac (10.30.0.7)"], ["10.20.0.102", "10.30.0.141"]]);
    await user.click(screen.getByRole("option", { name: "10.20.0.102" }));
    expect(playback.scan).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect(playback.scan).toHaveBeenCalledWith("10.20.0.102");
  });

  it("a computer that connects is in the list afterwards (the list follows the memory)", async () => {
    const view = render(<PlaybackLiveLink library={libraryController("not_installed")} playback={disconnected("10.30.0.9")} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Show recent and scanned addresses" }));
    expect(screen.getByText("No recent or scanned addresses yet.")).toBeVisible();
    view.rerender(<PlaybackLiveLink library={libraryController("not_installed")} playback={controller()} />);
    expect(await screen.findByRole("option", { name: /10\.30\.0\.141/ })).toBeVisible();
  });
});

describe("the Library Link has no Forget button and no address box (it is always on the Playback computer)", () => {
  it("has no Advanced button, no Forget button and no Library Link address box in any state", () => {
    for (const state of ["ready", "not_installed", "outdated", "needs_key", "unreachable"] as HelperState[]) {
      const { unmount } = render(<PlaybackLiveLink library={libraryController(state)} playback={controller()} />);
      expect(screen.queryByRole("button", { name: /Advanced/ })).toBeNull();
      expect(screen.queryByRole("button", { name: /Forget/ })).toBeNull();
      expect(screen.queryByLabelText("Library Link address")).toBeNull();
      expect(document.body.textContent).not.toMatch(/Forget the Library Link|Library Link address|Same as the Playback computer/);
      unmount();
    }
  });

  it("clears an address typed in an older version, once", () => {
    const library = libraryController("ready", { address: "http://10.20.0.102:9000" });
    const { rerender } = render(<PlaybackLiveLink library={library} playback={controller()} />);
    rerender(<PlaybackLiveLink library={library} playback={controller()} />);
    expect(library.saveAddress).toHaveBeenCalledTimes(1);
    expect(library.saveAddress).toHaveBeenCalledWith("");
    const clean = libraryController("ready");
    render(<PlaybackLiveLink library={clean} playback={controller()} />);
    expect(clean.saveAddress).not.toHaveBeenCalled();
  });
});

describe("a computer with no Library Link looks like a build without the library", () => {
  const withoutBlock = (html: HTMLElement) => {
    const copy = html.cloneNode(true) as HTMLElement;
    copy.querySelector("[data-testid='playback-live-link']")?.remove();
    // React's generated ids (`_r_1_`) count up with every render; they are not part of the look.
    return copy.innerHTML.replace(/_r_[0-9a-z]+_/g, "_r_");
  };
  const panel = (library?: PlaybackLibraryController) => render(<PlaybackApiPanel library={library} playback={controller()} plan={null} />).container;

  it.each(["not_installed", "needs_key", "unreachable"] as HelperState[])("%s: everything outside the new block is the same", (state) => {
    const plain = withoutBlock(panel(undefined));
    document.body.innerHTML = "";
    const other = withoutBlock(panel(libraryController(state)));
    expect(other).toBe(plain);
    expect(screen.queryByTestId("library-songs")).toBeNull();
    expect(screen.queryByTestId("library-notices")).toBeNull();
    expect(screen.queryByTestId("playback-library")).toBeNull();
  });

  it("puts Songs in Playback right under the Live Link block and above Song order", () => {
    render(<PlaybackApiPanel library={libraryController("ready", { plan: libraryPlan([librarySongOf("Song One", 1)]) })} playback={controller()} plan={null} />);
    const live = screen.getByTestId("playback-live-link");
    const songs = screen.getByTestId("playback-library");
    const order = screen.getByRole("region", { name: "Song order" });
    expect(live.compareDocumentPosition(songs) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(songs.compareDocumentPosition(order) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("and only a ready or outdated Library Link brings the song chips in", () => {
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
