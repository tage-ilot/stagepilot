import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import { libraryController } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
import { PlaybackApiPanel } from "./PlaybackApiPanel";

// "Scan Playback Songs" when the Library Link works reads it instead of stepping through Playback.
const controller = (overrides: Partial<PlaybackController> = {}): PlaybackController => ({
  status: playbackStatus({ order_source: "library", order_library_note: null }), events: [], error: null, message: null, pending: null,
  saveArrangement: vi.fn(async () => true), save: vi.fn().mockResolvedValue(true), selectSource: vi.fn().mockResolvedValue(true),
  scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(), refreshFromLibrary: vi.fn(), libraryRead: null, ...overrides,
});
const NOTE = "Not using the Library Link's order: Faithfulness is not in Playback's setlist.";
const failed = (extra = {}) => playbackStatus({ order_source: "none", order_library_note: NOTE, song_order: [], stale: true, ...extra });

describe("Scan Playback Songs with the Library Link working", () => {
  it("reads the Library Link at one press: no confirm dialog, nothing stepped, the label stays", () => {
    const playback = controller();
    render(<PlaybackApiPanel library={libraryController("ready")} playback={playback} />);
    fireEvent.click(screen.getByRole("button", { name: "Scan Playback Songs" }));
    expect(playback.refreshFromLibrary).toHaveBeenCalledOnce();
    expect(playback.discover).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Scan with Playback instead" })).not.toBeInTheDocument();
  });

  it("says in the help text that the order comes from the Library Link, and not that Playback is stepped through", () => {
    render(<PlaybackApiPanel library={libraryController("ready")} playback={controller()} />);
    expect(screen.getByTestId("order-help")).toHaveTextContent("The song order comes from the Library Link");
    expect(screen.queryByText(/steps through your Playback songs/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Playback must be stopped/)).not.toBeInTheDocument();
  });

  it("an outdated helper still works the same way", () => {
    const playback = controller();
    render(<PlaybackApiPanel library={libraryController("outdated")} playback={playback} />);
    fireEvent.click(screen.getByRole("button", { name: "Scan Playback Songs" }));
    expect(playback.refreshFromLibrary).toHaveBeenCalledOnce();
  });

  it("is there and works while Playback is playing: it only reads the Library Link", () => {
    const playback = controller({ status: playbackStatus({ playing: true, order_source: "library", order_library_note: null }) });
    render(<PlaybackApiPanel library={libraryController("ready")} playback={playback} />);
    expect(screen.queryByText("Stop Playback to scan Playback songs.")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Scan Playback Songs" }));
    expect(playback.refreshFromLibrary).toHaveBeenCalledOnce();
  });

  it("shows the loading state on the button itself, keeps its text, and cannot be pressed again while it reads", () => {
    const playback = controller({ pending: "library" });
    render(<PlaybackApiPanel library={libraryController("ready")} playback={playback} />);
    const button = screen.getByRole("button", { name: "Scan Playback Songs" });
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).toHaveAttribute("data-loading", "true");
    expect(button).toHaveTextContent("Scan Playback Songs");
    fireEvent.click(button);
    expect(playback.refreshFromLibrary).not.toHaveBeenCalled();
  });

  it("is not loading when nothing is being read", () => {
    render(<PlaybackApiPanel library={libraryController("ready")} playback={controller()} />);
    expect(screen.getByRole("button", { name: "Scan Playback Songs" })).not.toHaveAttribute("aria-busy");
  });

  it("confirms the read in the Song order section, in a polite live region, once the answer is in", () => {
    const view = render(<PlaybackApiPanel library={libraryController("ready")} playback={controller({ pending: "library" })} />);
    expect(screen.getByTestId("library-read-line")).toBeEmptyDOMElement();
    view.rerender(<PlaybackApiPanel library={libraryController("ready")} playback={controller({ libraryRead: 2 })} />);
    const line = screen.getByTestId("library-read-line");
    expect(line).toHaveTextContent("Read 2 songs from the Library Link");
    expect(line).toHaveAttribute("role", "status");
    expect(line).toHaveAttribute("aria-live", "polite");
    view.rerender(<PlaybackApiPanel library={libraryController("ready")} playback={controller({ libraryRead: 1, status: playbackStatus({ order_source: "library", order_library_note: null, song_order: [101] }) })} />);
    expect(screen.getByTestId("library-read-line")).toHaveTextContent("Read 1 song from the Library Link");
  });

  it("a failed condition: the plain line, and the stepping scan offered quietly as the backup", () => {
    const playback = controller({ status: failed() });
    render(<PlaybackApiPanel library={libraryController("ready")} playback={playback} />);
    expect(screen.getByTestId("order-library-note")).toHaveTextContent(NOTE);
    expect(screen.getByTestId("library-read-line")).toBeEmptyDOMElement();
    const backup = screen.getByRole("button", { name: "Scan with Playback instead" });
    expect(backup).toHaveAttribute("data-variant", "secondary");
    // The first button still tries the Library Link.
    fireEvent.click(screen.getByRole("button", { name: "Scan Playback Songs" }));
    expect(playback.refreshFromLibrary).toHaveBeenCalledOnce();
    expect(playback.discover).not.toHaveBeenCalled();
    // The backup opens today's confirm, with today's words, and then steps as before.
    fireEvent.click(backup);
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("must stay stopped");
    expect(dialog).toHaveTextContent("Nothing will be played.");
    fireEvent.click(screen.getAllByRole("button", { name: "Scan Playback Songs" }).at(-1)!);
    expect(playback.discover).toHaveBeenCalledOnce();
  });

  it("a failed condition while Playback plays: the backup is not offered (stepping needs Playback stopped)", () => {
    render(<PlaybackApiPanel library={libraryController("ready")} playback={controller({ status: failed({ playing: true }) })} />);
    expect(screen.getByTestId("order-library-note")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Scan with Playback instead" })).not.toBeInTheDocument();
  });

  it.each(["not_installed", "needs_key", "unreachable"] as const)("Library Link %s: exactly as before (confirm, then step through Playback)", (state) => {
    const playback = controller({ status: playbackStatus({ song_order: [], stale: true }) });
    render(<PlaybackApiPanel library={libraryController(state)} playback={playback} />);
    expect(screen.getByText(/steps through your Playback songs once/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Scan Playback Songs" }));
    expect(playback.refreshFromLibrary).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Scan with Playback instead" })).not.toBeInTheDocument();
  });

  it("an old copy of the Library Link's answer is not usable: the button steps as before", () => {
    const library = libraryController("ready");
    const playback = controller({ status: playbackStatus({ song_order: [], stale: true }) });
    render(<PlaybackApiPanel library={{ ...library, status: { ...library.status!, stale: true } }} playback={playback} />);
    fireEvent.click(screen.getByRole("button", { name: "Scan Playback Songs" }));
    expect(playback.refreshFromLibrary).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeVisible();
  });

  it("Playback not connected: no button, the same explanation as before", () => {
    const status = playbackStatus({ sources: { playback_api: { connected: false, reason: "Playback is disconnected." }, midi: { connected: false, reason: "MIDI is disconnected." } } });
    render(<PlaybackApiPanel library={libraryController("ready")} playback={controller({ status })} />);
    expect(screen.getByText("Connect to Playback first.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Scan Playback Songs" })).not.toBeInTheDocument();
  });
});
