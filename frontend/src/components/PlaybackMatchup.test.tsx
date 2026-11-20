import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { libraryController, libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
import type { LibraryPlan, LibrarySong, PlaybackStatusResponse } from "../types";
import { PlaybackApiPanel } from "./PlaybackApiPanel";
import { PlaybackSaveBar, usePlaybackDraft } from "./PlaybackSaveFooter";
import { usePlaybackSave, type PlaybackMidiStep } from "./usePlaybackSave";

const noMidi: PlaybackMidiStep = { pending: false, count: 0, valid: true, snapshot: "", apply: async () => true, revert: () => undefined };
const TITLES = ["One Thing Remains", "So Much", "Easy To Worship", "Way Maker"];
const PLAN = TITLES.map((title, i) => ({ id: `p${i + 1}`, title, duration_seconds: 389 }));
const pb = (title: string, position: number, over: Partial<LibrarySong> = {}) => librarySong({ title, position, setlist_song_id: 2000000 + position, countdown_length_s: 389, ...over });
const IDS = [2000001, 2000002, 2000003, 2000004, 2000005];

/** Playback's setlist: So Much first, One Thing Remains second, Easy To Worship (a key lower), Waymaker (alike, not the same), and a song only Playback has. */
const sampleLibrary = (over: Partial<LibraryPlan> = {}): LibraryPlan => libraryPlan([
  librarySongOf("One Thing Remains", 1, { library: pb("One Thing Remains", 2) }),
  librarySongOf("So Much", 2, { library: pb("So Much", 1), confidence: "close" }),
  librarySongOf("Easy To Worship", 3, { library: pb("Easy To Worship", 3, { key: "Ab" }), plan_key: "A" }),
  librarySongOf("Way Maker", 4, { status: "uncertain", confidence: "uncertain", library: null, candidate: pb("Waymaker", 4) }),
], { playback_only: [pb("Background Music", 5, { artwork: null, countdown_length_s: 120 })], ...over });

const status = (extra: Partial<PlaybackStatusResponse> = {}) => playbackStatus({
  song_order: IDS, song_count: 5, plan_song_count: 4, song_lengths: [100, 200, 300, 400, 120], plan_songs: PLAN, skipped_song_ids: [], skipped_plan_song_ids: [], ...extra,
});

function Host({ initial, library, saveArrangement }: { initial?: Partial<PlaybackStatusResponse>; library?: PlaybackLibraryController; saveArrangement?: PlaybackController["saveArrangement"] }) {
  const [current, setStatus] = useState(status(initial));
  const playback: PlaybackController = {
    status: current, events: [], error: null, message: null, pending: null,
    save: vi.fn(async () => true), selectSource: vi.fn(async () => true), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(),
    saveArrangement: async (playbackSkips, planSkips) => {
      const ok = saveArrangement ? await saveArrangement(playbackSkips, planSkips) : true;
      if (ok) setStatus((now) => ({ ...now, skipped_song_ids: playbackSkips, skipped_plan_song_ids: planSkips }));
      return ok;
    },
  };
  const draft = usePlaybackDraft(playback);
  const bar = usePlaybackSave(draft, playback, noMidi);
  return <><PlaybackApiPanel playback={playback} draft={draft} library={library} /><PlaybackSaveBar save={bar} /></>;
}

const withLibrary = (plan: LibraryPlan | null, over: Partial<PlaybackLibraryController> = {}) => libraryController("ready", { plan, ...over });
const rows = () => within(screen.getByRole("list", { name: "Song pairs" })).getAllByRole("listitem", { hidden: true }).filter((li) => li.classList.contains("arr-line"));
const skip = (name: string) => screen.getByRole("switch", { name });

describe("the matchup without the library", () => {
  it("is the saved scan alone, in the new ledger, when there is no library at all", () => {
    render(<Host initial={{ song_order: [900, 17, 42, 55, 70], song_lengths: [388, 395, 380, 400, 700], plan_song_count: 4 }} />);
    expect(screen.queryByTestId("playback-setlist")).not.toBeInTheDocument();
    expect(rows()[0]).toHaveTextContent("Playback song 1");
    expect(rows()[0]!.querySelector("[data-stamp]")).toHaveTextContent("Matched");
    expect(skip("Skip Playback song 5")).toBeInTheDocument();
    expect(skip("Skip One Thing Remains")).toBeInTheDocument();
  });

  it("is the same when the Library Link is not installed, needs its command or is not answering, even with a plan the library once held", () => {
    for (const state of ["not_installed", "needs_key", "unreachable"] as const) {
      const { unmount } = render(<Host initial={{ song_order: [900, 17, 42, 55, 70] }} library={libraryController(state, { plan: sampleLibrary() })} />);
      expect(screen.queryByTestId("playback-setlist")).not.toBeInTheDocument();
      expect(rows()[0]).toHaveTextContent("Playback song 1");
      expect(rows()[0]).not.toHaveTextContent("So Much");
      unmount();
    }
  });

  it("shows nothing before a scan when there is no library to read", () => {
    render(<Host initial={{ song_order: [] }} />);
    expect(screen.queryByRole("group", { name: "Song arrangement" })).not.toBeInTheDocument();
    render(<Host initial={{ song_order: [] }} library={libraryController("not_installed")} />);
    expect(screen.queryByRole("group", { name: "Song arrangement" })).not.toBeInTheDocument();
  });
});

describe("the matchup with the library", () => {
  it("shows Playback's own titles, cover, key, length and bar, with a stamp and plain words per row", () => {
    render(<Host library={withLibrary(sampleLibrary())} />);
    expect(rows()).toHaveLength(5);
    const first = rows()[0]!;
    expect(first).toHaveTextContent("Song 1 in plan");
    expect(first).toHaveTextContent("Song 2 in Playback");
    expect(first.querySelector(".arr-pb")).toHaveTextContent("One Thing Remains");
    const cover = first.querySelector("img")!;
    expect(cover).toHaveAttribute("width", "28");
    expect(cover).toHaveAttribute("height", "28");
    expect(cover.getAttribute("src")).toContain("/api/v1/playback-library/songs/2000002/artwork");
    expect(first.querySelector("[data-testid=library-bar]")).toBeInTheDocument();
    expect(first.querySelector(".arr-pb")).toHaveTextContent("Ab 65 BPM");
    // The key and the tempo are one square chip each (the bare key letter, as everywhere), with no dot between them.
    const facts = within(first.querySelector(".arr-pb") as HTMLElement).getByRole("list", { name: "Key Ab, 65 BPM", hidden: true });
    expect(within(facts).getAllByRole("listitem", { hidden: true }).map((chip) => chip.textContent)).toEqual(["Ab", "65 BPM"]);
    expect(first.textContent).not.toMatch(/Key Ab|\u00B7/);
    expect(first.querySelector("[title]")).toBeTruthy();
    const stamps = rows().map((row) => row.querySelector("[data-stamp]")!.textContent);
    expect(stamps).toEqual(["!Check order", "!Check order", "!Check key", "?Check title", "+Only in Playback"]);
  });

  it("states a key difference, a close match and an uncertain pair in words, with both titles", () => {
    render(<Host library={withLibrary(sampleLibrary())} />);
    expect(rows()[2]).toHaveTextContent("Plan says key A, Playback plays Ab.");
    expect(rows()[3]).toHaveTextContent("The titles are alike but not the same: the plan says “Way Maker”, Playback has “Waymaker”.");
    expect(rows()[3]!.querySelector(".arr-pb")).toHaveTextContent("Waymaker");
    expect(rows()[3]!.querySelector(".arr-pb img")).toBeNull();
    expect(rows()[1]!.querySelector("[data-stamp]")).toHaveTextContent("Check order");
  });

  it("uses the existing wording for a length more than 15 s apart", () => {
    const lib = libraryPlan([librarySongOf("One Thing Remains", 1, { plan_seconds: 408, library: pb("One Thing Remains", 1, { countdown_length_s: 442 }) })]);
    render(<Host initial={{ song_order: [2000001], plan_songs: [PLAN[0]!], plan_song_count: 1, song_count: 1, song_lengths: [442] }} library={withLibrary(lib)} />);
    expect(rows()[0]).toHaveTextContent("Playback counts this song down to 7:22; the plan says 6:48.");
    expect(rows()[0]!.querySelector("[data-stamp]")).toHaveTextContent("Check length");
  });

  it("explains an order difference with the saved scan, and a skip on the song only Playback has fixes it", async () => {
    const user = userEvent.setup();
    const lib = libraryPlan([
      librarySongOf("One Thing Remains", 1, { library: pb("One Thing Remains", 1) }),
      librarySongOf("So Much", 2, { library: pb("So Much", 3) }),
      librarySongOf("Easy To Worship", 3, { library: pb("Easy To Worship", 4) }),
    ], { playback_only: [pb("Background Music", 2)] });
    render(<Host initial={{ song_order: [2000001, 2000002, 2000003, 2000004], plan_songs: PLAN.slice(0, 3), plan_song_count: 3, song_count: 4, song_lengths: [389, 120, 389, 389] }} library={withLibrary(lib)} />);
    // The long "Songs never change places..." sentence is in docs/playback-api.md; the row's own note and the Check order stamp say what is wrong.
    expect(screen.queryByTestId("matchup-order-note")).not.toBeInTheDocument();
    expect(screen.queryByText(/Songs never change places/)).not.toBeInTheDocument();
    expect(rows()[2]).toHaveTextContent("With the skips as they are, Playback song 2, Background Music, will start this plan song's timer, not So Much.");
    expect(screen.getByTestId("arrangement-badge")).toHaveTextContent("2 to check");
    await user.click(skip("Skip Playback song 2, Background Music"));
    expect(screen.queryByTestId("matchup-order-note")).not.toBeInTheDocument();
    expect(rows().map((row) => row.querySelector("[data-stamp]")!.textContent)).toEqual(["✓Matched", "⌀Skipped", "✓Matched", "✓Matched"]);
    expect(screen.getByTestId("arrangement-badge")).toHaveTextContent("Lines up");
    expect(document.querySelector("[data-held-marker]")).toHaveTextContent("Not saved yet");
  });

  it("saves the skip by the scan's own song number when Save configuration is pressed", async () => {
    const user = userEvent.setup();
    const saveArrangement = vi.fn(async () => true);
    render(<Host library={withLibrary(sampleLibrary())} saveArrangement={saveArrangement} />);
    await user.click(skip("Skip Playback song 5, Background Music"));
    expect(saveArrangement).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    await act(async () => undefined);
    expect(saveArrangement).toHaveBeenCalledWith([2000005], []);
  });

  it("reads each row as one sentence for a screen reader and hides the drawn copy", () => {
    render(<Host library={withLibrary(sampleLibrary())} />);
    const list = screen.getByRole("list", { name: "Song pairs" });
    const sentence = within(list).getByText(/^Plan song 3, Easy To Worship goes with Playback song 3, Easy To Worship\. Check key\. Plan says key A, Playback plays Ab\./);
    expect(sentence).toHaveClass("sr-only");
    expect(within(list).getByText("Playback song 5, Background Music has no plan song. Only in Playback. Playback has this song; the plan does not.")).toHaveClass("sr-only");
    const row = rows()[2]!;
    expect(row.querySelector(".arr-head")).toHaveAttribute("aria-hidden", "true");
    expect(row.querySelector(".arr-notes")!.closest("[aria-hidden=true]")).toBe(row.querySelector(".arr-head"));
    expect(row.querySelector(".arr-link")).toHaveAttribute("aria-hidden", "true");
    // The two switches stay real controls with names that say whose song they skip.
    expect(skip("Skip Easy To Worship")).toBeVisible();
    expect(skip("Skip Playback song 3, Easy To Worship")).toBeVisible();
  });

  it("names the setlist it uses and says it is the one on the plan's date", () => {
    render(<Host library={withLibrary(sampleLibrary())} />);
    const line = screen.getByTestId("playback-setlist");
    expect(line).toHaveTextContent("Playback setlist");
    expect(line).toHaveTextContent("Set, 2026-11-22, 4 songs.");
    expect(line).not.toHaveTextContent("It is the one on the plan's date");
    expect(within(line).getByRole("status")).toBeInTheDocument();
    expect(screen.getByTestId("arrangement-summary")).toHaveTextContent("3 of 4 service songs go with a Playback song.");
    expect(screen.getByTestId("arrangement-summary")).toHaveTextContent("1 is only in Playback.");
    expect(screen.getByTestId("arrangement-summary")).toHaveTextContent("1 is only alike by title.");
  });

  it("with no scan it is a comparison only: no switches, and and no sentence about it", () => {
    render(<Host initial={{ song_order: [], song_count: 0 }} library={withLibrary(sampleLibrary())} />);
    expect(rows()).toHaveLength(5);
    expect(screen.queryAllByRole("switch").filter((s) => /^Skip /.test(s.getAttribute("aria-label") ?? ""))).toHaveLength(0);
    expect(screen.queryByTestId("matchup-readonly-note")).not.toBeInTheDocument();
    expect(rows()[0]!.querySelector("[data-stamp]")).toHaveTextContent("Matched");
  });

  it("with an out-of-date scan it is the same comparison, not the scan", () => {
    render(<Host initial={{ stale: true }} library={withLibrary(sampleLibrary())} />);
    expect(screen.queryByTestId("matchup-readonly-note")).not.toBeInTheDocument();
    expect(rows()[0]).toHaveTextContent("One Thing Remains");
  });

  it("falls back to the plain scan view, and says why, when the scan is of another setlist", () => {
    render(<Host initial={{ song_order: [11, 12, 13, 14, 15, 16] }} library={withLibrary(sampleLibrary())} />);
    expect(screen.getByTestId("matchup-scan-note")).toHaveTextContent("The saved scan does not match the Playback setlist above");
    expect(rows()[0]).toHaveTextContent("Playback song 1");
    expect(screen.getByTestId("playback-setlist")).toBeInTheDocument();
  });

  it("lines a scan up by place, and says so, when its numbers share nothing with the library's", () => {
    render(<Host initial={{ song_order: [11, 12, 13, 14, 15] }} library={withLibrary(sampleLibrary())} />);
    expect(screen.getByTestId("matchup-join-note")).toHaveTextContent("by place");
    expect(skip("Skip Playback song 5, Background Music")).toBeInTheDocument();
  });

  it("ignores a library answer about another plan", () => {
    render(<Host library={withLibrary(sampleLibrary({ songs: sampleLibrary().songs.slice(0, 2) }))} />);
    expect(screen.queryByTestId("playback-setlist")).not.toBeInTheDocument();
    expect(rows()[0]).toHaveTextContent("Playback song 1");
  });

  it("does not use the Library Link for anything while it is not active, whatever the plan answer holds", () => {
    const library = libraryController("ready", { plan: sampleLibrary() });
    library.status = { ...library.status!, active: false };
    render(<Host library={library} />);
    expect(rows()[0]).toHaveTextContent("Playback song 1");
  });
});

describe("the Playback setlist chooser", () => {
  const none = (over: Partial<LibraryPlan> = {}): LibraryPlan => ({
    ...libraryPlan([]), state: "no_setlist", setlist: null, plan_date: "2026-11-29", message: "Playback has no setlist for 2026-11-29; its newest is Saturday Night from 2026-11-21.",
    newest: { id: 7, title: "Saturday Night", date: "2026-11-21", songs: 5 },
    setlists: [{ id: 7, title: "Saturday Night", date: "2026-11-21", songs: 5 }, { id: 6, title: "Old Set", date: "2026-11-08", songs: 1 }, { id: null, title: "No number", date: null, songs: 0 }],
    ...over,
  });

  it("says plainly there is none for the date and offers the others by name, date and song count, never choosing one", () => {
    render(<Host initial={{ song_order: [] }} library={withLibrary(none())} />);
    const line = screen.getByTestId("playback-setlist");
    expect(line).toHaveAttribute("data-state", "none");
    expect(line).toHaveTextContent("Playback has no setlist for 2026-11-29; its newest is Saturday Night from 2026-11-21.");
    expect(line).toHaveTextContent("Nothing is lined up until you choose one.");
    const choices = within(screen.getByRole("list", { name: "Other Playback setlists" })).getAllByRole("button");
    expect(choices.map((b) => b.getAttribute("aria-label"))).toEqual(["Use Saturday Night, 2026-11-21, 5 songs", "Use Old Set, 2026-11-08, 1 song"]);
    expect(screen.queryByRole("list", { name: "Song pairs" })).not.toBeInTheDocument();
  });

  it("choosing one asks the controller for that setlist", async () => {
    const user = userEvent.setup();
    const chooseSetlist = vi.fn(async () => true);
    render(<Host initial={{ song_order: [] }} library={withLibrary(none(), { chooseSetlist })} />);
    await user.click(screen.getByRole("button", { name: "Use Old Set, 2026-11-08, 1 song" }));
    expect(chooseSetlist).toHaveBeenCalledWith(6);
  });

  it("keeps the plain scan view under the line when there is a scan", () => {
    render(<Host initial={{ song_order: [900, 17, 42, 55] }} library={withLibrary(none())} />);
    expect(screen.getByTestId("playback-setlist")).toBeInTheDocument();
    expect(rows()[0]).toHaveTextContent("Playback song 1");
  });

  it("says a chosen setlist is the user's choice and offers the way back to the date's own", async () => {
    const user = userEvent.setup();
    const chooseSetlist = vi.fn(async () => true);
    const chosen = sampleLibrary({ chosen: true, setlist: { id: 7, title: "Saturday Night", date: "2026-11-21", songs: 4 }, plan_date: "2026-11-29", setlists: [{ id: 7, title: "Saturday Night", date: "2026-11-21", songs: 4 }, { id: 6, title: "Old Set", date: "2026-11-08", songs: 1 }] });
    render(<Host library={withLibrary(chosen, { chooseSetlist })} />);
    const line = screen.getByTestId("playback-setlist");
    expect(line).toHaveAttribute("data-state", "chosen");
    expect(line).toHaveTextContent("Chosen by you for this plan (the plan's date is 2026-11-29).");
    await user.click(screen.getByRole("button", { name: "Choose another setlist" }));
    expect(screen.getByRole("list", { name: "Other Playback setlists" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Use Saturday Night/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Use the setlist for 2026-11-29" }));
    expect(chooseSetlist).toHaveBeenCalledWith(null);
  });

  it("says when the switch did not work, and keeps the choices", async () => {
    const user = userEvent.setup();
    render(<Host initial={{ song_order: [] }} library={withLibrary(none(), { chooseSetlist: vi.fn(async () => false) })} />);
    await user.click(screen.getByRole("button", { name: "Use Old Set, 2026-11-08, 1 song" }));
    expect(screen.getByRole("alert")).toHaveTextContent("could not switch the Playback setlist");
    expect(screen.getByRole("list", { name: "Other Playback setlists" })).toBeInTheDocument();
  });

  it("follows the loaded plan's date, never today's", () => {
    render(<Host initial={{ song_order: [] }} library={withLibrary(none({ plan_date: "2031-05-04", message: "Playback has no setlist for 2031-05-04." }))} />);
    const line = screen.getByTestId("playback-setlist");
    expect(line).toHaveTextContent("Playback has no setlist for 2031-05-04.");
    expect(screen.getByRole("button", { name: /Use Saturday Night/ })).toBeVisible();
    expect(line).not.toHaveTextContent(new Date().toISOString().slice(0, 10));
  });
});
