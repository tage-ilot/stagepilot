import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import { libraryController } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
import { PlaybackApiPanel } from "./PlaybackApiPanel";
import { SCAN_IS_RUNNING, SCAN_NEEDS_API, SCAN_NEEDS_STOPPED, SCAN_PLAYBACK_API, SCAN_PLAYBACK_LIBRARY, songScanButton } from "./playbackStatus";

// The Song order section has ONE song-scan button, always drawn, naming the best scan there is.
const controller = (overrides: Partial<PlaybackController> = {}): PlaybackController => ({
  status: playbackStatus({ order_source: "library", order_library_note: null }), events: [], error: null, message: null, pending: null,
  saveArrangement: vi.fn(async () => true), save: vi.fn().mockResolvedValue(true), selectSource: vi.fn().mockResolvedValue(true),
  scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(), refreshFromLibrary: vi.fn(), libraryRead: null, ...overrides,
});
const NOTE = "Not using the Library Link's order: Faithfulness is not in Playback's setlist.";
const disconnected = { sources: { playback_api: { connected: false, reason: "Playback is disconnected." }, midi: { connected: false, reason: "MIDI is disconnected." } } };
const songScan = () => document.querySelector("[data-part=song-scan]") as HTMLElement;
const scanButtons = () => screen.queryAllByRole("button", { name: /^Scan Playback|^Scan with Playback/ });

describe("songScanButton: the label and whether it can be pressed", () => {
  const base = { libraryUsable: false, libraryNote: null, apiConnected: true, playing: false, running: false, reading: false };
  it.each([
    ["only the Playback API", base, "api", SCAN_PLAYBACK_API, null],
    ["the Library Link usable", { ...base, libraryUsable: true }, "library", SCAN_PLAYBACK_LIBRARY, null],
    ["the Library Link usable but its order failed", { ...base, libraryUsable: true, libraryNote: NOTE }, "api", SCAN_PLAYBACK_API, null],
    ["nothing: not connected", { ...base, apiConnected: false }, "api", SCAN_PLAYBACK_API, SCAN_NEEDS_API],
    ["nothing: not connected, Library Link usable", { ...base, apiConnected: false, libraryUsable: true }, "library", SCAN_PLAYBACK_LIBRARY, SCAN_NEEDS_API],
    ["nothing: playing, API only", { ...base, playing: true }, "api", SCAN_PLAYBACK_API, SCAN_NEEDS_STOPPED],
    ["playing, Library Link usable (reading touches nothing)", { ...base, playing: true, libraryUsable: true }, "library", SCAN_PLAYBACK_LIBRARY, null],
    ["nothing: playing, Library Link order failed", { ...base, playing: true, libraryUsable: true, libraryNote: NOTE }, "api", SCAN_PLAYBACK_API, SCAN_NEEDS_STOPPED],
    ["nothing: a scan is running", { ...base, running: true }, "api", SCAN_PLAYBACK_API, SCAN_IS_RUNNING],
    ["nothing: a scan is running, Library Link usable", { ...base, running: true, libraryUsable: true }, "library", SCAN_PLAYBACK_LIBRARY, SCAN_IS_RUNNING],
  ] as const)("%s", (_name, state, kind, label, reason) => {
    expect(songScanButton(state)).toEqual({ kind, label, reason });
  });

  it("has one constant per label, with the words Hunter asked for", () => {
    expect(SCAN_PLAYBACK_API).toBe("Scan Playback API");
    expect(SCAN_PLAYBACK_LIBRARY).toBe("Scan Playback Library");
  });
});

describe("the one song-scan button in the Song order section", () => {
  it("API only: Scan Playback API, the confirm dialog follows its label, then it steps", () => {
    const playback = controller({ status: playbackStatus({ song_order: [], stale: true }) });
    render(<PlaybackApiPanel playback={playback} />);
    expect(scanButtons().map((b) => b.textContent)).toEqual(["Scan Playback API"]);
    fireEvent.click(screen.getByRole("button", { name: "Scan Playback API" }));
    const dialog = screen.getByRole("dialog", { name: "Scan Playback API?" });
    expect(dialog).toBeVisible();
    fireEvent.click(screen.getAllByRole("button", { name: "Scan Playback API" }).at(-1)!);
    expect(playback.discover).toHaveBeenCalledOnce();
    expect(playback.refreshFromLibrary).not.toHaveBeenCalled();
  });

  it("Library Link usable: Scan Playback Library reads it at once, no dialog, also while a song plays", () => {
    const playback = controller({ status: playbackStatus({ playing: true, order_source: "library", order_library_note: null }) });
    render(<PlaybackApiPanel library={libraryController("ready")} playback={playback} />);
    expect(scanButtons().map((b) => b.textContent)).toEqual(["Scan Playback Library"]);
    const button = screen.getByRole("button", { name: "Scan Playback Library" });
    expect(button).toBeEnabled();
    expect(button).not.toHaveAttribute("title");
    fireEvent.click(button);
    expect(playback.refreshFromLibrary).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("Library Link usable but its order failed: Scan Playback API, the plain line stays, and there is no second button", () => {
    render(<PlaybackApiPanel library={libraryController("ready")} playback={controller({ status: playbackStatus({ order_source: "none", order_library_note: NOTE, song_order: [], stale: true }) })} />);
    expect(scanButtons().map((b) => b.textContent)).toEqual(["Scan Playback API"]);
    expect(screen.getByTestId("order-library-note")).toHaveTextContent(NOTE);
    expect(songScan()).toHaveAttribute("data-scan", "api");
  });

  it.each([
    ["not connected", controller({ status: playbackStatus(disconnected) }), SCAN_NEEDS_API],
    ["playing, for the API scan", controller({ status: playbackStatus({ playing: true }) }), SCAN_NEEDS_STOPPED],
    ["a scan is running", controller({ status: playbackStatus({ discovery: "running" }) }), SCAN_IS_RUNNING],
    ["a scan was just asked for", controller({ pending: "discover" }), SCAN_IS_RUNNING],
  ])("nothing can run (%s): drawn, greyed out, not pressable, with the reason as title and screen-reader text", (_name, playback, reason) => {
    render(<PlaybackApiPanel playback={playback} />);
    const button = screen.getByRole("button", { name: "Scan Playback API" });
    expect(button).toBeVisible();
    expect(button).toBeDisabled();
    expect(button).toHaveClass("opacity-40");
    expect(button).toHaveAttribute("title", reason);
    expect(songScan()).toHaveAttribute("title", reason);
    expect(button).toHaveAccessibleDescription(reason);
    expect(screen.getByText(reason, { selector: ".sr-only" })).toBeInTheDocument();
    fireEvent.click(button);
    expect(playback.discover).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("the second button is gone in every state", () => {
    for (const status of [playbackStatus(), playbackStatus({ order_source: "none", order_library_note: NOTE }), playbackStatus({ playing: true, order_library_note: NOTE })]) {
      const { unmount } = render(<PlaybackApiPanel library={libraryController("ready")} playback={controller({ status })} />);
      expect(screen.queryByRole("button", { name: "Scan with Playback instead" })).toBeNull();
      expect(scanButtons()).toHaveLength(1);
      unmount();
    }
  });
});
