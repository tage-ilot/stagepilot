import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { libraryController } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
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

/** Adding songs to ProPresenter lives in the ProPresenter (Presentation) panel's Presenter Link section; the Playback panel only shows what Playback plays. */
describe("the Playback panel has nothing about adding songs to ProPresenter", () => {
  it("shows the songs Playback plays and no Import songs to ProPresenter (once Add to ProPresenter) button, with the library ready, or playing", () => {
    for (const status of [undefined, playbackStatus({ playing: true, order_source: "library", order_library_note: null })]) {
      const { container, unmount } = render(<PlaybackApiPanel library={libraryController("ready")} playback={controller(status)} />);
      expect(screen.getByTestId("playback-library")).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /Add to ProPresenter|Import songs to ProPresenter/ })).toBeNull();
      expect(screen.queryByTestId("add-to-propresenter")).toBeNull();
      expect(container.textContent).not.toMatch(/ProPresenter/);
      expect(screen.queryByRole("dialog")).toBeNull();
      unmount();
    }
    expect(api.getProPresenterPlaylists).not.toHaveBeenCalled();
    expect(api.previewAddSongs).not.toHaveBeenCalled();
  });
});
