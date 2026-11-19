import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../../api";
import { libraryController } from "../../test/libraryFixtures";
import { playbackStatus } from "../../test/playbackFixtures";
import { helperStatus, linkController, presentation } from "../../test/propresenterLinkFixtures";
import type { PlaybackController } from "../../hooks/usePlaybackInput";
import type { LightkeyStatus } from "../../types";
import { LightkeyStrip } from "../LightkeyStrip";
import { LIGHTKEY_SCAN_WORDS } from "../lightkeyScan";
import { PlaybackLiveLink } from "../PlaybackLiveLink";
import { PLAYBACK_SCAN_WORDS } from "../playbackScan";
import { PROPRESENTER_SCAN_WORDS } from "../proPresenterScan";
import { ProPresenterLinkBlock } from "../ProPresenterLinkBlock";
import { AddressBox } from "./AddressBox";
import { ScanHelp } from "./ScanHelp";
import { ScanProgress } from "./ScanProgress";
import { SCAN_WORDS } from "./ScanConnectButtons";
import { NO_SCAN, type ScanState } from "./scanState";

/**
 * Search finds a computer; Scan reads the songs (the Playback config's Scan Playback API / Library). So no Live Link block (Lighting,
 * Playback, ProPresenter) has a button, a button's title or a button's name with the word Scan, in any state; the identifiers stay `scan`.
 */
vi.mock("../../api", async (original) => ({
  ...(await original<typeof import("../../api")>()),
  getLightkeyStatus: vi.fn(),
  searchLightkeyAgain: vi.fn(),
  startLightkeyHelperInstall: vi.fn(),
  endLightkeyHelperInstall: vi.fn().mockResolvedValue({ ended: true }),
}));
const lightkey = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://192.0.2.10:8770", link: "unreachable", detail: "Cannot reach the Lightkey API.", active_count: null, recording: false, recording_song: null, replay: true, replay_blocked: null, replaying_song: null, message: null, ...over,
});
beforeEach(() => { vi.mocked(api.getLightkeyStatus).mockResolvedValue(lightkey({ discovery: { state: "searching", found: [], refusing: [] } })); });
afterEach(() => { vi.clearAllMocks(); window.localStorage.clear(); });

const SCAN = /\bscan(?:ning|ned)?\b/i;
const scanOf = (state: "idle" | "scanning" | "found" | "not_found", extra = {}) => ({ state, candidates: [], reason: null, current: 0, total: 0, ...extra });
const controller = (status = playbackStatus(), pending: PlaybackController["pending"] = null): PlaybackController => ({
  status, events: [], error: null, message: null, pending, saveArrangement: vi.fn(async () => true), save: vi.fn(), selectSource: vi.fn(), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(),
});
const fields = (state: ScanState) => ({ host: "192.168.1.60", port: "1025", onHost: vi.fn(), onPort: vi.fn(), held: { host: false, port: false }, errors: {}, busy: false, checksAgain: false, connectDisabled: false, connecting: false, onConnect: vi.fn(), scan: { state, start: vi.fn(), cancel: vi.fn(), choose: vi.fn() } });
const FOUND: ScanState = { state: "found", candidates: [{ host: "192.0.2.21", label: "Front Mac", port: 1025 }, { host: "192.0.2.34", label: "Stage Mac", port: 1025 }], current: 0, total: 0 };

/** Every word a person or a screen reader gets from a button: its text, its accessible name, its title and its wrapper's title. */
function buttonWords(root: HTMLElement): string[] {
  return within(root).queryAllByRole("button", { hidden: true }).flatMap((button) => [
    button.textContent ?? "", button.getAttribute("aria-label") ?? "", button.getAttribute("title") ?? "", button.parentElement?.getAttribute("title") ?? "",
  ]).filter(Boolean);
}

describe("Search, not Scan, in every Live Link block", () => {
  it("the pair's words are Search and Connect; while it runs, Searching… (cancel)", () => {
    expect(SCAN_WORDS).toEqual({ scan: "Search", scanning: "Searching\u2026 (cancel)", connect: "Connect", connecting: "Connecting\u2026" });
  });

  it.each([
    ["idle", controller()],
    ["searching", controller(playbackStatus({ scan: scanOf("scanning", { current: 12, total: 254, phase: "This computer", elapsed: 2 }) }), "scan")],
    ["found several", controller(playbackStatus({ scan: scanOf("found", { candidates: [{ name: "Front", host: "192.0.2.1" }, { name: "Stage", host: "192.0.2.2" }] }) }))],
    ["found nothing", controller(playbackStatus({ connected: false, active_source: "none", reason: "Disconnected. Choose Scan Network to find Playback.", scan: scanOf("not_found", { reason: "No Playback answered." }) }))],
    ["blocked by macOS", controller(playbackStatus({ scan: scanOf("not_found", { error_class: "permission_denied", reason: "Blocked." }) }))],
  ])("Playback (%s): no button says Scan, and neither does the block", async (_name, playback) => {
    render(<PlaybackLiveLink library={libraryController("ready")} playback={playback} />);
    const block = screen.getByTestId("playback-live-link");
    await userEvent.setup().click(within(block).getByRole("button", { name: "Show recent and found addresses" }));
    expect(buttonWords(block).filter((word) => SCAN.test(word))).toEqual([]);
    expect(block.textContent).not.toMatch(SCAN);
  });

  it.each([
    ["idle", NO_SCAN],
    ["searching", { state: "scanning", candidates: [], current: 64, total: 255, phase: "Looking for ProPresenter\u2026", elapsed: 3 } as ScanState],
    ["found several", FOUND],
    ["found nothing", { state: "not_found", candidates: [], current: 0, total: 0, reason: "Scan cancelled." } as ScanState],
    ["could not search", { state: "not_found", candidates: [], current: 0, total: 0, error: "failed", reason: "Something went wrong." } as ScanState],
  ])("ProPresenter (%s): no button says Scan, and neither does the block", (_name, state) => {
    render(<ProPresenterLinkBlock connection={fields(state)} link={linkController(helperStatus("ready"))} propresenter={presentation()} />);
    const block = screen.getByTestId("propresenter-live-link");
    expect(buttonWords(block).filter((word) => SCAN.test(word))).toEqual([]);
    expect(block.textContent).not.toMatch(SCAN);
  });

  it("Lighting (searching, then stopped): no button says Scan, and neither does the block", async () => {
    render(<LightkeyStrip lightingEnabled />);
    const button = await screen.findByRole("button", { name: "Searching\u2026 (cancel)" });
    const block = screen.getByTestId("lightkey-strip");
    expect(buttonWords(block).filter((word) => SCAN.test(word))).toEqual([]);
    expect(block.textContent).not.toMatch(SCAN);
    await userEvent.setup().click(button);
    expect(screen.getByRole("button", { name: "Search" })).toBeEnabled();
    expect(buttonWords(block).filter((word) => SCAN.test(word))).toEqual([]);
  });
});

describe("the words inside the blocks that name the action", () => {
  it("the progress line says Searching the network…, with what it does now and how far", () => {
    const { rerender } = render(<ScanProgress scan={{ state: "scanning", candidates: [], current: 64, total: 254, phase: "This computer", elapsed: 3.4 }} wording={PLAYBACK_SCAN_WORDS} />);
    expect(screen.getByTestId("scan-progress")).toHaveTextContent("Searching the network for Playback\u2026 This computer \u2014 Checked 64 of 254 addresses (3s)");
    rerender(<ScanProgress scan={{ state: "scanning", candidates: [], current: 64, total: 255, phase: "Looking for ProPresenter\u2026", elapsed: 3 }} wording={PROPRESENTER_SCAN_WORDS} />);
    expect(screen.getByTestId("scan-progress")).toHaveTextContent("Searching the network for ProPresenter\u2026 \u2014 Checked 64 of 255 addresses (3s)");
    rerender(<ScanProgress scan={{ state: "scanning", candidates: [], current: 0, total: 0 }} wording={LIGHTKEY_SCAN_WORDS} />);
    expect(screen.getByTestId("scan-progress")).toHaveTextContent("Searching the network for a Lighting computer\u2026");
    // Connect to one typed address says what it does, not Searching.
    rerender(<ScanProgress scan={{ state: "scanning", candidates: [], current: 0, total: 1, typed: true, phase: "Connecting to 192.0.2.50\u2026" }} wording={PLAYBACK_SCAN_WORDS} />);
    expect(screen.getByTestId("scan-progress")).toHaveTextContent("Connecting to 192.0.2.50\u2026");
  });

  it("the results say Found <n> … computers", () => {
    expect(PLAYBACK_SCAN_WORDS.choose(2)).toBe("Found 2 Playback computers. Choose one:");
    expect(PROPRESENTER_SCAN_WORDS.choose(3)).toBe("Found 3 ProPresenter computers. Choose one:");
    expect(LIGHTKEY_SCAN_WORDS.choose(2)).toBe("Found 2 Lighting computers. Choose one:");
  });

  it("the macOS permission help says to press Search once, and to search again", () => {
    render(<ScanHelp scan={{ state: "not_found", candidates: [], current: 0, total: 0, error: "permission_denied" }} wording={PLAYBACK_SCAN_WORDS} />);
    const help = screen.getByTestId("scan-error");
    expect(help).toHaveTextContent("press Search once and click Allow when macOS asks");
    expect(help).toHaveTextContent("Reopen StagePilot and search again.");
    expect(help.textContent).not.toMatch(SCAN);
  });

  it("a search that went wrong says it could not search the network", () => {
    render(<ScanHelp scan={{ state: "not_found", candidates: [], current: 0, total: 0, error: "failed", reason: "x", elapsed: 1 }} wording={PLAYBACK_SCAN_WORDS} />);
    expect(screen.getByTestId("scan-error")).toHaveTextContent("Couldn't search the network");
  });

  it("the address box's groups are Found and Recent, and its arrow is named for them", async () => {
    render(<AddressBox aria-label="Host" onChange={vi.fn()} recent={["192.0.2.9"]} scanned={[{ host: "192.0.2.21", label: "Front Mac (192.0.2.21)" }]} scope="playback" value="" />);
    await userEvent.setup().click(screen.getByRole("button", { name: "Show recent and found addresses" }));
    expect(screen.getByRole("listbox", { name: "Recent and found addresses" })).toBeVisible();
    expect(screen.getByRole("group", { name: "Found" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Scanned" })).toBeNull();
    expect(screen.getByRole("group", { name: "Recent" })).toBeInTheDocument();
  });
});
