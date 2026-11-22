import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { JOB_POLL_MS } from "../hooks/useAddSongs";
import { libraryController, libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import { addPlan, addSong, helperStatus, jobOf, linkController, playlists, report } from "../test/propresenterLinkFixtures";
import type { Song } from "../types";
import { PresenterLinkSection } from "./PresenterLinkSection";
import { PRESENTER_LINK_OPEN_KEY } from "./proPresenterLinkState";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getProPresenterPlaylists: vi.fn(),
  lookupAddSongsTitles: vi.fn(),
  previewAddSongs: vi.fn(),
  applyAddSongs: vi.fn(),
  getAddSongsJob: vi.fn(),
}));

/** The Presenter Link section is a fold that starts closed; its header says whether the songs can be imported. */
const TITLES = ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"];
const REF = TITLES.map((_, i) => String(2000001 + i));
const playbackSongs = (count = 4, over: (index: number) => Parameters<typeof librarySong>[0] = () => ({})) => libraryPlan(
  TITLES.slice(0, count).map((title, i) => librarySongOf(title, i + 1, { library: librarySong({ title, position: i + 1, setlist_song_id: 2000001 + i, content_id: 20000 + i, key: "G", arrangement: "Default", sections: [{ name: "Verse 1", start: 0, end: 1 }], ...over(i) }) })),
);
const names = (count = 4): { songs: Song[] } => ({ songs: TITLES.slice(0, count).map((title, order) => ({ id: `s${order}`, title, duration_seconds: 300, order, is_generic: false, source_song_id: null })) });

type Setup = { songs?: number; plan?: { songs: Song[] } | null; library?: ReturnType<typeof libraryController> | undefined; playing?: boolean };
const view = (setup: Setup = {}) => {
  const lib = "library" in setup ? setup.library : libraryController("ready", { plan: playbackSongs(setup.songs ?? 4) });
  const link = linkController(helperStatus("ready"));
  return render(<PresenterLinkSection library={lib} link={link} plan={"plan" in setup ? setup.plan : names()} playing={setup.playing ?? false} />);
};
const toggle = () => screen.getByRole("button", { name: /^Presenter Link/ });
const summary = () => screen.getByTestId("presenter-link-summary");
const body = () => screen.getByTestId("presenter-link-section").querySelector("[data-part=region]") as HTMLElement;
const ADD = { name: "Import songs to ProPresenter" };
/** Waits until the playlists are in (the Playlist box has its options). */
const loaded = () => screen.findByRole("option", { name: "Sunday Morning Service" });

beforeEach(() => {
  window.localStorage.clear();
  vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
  vi.mocked(api.lookupAddSongsTitles).mockImplementation(async (titles) => ({ titles: titles.map((title) => ({ title, state: "found" as const })) }));
  vi.mocked(api.applyAddSongs).mockResolvedValue({ job_id: "fedcba9876543210fedcba9876543210", plan_id: "p", state: "running" });
  vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf());
});
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); window.localStorage.clear(); });

describe("the fold", () => {
  it("starts folded: the heading is the toggle (aria-expanded, aria-controls) and the body is inert", async () => {
    view();
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(toggle()).toHaveAttribute("aria-controls", body().id);
    expect(screen.getByRole("heading", { name: "Presenter Link" })).toBeInTheDocument();
    expect(body()).toHaveAttribute("data-state", "closed");
    expect(body().firstElementChild).toHaveAttribute("inert");
    await loaded();
  });

  it("opens with the toggle and remembers the choice in this browser, under its own key", async () => {
    const user = userEvent.setup();
    const { unmount } = view();
    await user.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(body()).toHaveAttribute("data-state", "open");
    expect(body().firstElementChild).not.toHaveAttribute("inert");
    expect(window.localStorage.getItem(PRESENTER_LINK_OPEN_KEY)).toBe("true");
    unmount();
    view();
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle());
    expect(window.localStorage.getItem(PRESENTER_LINK_OPEN_KEY)).toBe("false");
    expect(body().firstElementChild).toHaveAttribute("inert");
  });

  it("keeps everything inside while folded: the playlist, the section and the ticks stay, and nothing is asked again", async () => {
    const user = userEvent.setup();
    view();
    await user.click(toggle());
    await loaded();
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-SUNDAY");
    await user.selectOptions(screen.getByLabelText("Section"), "Message");
    await user.click(screen.getByRole("checkbox", { name: "Battle Belongs" }));
    const asked = vi.mocked(api.getProPresenterPlaylists).mock.calls.length;
    await user.click(toggle());
    expect(body().firstElementChild).toHaveAttribute("inert");
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    await user.click(toggle());
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    expect(screen.getByLabelText("Section")).toHaveValue("Message");
    expect(screen.getByRole("checkbox", { name: "Battle Belongs" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("checkbox", { name: "Holy Forever" })).toHaveAttribute("aria-checked", "true");
    // The list's state is not made again by folding: the playlists were read once and the live region is the same node.
    expect(vi.mocked(api.getProPresenterPlaylists).mock.calls.length).toBe(asked);
    expect(screen.getByTestId("add-songs-live")).toBeInTheDocument();
    expect(toggle().closest("[data-testid=presenter-link-section]")).toBe(screen.getByTestId("presenter-link-section"));
  });

  it("the live region is outside the folded body, so a closed section is not silent", () => {
    view();
    expect(body()).not.toContainElement(screen.getByTestId("add-songs-live"));
    expect(screen.getByTestId("add-songs-live")).toHaveAttribute("aria-live", "polite");
  });

  it("the summary line is a plain status, not a live region", () => {
    view();
    expect(summary().closest("[aria-live]")?.getAttribute("data-testid")).not.toBe("presenter-link-summary");
    expect(summary()).not.toHaveAttribute("aria-live");
  });
});

describe("the header says whether the songs can be imported", () => {
  it("4 songs ready to import (Playback's songs), and the singular", () => {
    const { unmount } = view();
    expect(summary()).toHaveTextContent("4 songs ready to import");
    unmount();
    view({ songs: 1, plan: names(1) });
    expect(summary()).toHaveTextContent("1 song ready to import");
  });

  it("with no playlist chosen the songs are still importable, and the playlist is not mentioned", async () => {
    view();
    await loaded();
    expect(summary()).toHaveTextContent("4 songs ready to import");
    expect(summary().textContent).not.toMatch(/playlist/i);
  });

  it("Planning Center names count the same way, from the cards: the ones ProPresenter does not have cannot be imported", async () => {
    vi.mocked(api.lookupAddSongsTitles).mockImplementation(async (titles) => ({ titles: titles.map((title) => ({ title, state: title === "Firm Foundation" ? "missing" as const : "found" as const })) }));
    view({ library: undefined });
    await waitFor(() => expect(summary()).toHaveTextContent("3 of 4 songs can be imported"));
    expect(within(screen.getByTestId("presenter-link-section")).getAllByTestId("add-songs-blocked")).toHaveLength(1);
  });

  it("a song Playback gave no number is a card that cannot be imported, so it is counted out", () => {
    view({ plan: null, songs: 4, library: libraryController("ready", { plan: playbackSongs(4, (i) => (i === 3 ? { setlist_song_id: null } : {})) }) });
    expect(summary()).toHaveTextContent("3 of 4 songs can be imported");
  });

  it("No songs can be imported", async () => {
    vi.mocked(api.lookupAddSongsTitles).mockImplementation(async (titles) => ({ titles: titles.map((title) => ({ title, state: "missing" as const })) }));
    view({ library: undefined });
    await waitFor(() => expect(summary()).toHaveTextContent("No songs can be imported"));
  });

  it("Not while a song plays", () => {
    view({ playing: true });
    expect(summary()).toHaveTextContent("Not while a song plays");
  });

  it("No songs in the plan", () => {
    view({ library: undefined, plan: { songs: [] } });
    expect(summary()).toHaveTextContent("No songs in the plan");
  });

  it("follows the playing flag and the plan while the section stays where it is", () => {
    const { rerender } = view();
    const link = linkController(helperStatus("ready"));
    const lib = libraryController("ready", { plan: playbackSongs(4) });
    rerender(<PresenterLinkSection library={lib} link={link} plan={names()} playing />);
    expect(summary()).toHaveTextContent("Not while a song plays");
    rerender(<PresenterLinkSection library={lib} link={link} plan={names()} />);
    expect(summary()).toHaveTextContent("4 songs ready to import");
  });
});

describe("work in progress is never hidden", () => {
  const stuck = () => addPlan([
    addSong({ ref_id: REF[0]!, title: "Holy Forever" }),
    addSong({ ref_id: REF[1]!, title: "Battle Belongs", action: "blocked", reason: "Not in ProPresenter." }),
    addSong({ ref_id: REF[2]!, title: "Gratitude" }),
    addSong({ ref_id: REF[3]!, title: "Firm Foundation" }),
  ]);
  const clean = () => addPlan(TITLES.map((title, i) => addSong({ ref_id: REF[i]!, title })));
  /** Chooses the options in the (closed) body and presses the one button: the way a job starts. */
  async function start(user: ReturnType<typeof userEvent.setup>) {
    await loaded();
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-SUNDAY");
    await user.click(screen.getByRole("button", ADD));
  }

  it("a preview that stopped opens the folded section, leaves the remembered choice alone, and does not close by itself", async () => {
    const user = userEvent.setup();
    vi.mocked(api.previewAddSongs).mockResolvedValue(stuck());
    view();
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    await start(user);
    await screen.findByTestId("add-songs-preview");
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(body().firstElementChild).not.toHaveAttribute("inert");
    expect(summary()).toHaveTextContent("1 can't be imported yet");
    expect(window.localStorage.getItem(PRESENTER_LINK_OPEN_KEY)).toBeNull();
    // Only the person's press closes it again.
    await user.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(body().firstElementChild).toHaveAttribute("inert");
  });

  it("the import running: Importing…, open, and the toggle cannot fold it away", async () => {
    const user = userEvent.setup();
    vi.mocked(api.previewAddSongs).mockResolvedValue(clean());
    view();
    await start(user);
    await screen.findByTestId("add-songs-progress");
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(summary()).toHaveTextContent("Importing…");
    await user.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(body().firstElementChild).not.toHaveAttribute("inert");
    expect(screen.getByTestId("add-songs-progress")).toBeInTheDocument();
  });

  it("the preview being made: Importing…, open", async () => {
    const user = userEvent.setup();
    let release: (value: ReturnType<typeof clean>) => void = () => undefined;
    vi.mocked(api.previewAddSongs).mockReturnValue(new Promise((resolve) => { release = resolve; }));
    view();
    await start(user);
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(summary()).toHaveTextContent("Importing…");
    await act(async () => { release(stuck()); });
    await screen.findByTestId("add-songs-preview");
  });

  it("a job that has just ended stays open and says how it ended; folding it is the person's own press", async () => {
    const user = userEvent.setup();
    vi.mocked(api.previewAddSongs).mockResolvedValue(clean());
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report({ songs: TITLES.map((title, i) => ({ ref_id: REF[i]!, title, status: "added" as const, file: title, text: "Added.", code: null, guesses: [] })) }) }));
    view();
    await start(user);
    await screen.findByTestId("add-songs-result", undefined, { timeout: JOB_POLL_MS * 5 });
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(summary()).toHaveTextContent("Done: 4 added");
    expect(window.localStorage.getItem(PRESENTER_LINK_OPEN_KEY)).toBeNull();
    await user.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(summary()).toHaveTextContent("Done: 4 added");
  });

  it("a job that stopped before it changed anything: Stopped: nothing changed", async () => {
    const user = userEvent.setup();
    vi.mocked(api.previewAddSongs).mockResolvedValue(clean());
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report({ outcome: "stopped", message: "Stopped.", playlist_changed: false }) }));
    view();
    await start(user);
    await screen.findByTestId("add-songs-result", undefined, { timeout: JOB_POLL_MS * 5 });
    expect(summary()).toHaveTextContent("Stopped: nothing changed");
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
  });

  it("a problem opens the folded section and the header says Problem", async () => {
    const user = userEvent.setup();
    vi.mocked(api.previewAddSongs).mockRejectedValueOnce(new api.ApiError("That playlist is not in ProPresenter any more.", 404, "playlist_not_found"));
    view();
    await start(user);
    await screen.findByTestId("add-songs-problem");
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(summary()).toHaveTextContent("Problem");
    expect(window.localStorage.getItem(PRESENTER_LINK_OPEN_KEY)).toBeNull();
  });
});
