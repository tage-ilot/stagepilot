import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { libraryController } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
import { helperStatus, linkController, localStatus, playlists } from "../test/propresenterLinkFixtures";
import { PlaybackApiPanel } from "./PlaybackApiPanel";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getProPresenterPlaylists: vi.fn(),
  previewAddSongs: vi.fn(),
}));
afterEach(() => vi.clearAllMocks());

const controller = (status = playbackStatus({ order_source: "library", order_library_note: null })): PlaybackController => ({
  status, events: [], error: null, message: null, pending: null, saveArrangement: vi.fn(async () => true), save: vi.fn().mockResolvedValue(true), selectSource: vi.fn().mockResolvedValue(true),
  scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(), refreshFromLibrary: vi.fn(), libraryRead: null,
});
const button = () => screen.queryByRole("button", { name: "Add to ProPresenter" });

describe("Add to ProPresenter in the Playback panel", () => {
  it("is under the songs Playback plays when the ProPresenter Link is ready, on another computer or this one", () => {
    const { unmount } = render(<PlaybackApiPanel library={libraryController("ready")} link={linkController(helperStatus("ready"))} playback={controller()} />);
    expect(within(screen.getByTestId("playback-library")).getByRole("button", { name: "Add to ProPresenter" })).toBeInTheDocument();
    unmount();
    render(<PlaybackApiPanel library={libraryController("ready")} link={linkController(localStatus())} playback={controller()} />);
    expect(button()).toBeInTheDocument();
  });

  it("is not drawn without the ProPresenter Link, and a computer without it sees nothing else about it", () => {
    const { container, rerender } = render(<PlaybackApiPanel library={libraryController("ready")} playback={controller()} />);
    expect(button()).toBeNull();
    for (const status of [null, helperStatus("not_installed"), helperStatus("needs_command"), helperStatus("not_answering"), helperStatus("outdated")]) {
      rerender(<PlaybackApiPanel library={libraryController("ready")} link={linkController(status)} playback={controller()} />);
      expect(button()).toBeNull();
    }
    expect(container.textContent).not.toMatch(/ProPresenter/);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("is not drawn without the Library Link", () => {
    render(<PlaybackApiPanel library={libraryController("not_installed")} link={linkController(helperStatus("ready"))} playback={controller()} />);
    expect(button()).toBeNull();
  });

  it("while Playback says a song is playing, the window opens but Add songs is off with that reason", async () => {
    vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
    render(<PlaybackApiPanel library={libraryController("ready")} link={linkController(helperStatus("ready"))} playback={controller(playbackStatus({ playing: true, order_source: "library", order_library_note: null }))} />);
    await userEvent.setup().click(button()!);
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    expect(within(screen.getByRole("dialog")).getByRole("button", { name: "Add songs" })).toBeDisabled();
    expect(screen.getByTestId("add-songs-playing")).toHaveTextContent("A song is playing. You can look at a preview now, but songs can only be added when it has ended.");
    expect(screen.getByTestId("add-songs-playing").closest("[role=status]")).not.toBeNull();
  });

  it("while StagePilot's own countdown runs, the same", async () => {
    vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
    render(<PlaybackApiPanel library={libraryController("ready")} link={linkController(helperStatus("ready"))} playback={controller()} timerRunning />);
    await userEvent.setup().click(button()!);
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    expect(screen.getByTestId("add-songs-playing")).toHaveTextContent("A song is playing.");
    expect(within(screen.getByRole("dialog")).getByRole("button", { name: "Add songs" })).toBeDisabled();
  });
});
