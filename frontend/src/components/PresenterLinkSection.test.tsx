import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { ADD_SONGS_CHOICE_KEY, JOB_POLL_MS } from "../hooks/useAddSongs";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { AddSongsPlan } from "../propresenterLink";
import { libraryController, libraryPlan, librarySong, librarySongOf, libraryStatus } from "../test/libraryFixtures";
import { addPlan, addSong, guess, helperStatus, jobOf, linkController, localStatus, noComputerStatus, playlists, report, result, step } from "../test/propresenterLinkFixtures";
import type { Song } from "../types";
import { PresenterLinkSection } from "./PresenterLinkSection";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getProPresenterPlaylists: vi.fn(),
  lookupAddSongsTitles: vi.fn(),
  previewAddSongs: vi.fn(),
  applyAddSongs: vi.fn(),
  getAddSongsJob: vi.fn(),
}));

const SONGS = [
  { title: "Holy Forever", key: "G", arrangement: "Default" },
  { title: "Battle Belongs", key: "Ab", arrangement: "Long ending" },
  { title: "Gratitude", key: "A", arrangement: "Default" },
];
const plan = () => libraryPlan(SONGS.map((s, i) => librarySongOf(s.title, i + 1, { library: librarySong({ title: s.title, position: i + 1, setlist_song_id: 2000001 + i, content_id: [23126, 31877, 8412][i]!, key: s.key, arrangement: s.arrangement, sections: [{ name: "Verse 1", start: 0, end: 1 }, { name: "Chorus", start: 1, end: 2 }] }) })));
const library = (over: Partial<PlaybackLibraryController> = {}) => libraryController("ready", { plan: plan(), ...over });
const REF = ["2000001", "2000002", "2000003"];
/** The loaded Planning Center plan: the names the list shows until Playback has given the songs. */
const pcSong = (title: string, order: number): Song => ({ id: `s${order}`, title, duration_seconds: 300, order, is_generic: false, source_song_id: null });
const pcPlan = (...titles: string[]) => ({ songs: titles.map((title, index) => pcSong(title, index)) });
const PC = pcPlan("Holy Forever", "Battle Belongs", "Gratitude");

const copyPlan = (over: Partial<AddSongsPlan> = {}) => addPlan([
  addSong({ ref_id: REF[0]!, title: "Holy Forever", file: "Holy Forever (1)", mapping: [{ section: "Verse 1", group: "Verse 1", how: "full", line: null }, { section: "Chorus", group: "Chorus", how: "full", line: null }] }),
  addSong({ ref_id: REF[1]!, title: "Battle Belongs", file: "Battle Belongs (2)" }),
  addSong({ ref_id: REF[2]!, title: "Gratitude", file: "Gratitude (1)" }),
], over);
const stuckPlan = () => addPlan([
  addSong({ ref_id: REF[0]!, title: "Holy Forever" }),
  addSong({ ref_id: REF[1]!, title: "Battle Belongs", action: "blocked", reason: "Not in ProPresenter." }),
  addSong({ ref_id: REF[2]!, title: "Gratitude", action: "needs_choice", reason: "Two files." }),
]);

type Setup = { link?: ProPresenterLinkController; library?: PlaybackLibraryController; plan?: { songs: Song[] } | null; playing?: boolean };
const view = (setup: Setup = {}) => {
  const link = setup.link ?? linkController(helperStatus("ready"));
  const lib = "library" in setup ? setup.library : library();
  const pc = "plan" in setup ? setup.plan : PC;
  const utils = render(<PresenterLinkSection library={lib} link={link} plan={pc} playing={setup.playing ?? false} />);
  const again = (next: Setup) => utils.rerender(<PresenterLinkSection library={"library" in next ? next.library : lib} link={next.link ?? link} plan={"plan" in next ? next.plan : pc} playing={next.playing ?? setup.playing ?? false} />);
  return { ...utils, again };
};
const ADD = { name: "Import songs to ProPresenter" };
const addButton = () => screen.getByRole("button", ADD);
const section = () => screen.getByTestId("presenter-link-section");

/** The section, with its playlists read. */
async function ready(user = userEvent.setup()) {
  await screen.findByRole("option", { name: "Sunday Morning Service" });
  return user;
}
async function choose(user: ReturnType<typeof userEvent.setup>, playlist = "PL-SUNDAY", where?: string) {
  await user.selectOptions(screen.getByLabelText("Playlist"), playlist);
  if (where !== undefined) await user.selectOptions(screen.getByLabelText("Section"), where);
}
/** Presses the one button, with the preview the route would answer. */
async function press(user: ReturnType<typeof userEvent.setup>, made: AddSongsPlan = copyPlan()) {
  vi.mocked(api.previewAddSongs).mockResolvedValue(made);
  await user.click(addButton());
}
/** The sentences the section no longer says (docs/propresenter.md explains instead). */
const REMOVED = /This imports songs|This adds the songs as they are|contents are replaced|Nothing in ProPresenter changes until|Playback has not given the songs yet|These songs come from Playback|ProPresenter does not show the key/;
/** What a person sees: the element's text without the parts only a screen reader gets. */
const visibleText = (element: Element) => {
  const copy = element.cloneNode(true) as Element;
  copy.querySelectorAll(".sr-only").forEach((node) => node.remove());
  return copy.textContent ?? "";
};
const checked = (box: HTMLElement) => box.getAttribute("aria-checked") === "true";
const nameOf = (row: HTMLElement) => within(row).getByRole("checkbox").getAttribute("aria-label");

beforeEach(() => {
  window.localStorage.clear();
  vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
  vi.mocked(api.lookupAddSongsTitles).mockImplementation(async (titles) => ({ titles: titles.map((title) => ({ title, state: "found" as const })) }));
  vi.mocked(api.previewAddSongs).mockResolvedValue(copyPlan());
  vi.mocked(api.applyAddSongs).mockResolvedValue({ job_id: "fedcba9876543210fedcba9876543210", plan_id: "p", state: "running" });
  vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf());
});
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); window.localStorage.clear(); });

describe("when the section is drawn: only while the Presenter Link is ready", () => {
  it("is there for another computer and for this one, with or without the Playback library", async () => {
    const { unmount } = view();
    expect(screen.getByRole("group", { name: "Presenter Link" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Presenter Link" })).toBeInTheDocument();
    expect(addButton()).toBeInTheDocument();
    await ready();
    unmount();
    view({ link: linkController(localStatus()), library: undefined });
    expect(addButton()).toBeInTheDocument();
    await ready();
  });

  it.each([
    ["no status read yet", { link: linkController(null) }],
    ["no ProPresenter computer saved", { link: linkController(noComputerStatus()) }],
    ["Link not installed", { link: linkController(helperStatus("not_installed")) }],
    ["Link needs an update (the backend wants it ready)", { link: linkController(helperStatus("outdated")) }],
    ["Link needs the command again", { link: linkController(helperStatus("needs_command")) }],
    ["Link not answering", { link: linkController(helperStatus("not_answering")) }],
    ["Link being checked", { link: linkController(helperStatus("checking")) }],
  ] as [string, Setup][])("draws nothing, asks nothing and warns about nothing when: %s", (_name, setup) => {
    const { container } = view(setup);
    expect(container).toBeEmptyDOMElement();
    expect(api.getProPresenterPlaylists).not.toHaveBeenCalled();
    expect(api.lookupAddSongsTitles).not.toHaveBeenCalled();
  });

  it("draws nothing when no controller is given (a panel that does not know the Link)", () => {
    const { container } = render(<PresenterLinkSection library={library()} plan={PC} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("appears and goes with the Link, with no warning when it goes", async () => {
    const { again, container } = view({ link: linkController(helperStatus("not_installed")) });
    expect(container).toBeEmptyDOMElement();
    again({ link: linkController(helperStatus("ready")) });
    await ready();
    expect(screen.getByRole("button", ADD)).toBeInTheDocument();
    again({ link: linkController(helperStatus("not_answering")) });
    expect(container).toBeEmptyDOMElement();
  });
});

describe("the list: Planning Center names until Playback has given the songs", () => {
  it("lists the plan's names as cards, selected, each with just the title and a Planning Center chip: no sentence about where they come from", async () => {
    view({ library: undefined });
    await ready();
    expect(section().textContent).not.toMatch(REMOVED);
    const list = screen.getByRole("list", { name: "Songs to import" });
    const rows = within(list).getAllByTestId("add-songs-row");
    expect(rows.map(nameOf)).toEqual(["Holy Forever", "Battle Belongs", "Gratitude"]);
    for (const row of rows) {
      expect(row).toHaveAttribute("data-source", "planning-center");
      expect(within(row).getByRole("checkbox")).toBeChecked();
      expect(within(row).getByTestId("add-songs-source-chip")).toHaveTextContent("Planning Center");
      expect(within(row).queryByTestId("add-songs-key")).toBeNull();
      expect(within(row).queryByTestId("add-songs-multitracks")).toBeNull();
    }
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("3 of 3 selected");
  });

  it("uses the names when the Library Link has no data: not installed, not answering, an old copy, no setlist, no paired song", () => {
    for (const lib of [
      libraryController("not_installed"), libraryController("unreachable"), libraryController("needs_key"),
      libraryController("ready", { status: libraryStatus("ready", { stale: true }), plan: plan() }),
      libraryController("ready", { plan: null }),
      libraryController("ready", { plan: libraryPlan([librarySongOf("A", 1)], { setlist: null }) }),
      libraryController("ready", { plan: libraryPlan([], { state: "no_setlist" }) }),
      libraryController("ready", { plan: libraryPlan([librarySongOf("A", 1, { status: "missing", confidence: null, library: null })]) }),
    ]) {
      const { unmount } = view({ library: lib });
      expect(screen.getByTestId("add-songs-songs")).toHaveAttribute("data-source", "planning-center");
      unmount();
    }
  });

  it("says so, and cannot add, when the plan has no songs", async () => {
    view({ library: undefined, plan: pcPlan() });
    await ready();
    expect(screen.getByTestId("add-songs-empty")).toHaveTextContent("The loaded plan has no songs yet.");
    expect(addButton()).toBeDisabled();
  });

  it("marks a name ProPresenter does not have, says why, and it cannot be ticked", async () => {
    vi.mocked(api.lookupAddSongsTitles).mockResolvedValue({ titles: [{ title: "Holy Forever", state: "found" }, { title: "Battle Belongs", state: "missing" }, { title: "Gratitude", state: "several" }] });
    view({ library: undefined });
    await ready();
    const missing = await screen.findByRole("checkbox", { name: "Battle Belongs" });
    await waitFor(() => expect(missing).toBeDisabled());
    expect(missing).not.toBeChecked();
    const row = missing.closest("li")!;
    expect(row).toHaveAttribute("data-blocked", "missing");
    // A chip and one short phrase; the full sentence is the hover text.
    expect(row).toHaveTextContent("Not in ProPresenter");
    expect(within(row).getByTestId("add-songs-why")).toHaveTextContent("Needs Playback data");
    expect(within(row).getByTestId("add-songs-why")).toHaveAttribute("title", "Playback data is needed to import it.");
    expect(row.textContent).not.toMatch(/\.$/);
    expect(missing).toHaveAccessibleDescription(/Not in ProPresenter/);
    expect(screen.getByRole("checkbox", { name: "Gratitude" })).toBeDisabled();
    const several = screen.getByRole("checkbox", { name: "Gratitude" }).closest("li")!;
    expect(several).toHaveTextContent("Pick one");
    expect(several).toHaveTextContent("Several files");
    expect(within(several).getByTestId("add-songs-blocked")).toHaveAttribute("title", "More than one song in ProPresenter has this title.");
    // The disabled look: grey and dashed, like a Lighting card with nothing saved.
    expect(row.className).toMatch(/border-dashed/);
    expect(screen.getByRole("checkbox", { name: "Holy Forever" })).toBeEnabled();
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("1 of 1 selected");
    expect(api.lookupAddSongsTitles).toHaveBeenCalledWith(["Holy Forever", "Battle Belongs", "Gratitude"]);
  });

  it("asks ProPresenter once for the same names, and again only when the names change", async () => {
    const { again } = view({ library: undefined });
    await ready();
    await waitFor(() => expect(api.lookupAddSongsTitles).toHaveBeenCalledTimes(1));
    again({ library: undefined, plan: pcPlan("Holy Forever", "Battle Belongs", "Gratitude") });
    again({ library: undefined, plan: pcPlan("Holy Forever", "New Song") });
    await waitFor(() => expect(api.lookupAddSongsTitles).toHaveBeenCalledTimes(2));
    expect(api.lookupAddSongsTitles).toHaveBeenLastCalledWith(["Holy Forever", "New Song"]);
  });

  it("when ProPresenter cannot be asked, the names can still be ticked and the preview is the judge", async () => {
    vi.mocked(api.lookupAddSongsTitles).mockRejectedValue(new api.ApiError("ProPresenter did not answer.", 502, "propresenter_unreachable"));
    view({ library: undefined });
    await ready();
    await waitFor(() => expect(api.lookupAddSongsTitles).toHaveBeenCalled());
    for (const box of screen.getAllByRole("checkbox")) expect(box).toBeEnabled();
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("3 of 3 selected");
  });

  it("asks for the ticked names by title, in plan order, after the chosen header", async () => {
    view({ library: undefined });
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    await user.click(screen.getByRole("checkbox", { name: "Battle Belongs" }));
    await press(user);
    expect(api.previewAddSongs).toHaveBeenCalledWith({ titles: ["Holy Forever", "Gratitude"], playlist_id: "PL-SUNDAY", after_header: "Worship" });
  });

});

describe("the list: Playback's songs once it has given them", () => {
  it("shows title, key, mtID and the order on each card, in Playback's order, selected", async () => {
    view();
    await ready();
    expect(section().textContent).not.toMatch(REMOVED);
    expect(screen.getByTestId("add-songs-songs")).toHaveAttribute("data-source", "playback");
    const list = screen.getByRole("list", { name: "Songs to import" });
    const rows = within(list).getAllByTestId("add-songs-row");
    expect(rows.map((row) => [
      nameOf(row),
      within(row).getByTestId("add-songs-key").textContent,
      within(row).getByTestId("add-songs-mtid").textContent,
      within(row).getByTestId("add-songs-order").textContent,
    ])).toEqual([["Holy Forever", "G", "23126", "Verse 1 \u00B7 Chorus"], ["Battle Belongs", "Ab", "31877", "Verse 1 \u00B7 Chorus"], ["Gratitude", "A", "8412", "Verse 1 \u00B7 Chorus"]]);
    // Plain text: no "Key" word, no MultiTracks, no arrangement name.
    expect(list.textContent).not.toMatch(/\bKey\b|MultiTracks|Default|Long ending|Sections/);
    for (const box of within(list).getAllByRole("checkbox")) expect(box).toBeChecked();
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("3 of 3 selected");
    expect(screen.queryByTestId("add-songs-source-chip")).toBeNull();
    // Playback has the songs: nothing is asked of ProPresenter by title.
    expect(api.lookupAddSongsTitles).not.toHaveBeenCalled();
  });

  it("puts the songs in the setlist's order, not the plan's", async () => {
    const songs = [librarySongOf("Second", 1, { library: librarySong({ title: "Second", position: 2, setlist_song_id: 2000002 }) }), librarySongOf("First", 2, { library: librarySong({ title: "First", position: 1, setlist_song_id: 2000001 }) })];
    view({ library: library({ plan: libraryPlan(songs) }) });
    await ready();
    expect(screen.getAllByTestId("add-songs-row").map(nameOf)).toEqual(["First", "Second"]);
  });

  it("shows the order of the sections on the card with nothing to press, and the full names on hover", async () => {
    view();
    await ready();
    const row = screen.getAllByTestId("add-songs-row")[0]!;
    expect(within(row).getByTestId("add-songs-order")).toHaveTextContent("Verse 1 \u00B7 Chorus");
    expect(row).toHaveAttribute("title", "Holy Forever: Verse 1, Chorus");
    expect(within(row).queryByRole("button", { name: /Sections/ })).toBeNull();
    expect(row.querySelector("[aria-expanded]")).toBeNull();
  });

  it("a song can be ticked off by its name, and all of them at once", async () => {
    view();
    const user = await ready();
    await user.click(screen.getByRole("checkbox", { name: "Battle Belongs" }));
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("2 of 3 selected");
    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("3 of 3 selected");
    await user.click(screen.getByRole("button", { name: "Select none" }));
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("0 of 3 selected");
  });

  it("a song Playback gave no number cannot be added, and says so", async () => {
    const lib = library({ plan: libraryPlan([librarySongOf("Holy Forever", 1), librarySongOf("Odd One", 2, { library: librarySong({ title: "Odd One", position: 2, setlist_song_id: null }) })]) });
    view({ library: lib });
    await ready();
    expect(screen.getByTestId("add-songs-unnamed")).toHaveTextContent("Odd One");
    expect(screen.getByTestId("add-songs-unnamed")).toHaveTextContent("Can't import");
    expect(screen.getByTestId("add-songs-unnamed")).toHaveTextContent("No Playback number");
    expect(within(screen.getByTestId("add-songs-unnamed")).getByTestId("add-songs-blocked")).toHaveAttribute("title", "Playback gave this song no number, so it cannot be imported.");
    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
  });

  it("asks for the ticked songs by Playback's numbers, in setlist order, after the chosen header", async () => {
    view();
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    await user.click(screen.getByRole("checkbox", { name: "Battle Belongs" }));
    await press(user);
    expect(api.previewAddSongs).toHaveBeenCalledWith({ setlist_song_ids: [2000001, 2000003], playlist_id: "PL-SUNDAY", after_header: "Worship" });
  });

  it("asks for the end of the playlist with place: end", async () => {
    view();
    const user = await ready();
    await choose(user, "PL-WED");
    await press(user);
    expect(api.previewAddSongs).toHaveBeenCalledWith({ setlist_song_ids: [2000001, 2000002, 2000003], playlist_id: "PL-WED", place: "end" });
  });
});

describe("the swap between the two sources", () => {
  it("keeps the person's ticks for songs in both lists, ticks a new song, and does not move a row", async () => {
    const { again } = view({ library: undefined });
    const user = await ready();
    await user.click(screen.getByRole("checkbox", { name: "Battle Belongs" }));
    const before = screen.getAllByTestId("add-songs-row");
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("2 of 3 selected");
    // Playback gives the songs (one more than the plan has names for).
    const lib = library({ plan: libraryPlan([...plan().songs, librarySongOf("Brand New", 4, { library: librarySong({ title: "Brand New", position: 4, setlist_song_id: 2000004, content_id: 16500 }) })]) });
    again({ library: lib });
    const after = screen.getAllByTestId("add-songs-row");
    expect(screen.getByTestId("add-songs-songs")).toHaveAttribute("data-source", "playback");
    expect(after.slice(0, 3)).toEqual(before);
    expect(after.map((row) => checked(within(row).getByRole("checkbox")))).toEqual([true, false, true, true]);
    expect(within(after[1]!).getByTestId("add-songs-key")).toHaveTextContent(/^Ab$/);
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("3 of 4 selected");
    // And back when the data is lost: the ticks stay.
    again({ library: undefined });
    expect(screen.getByTestId("add-songs-songs")).toHaveAttribute("data-source", "planning-center");
    expect(screen.getByRole("checkbox", { name: "Battle Belongs" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Holy Forever" })).toBeChecked();
  });

  it("does not move the playlist or the section, and says the swap politely", async () => {
    const { again } = view({ library: undefined });
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    again({ library: library() });
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    expect(screen.getByLabelText("Section")).toHaveValue("Worship");
    // The count is the polite line a screen reader hears; the chips say where the songs come from.
    expect(screen.getByTestId("add-songs-count")).toHaveAttribute("role", "status");
    expect(screen.getByTestId("add-songs-count")).toHaveAttribute("aria-live", "polite");
    expect(screen.queryByTestId("add-songs-source-chip")).toBeNull();
    expect(api.getProPresenterPlaylists).toHaveBeenCalledTimes(1);
  });

  it("a song ticked off stays off in a title that Playback spells differently", async () => {
    const { again } = view({ library: undefined, plan: pcPlan("Holy Forever (Live)", "Gratitude") });
    const user = await ready();
    await user.click(screen.getByRole("checkbox", { name: "Holy Forever (Live)" }));
    again({ library: library({ plan: libraryPlan([librarySongOf("Holy Forever  LIVE", 1, { library: librarySong({ title: "Holy Forever  LIVE", position: 1, setlist_song_id: 2000001 }) })]) }) });
    expect(screen.getByRole("checkbox", { name: "Holy Forever LIVE" })).not.toBeChecked();
  });
});

describe("the options: playlist and section", () => {
  it("lists the playlists (a folder is named) and, once one is chosen, the section names in the playlist's order, with End of Playlist only when there is no section", async () => {
    view();
    const user = await ready();
    const playlist = screen.getByLabelText("Playlist");
    expect(within(playlist).getAllByRole("option").map((o) => o.textContent)).toEqual(["Sunday Morning Service", "Wednesday Night", "Special services / Youth Night"]);
    expect(playlist).toHaveValue("");
    expect(screen.getByLabelText("Section")).toBeDisabled();
    await choose(user);
    // No End of Playlist next to real sections: the last section is the same place.
    expect(within(screen.getByLabelText("Section")).getAllByRole("option").map((o) => o.textContent)).toEqual(["Welcome", "Worship", "Message"]);
    expect(screen.getByLabelText("Section")).toHaveValue("Message");
    expect(screen.queryByRole("option", { name: "End of Playlist" })).toBeNull();
    expect(section().textContent).not.toMatch(/After \u201C/);
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-WED");
    expect(within(screen.getByLabelText("Section")).getAllByRole("option").map((o) => o.textContent)).toEqual(["End of Playlist"]);
    expect(screen.getByLabelText("Section")).toHaveValue("");
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-YOUTH");
    expect(within(screen.getByLabelText("Section")).getAllByRole("option").map((o) => o.textContent)).toEqual(["Games"]);
  });

  it("offers no option at all (a dash) until a playlist is chosen", async () => {
    view();
    await ready();
    const box = screen.getByLabelText("Section");
    expect(box).toBeDisabled();
    expect(within(box).queryByRole("option", { name: "End of Playlist" })).toBeNull();
  });

  it("is built from the settings template: a group named Presenter Link, two fields side by side", async () => {
    view();
    await ready();
    const group = screen.getByRole("group", { name: "Presenter Link" });
    expect(group).toHaveAttribute("data-section", "settings");
    expect(group.querySelector("[data-columns='pair']")).not.toBeNull();
    expect(within(group).getByLabelText("Playlist")).toBeInTheDocument();
    expect(within(group).getByLabelText("Section")).toBeInTheDocument();
  });

  it("says when the playlists cannot be read, and Look again reads them", async () => {
    vi.mocked(api.getProPresenterPlaylists).mockRejectedValueOnce(new api.ApiError("ProPresenter did not answer on 192.168.1.60.", 502, "propresenter_unreachable"));
    view();
    const user = userEvent.setup();
    expect(await screen.findByText("ProPresenter did not answer on 192.168.1.60.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Look again" }));
    expect(await screen.findByRole("option", { name: "Sunday Morning Service" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Look again" })).toBeNull();
  });

  it("is disabled while the preview or the job runs", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    view();
    await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    await press(user);
    await act(async () => { await vi.advanceTimersByTimeAsync(5); });
    expect(screen.getByLabelText("Playlist")).toBeDisabled();
    expect(screen.getByLabelText("Section")).toBeDisabled();
    for (const box of screen.getAllByRole("checkbox")) expect(box).toBeDisabled();
  });
});

describe("the last playlist, and per playlist the section last USED FOR AN IMPORT, are remembered in this browser, and nowhere else", () => {
  const stored = () => JSON.parse(window.localStorage.getItem(ADD_SONGS_CHOICE_KEY) ?? "null");
  const importInto = async (user: ReturnType<typeof userEvent.setup>, playlist: string, where: string) => {
    await choose(user, playlist, where);
    await press(user);
    await screen.findByTestId("add-songs-progress");
  };

  it("the very first default is the LAST section (the same place as the end); a playlist with no section starts at its one option, End of Playlist", async () => {
    view();
    const user = await ready();
    await choose(user);
    expect(screen.getByLabelText("Section")).toHaveValue("Message");
    expect(within(screen.getByLabelText("Section")).getByRole("option", { selected: true })).toHaveTextContent("Message");
    await choose(user, "PL-WED");
    expect(within(screen.getByLabelText("Section")).getByRole("option", { selected: true })).toHaveTextContent("End of Playlist");
    // The last section is what is asked of the engine: after its header, which is the end of that playlist.
    await choose(user, "PL-SUNDAY");
    await press(user);
    await screen.findByTestId("add-songs-progress");
    expect(vi.mocked(api.previewAddSongs).mock.calls[0]![0]).toMatchObject({ after_header: "Message" });
  });

  it("choosing a section is not remembered; an import is, for that playlist, and is the default the next time", async () => {
    const first = view();
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    expect(stored()).toEqual({ playlist_id: "PL-SUNDAY", places: {} });
    first.unmount();
    view();
    await ready();
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    expect(screen.getByLabelText("Section")).toHaveValue("Message");
    await importInto(user, "PL-SUNDAY", "Worship");
    expect(stored()).toEqual({ playlist_id: "PL-SUNDAY", places: { "PL-SUNDAY": "Worship" } });
    cleanup();
    view();
    await ready();
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    expect(screen.getByLabelText("Section")).toHaveValue("Worship");
  });

  it("is per playlist: another playlist has its own default, and coming back brings the remembered section back", async () => {
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify({ playlist_id: "PL-SUNDAY", places: { "PL-SUNDAY": "Message", "PL-YOUTH": "Games" } }));
    view();
    const user = await ready();
    expect(screen.getByLabelText("Section")).toHaveValue("Message");
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-YOUTH");
    expect(screen.getByLabelText("Section")).toHaveValue("Games");
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-WED");
    expect(screen.getByLabelText("Section")).toHaveValue("");
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-SUNDAY");
    expect(screen.getByLabelText("Section")).toHaveValue("Message");
  });

  it("an import into a playlist with no section is remembered as End of Playlist", async () => {
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify({ playlist_id: "PL-SUNDAY", places: { "PL-SUNDAY": "Worship" } }));
    view();
    const user = await ready();
    await importInto(user, "PL-WED", "");
    expect(stored().places).toEqual({ "PL-SUNDAY": "Worship", "PL-WED": "" });
    expect(vi.mocked(api.previewAddSongs).mock.calls[0]![0]).toMatchObject({ place: "end" });
  });

  it("a remembered End of Playlist in a playlist that now has sections falls back to the last section; a remembered section in one that has none, to End of Playlist", async () => {
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify({ playlist_id: "PL-SUNDAY", places: { "PL-SUNDAY": "", "PL-WED": "Worship" } }));
    view();
    const user = await ready();
    expect(screen.getByLabelText("Section")).toHaveValue("Message");
    expect(within(screen.getByLabelText("Section")).queryByRole("option", { name: "End of Playlist" })).toBeNull();
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-WED");
    expect(screen.getByLabelText("Section")).toHaveValue("");
    expect(addButton()).toBeEnabled();
  });

  it("a preview that stopped (nothing imported) remembers nothing", async () => {
    view();
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    await press(user, stuckPlan());
    await screen.findByTestId("add-songs-preview");
    expect(stored().places).toEqual({});
  });

  it("falls back to the last section when the remembered section is not in the playlist any more, and to nothing when the playlist is gone", async () => {
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify({ playlist_id: "PL-SUNDAY", places: { "PL-SUNDAY": "Old header" } }));
    const { unmount } = view();
    await ready();
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    expect(screen.getByLabelText("Section")).toHaveValue("Message");
    unmount();
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify({ playlist_id: "GONE", places: {} }));
    view();
    await ready();
    expect(screen.getByLabelText("Playlist")).toHaveValue("");
  });

  it("ignores a damaged or older value, and works when storage refuses to write", async () => {
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify({ playlist_id: "PL-SUNDAY", place: "Worship" }));
    const older = view();
    await ready();
    // The older shape remembered a choice, not an import: the section is not taken from it (the last section shows).
    expect(screen.getByLabelText("Section")).toHaveValue("Message");
    older.unmount();
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, "{not json");
    const spy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("quota"); });
    view();
    const user = await ready();
    expect(screen.getByLabelText("Playlist")).toHaveValue("");
    await choose(user);
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    await press(user);
    await screen.findByTestId("add-songs-progress");
    spy.mockRestore();
  });

  it("sends nothing about the choice to StagePilot", async () => {
    view();
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    expect(api.previewAddSongs).not.toHaveBeenCalled();
    expect(api.applyAddSongs).not.toHaveBeenCalled();
  });
});

describe("two headers with the same name", () => {
  beforeEach(() => {
    vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: [{ id: "PL-TWICE", name: "Twice", folder: null, headers: ["Worship", "Message", "Worship"] }] });
  });

  it("lists both, and choosing that name shows the engine's line in place and does not guess", async () => {
    view();
    const user = userEvent.setup();
    await screen.findByRole("option", { name: "Twice" });
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-TWICE");
    expect(within(screen.getByLabelText("Section")).getAllByRole("option").map((o) => o.textContent)).toEqual(["Worship", "Message", "Worship"]);
    // The last section is the default, and its name is there twice: the line shows at once and the button waits.
    await user.selectOptions(screen.getByLabelText("Section"), "Message");
    expect(screen.queryByTestId("add-songs-two-headers")).toBeNull();
    await user.selectOptions(screen.getByLabelText("Section"), "Worship");
    const line = screen.getByTestId("add-songs-two-headers");
    expect(line).toHaveTextContent("More than one header in this playlist is named \u201CWorship\u201D. Pick another place.");
    expect(line.compareDocumentPosition(addButton()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(addButton()).toBeDisabled();
    await user.click(addButton());
    expect(api.previewAddSongs).not.toHaveBeenCalled();
    await user.selectOptions(screen.getByLabelText("Section"), "Message");
    expect(screen.queryByTestId("add-songs-two-headers")).toBeNull();
    expect(addButton()).toBeEnabled();
  });
});

describe("the Section default when the last section's name is there twice", () => {
  it("shows the engine's line at once and keeps the button off until another section is chosen", async () => {
    vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: [{ id: "PL-TWICE", name: "Twice", folder: null, headers: ["Worship", "Message", "Worship"] }] });
    view();
    const user = userEvent.setup();
    await screen.findByRole("option", { name: "Twice" });
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-TWICE");
    expect(screen.getByLabelText("Section")).toHaveValue("Worship");
    expect(screen.getByTestId("add-songs-two-headers")).toBeInTheDocument();
    expect(addButton()).toBeDisabled();
    await user.selectOptions(screen.getByLabelText("Section"), "Message");
    expect(addButton()).toBeEnabled();
  });
});

describe("the one button: when it is off, and why", () => {
  const reason = () => screen.getByTestId("add-songs-reason").textContent;

  it("is off with its reason until a playlist is chosen, and while no song is ticked", async () => {
    view();
    const user = await ready();
    expect(addButton()).toBeDisabled();
    expect(reason()).toBe("Choose a playlist");
    await choose(user);
    expect(addButton()).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Select none" }));
    expect(addButton()).toBeDisabled();
    expect(reason()).toBe("Select a song");
    expect(addButton()).toHaveAccessibleDescription("Select a song");
  });

  it("is off until the section is a header the playlist has", async () => {
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify({ playlist_id: "PL-SUNDAY", places: { "PL-SUNDAY": "Worship" } }));
    view();
    await ready();
    expect(screen.getByLabelText("Section")).toHaveValue("Worship");
    expect(addButton()).toBeEnabled();
  });

  it("says nothing about what pressing it does (the docs explain it): no sentence above it when nothing stops it", async () => {
    view();
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    expect(screen.queryByTestId("add-songs-what-it-does")).toBeNull();
    expect(screen.queryByTestId("add-songs-reason")).toBeNull();
    expect(addButton()).toBeEnabled();
    expect(addButton()).not.toHaveAttribute("aria-describedby");
    expect(section().textContent).not.toMatch(REMOVED);
    expect(api.previewAddSongs).not.toHaveBeenCalled();
  });

  it("is off when the Presenter Link stops being ready: the section says nothing changed", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const { again } = view();
    await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf());
    await press(user);
    await act(async () => { await vi.advanceTimersByTimeAsync(5); });
    again({ link: linkController(helperStatus("not_answering")) });
    // The job is running: the section stays to say how it ends, and says the Link is gone.
    expect(screen.getByTestId("add-songs-link-gone")).toHaveTextContent("Presenter Link not ready: nothing changed");
  });
});

describe("the one button: preview, then add, in one press", () => {
  it("makes the preview and, when nothing is blocked, applies it at once (nothing skipped)", async () => {
    view();
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    await press(user);
    await screen.findByTestId("add-songs-progress");
    expect(api.previewAddSongs).toHaveBeenCalledTimes(1);
    expect(api.applyAddSongs).toHaveBeenCalledTimes(1);
    expect(api.applyAddSongs).toHaveBeenCalledWith("0123456789abcdef0123456789abcdef", []);
    // The order: the preview was asked for first.
    expect(vi.mocked(api.previewAddSongs).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(api.applyAddSongs).mock.invocationCallOrder[0]!);
  });

  it("shows 'Importing…' on the button while it asks, and cannot be pressed twice", async () => {
    let release: (value: AddSongsPlan) => void = () => undefined;
    vi.mocked(api.previewAddSongs).mockReturnValue(new Promise((resolve) => { release = resolve; }));
    view();
    const user = await ready();
    await choose(user);
    await user.click(addButton());
    const busy = await screen.findByRole("button", { name: "Importing\u2026" });
    expect(busy).toBeDisabled();
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent("Importing\u2026");
    release(copyPlan());
    await screen.findByTestId("add-songs-progress");
    expect(api.previewAddSongs).toHaveBeenCalledTimes(1);
  });

  it("lists the guesses and still adds the songs: a guess is no block", async () => {
    view();
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    const song = addSong({ ref_id: REF[0]!, title: "How Awesome", file: "How Awesome (1)", guesses: [guess()] });
    await press(user, addPlan([song]));
    await screen.findByTestId("add-songs-progress");
    expect(api.applyAddSongs).toHaveBeenCalled();
    expect(screen.queryByTestId("add-songs-skip")).toBeNull();
  });

  it("an error on the preview is a sentence, never a raw status, and the choice stays", async () => {
    view();
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    vi.mocked(api.previewAddSongs).mockRejectedValueOnce(new api.ApiError("That playlist is not in ProPresenter any more.", 404, "playlist_not_found"));
    await user.click(addButton());
    expect(await screen.findByTestId("add-songs-problem")).toHaveTextContent("That playlist is not in ProPresenter any more.");
    expect(screen.getByTestId("add-songs-problem")).toHaveAttribute("data-code", "playlist_not_found");
    expect(screen.getByLabelText("Section")).toHaveValue("Worship");
    expect(api.applyAddSongs).not.toHaveBeenCalled();
    vi.mocked(api.previewAddSongs).mockRejectedValueOnce(new api.ApiError("{\"x\":1}", 502));
    await user.click(addButton());
    expect(await screen.findByText("StagePilot hit an error. Try again.")).toBeInTheDocument();
    vi.mocked(api.previewAddSongs).mockRejectedValueOnce(new TypeError("Failed to fetch"));
    await user.click(addButton());
    expect(await screen.findByText(/Can't reach StagePilot/)).toBeInTheDocument();
  });

  describe("when the preview stops it", () => {
    async function stopped(made: AddSongsPlan = stuckPlan(), setup: Setup = {}) {
      const utils = view(setup);
      const user = await ready();
      await choose(user, "PL-SUNDAY", "Worship");
      await press(user, made);
      await screen.findByTestId("add-songs-preview");
      return { user, ...utils };
    }

    it("stops after the preview when a song is blocked or needs a choice: nothing is applied, the reason is in words, Skip is offered", async () => {
      await stopped();
      expect(api.applyAddSongs).not.toHaveBeenCalled();
      expect(screen.getByTestId("add-songs-reason")).toHaveTextContent("2 can't be imported yet: skip them, or fix them and try again");
      // One neutral line of counts, then a chip per song.
      expect(screen.getByTestId("add-songs-summary")).toHaveTextContent("1 to add \u00B7 1 can't import \u00B7 1 pick one");
      expect(screen.getByTestId("add-songs-summary").className).not.toMatch(/danger|warning/);
      expect(screen.getAllByTestId("add-songs-status").map((chip) => chip.textContent)).toEqual(["+New copy", "!Can't import", "?Pick one"]);
      expect(screen.getByRole("button", { name: "Skip those and import the rest" })).toBeEnabled();
      const lines = screen.getAllByTestId("add-songs-preview-song");
      expect(lines.map((line) => line.getAttribute("data-action"))).toEqual(["copy", "blocked", "needs_choice"]);
      expect(screen.getByRole("heading", { name: "Preview" })).toHaveFocus();
      expect(screen.getByTestId("add-songs-live")).toHaveTextContent(/^Preview ready\. 2 can't be imported yet/);
    });

    it("Skip those and import the rest applies the preview with the stuck songs named as skipped", async () => {
      const { user } = await stopped();
      await user.click(screen.getByRole("button", { name: "Skip those and import the rest" }));
      await screen.findByTestId("add-songs-progress");
      expect(api.previewAddSongs).toHaveBeenCalledTimes(1);
      expect(api.applyAddSongs).toHaveBeenCalledWith("0123456789abcdef0123456789abcdef", [REF[1], REF[2]]);
      expect(screen.queryByRole("button", { name: "Skip those and import the rest" })).toBeNull();
    });

    it("is never half-added silently: pressing the main button again makes a new preview and stops again", async () => {
      const { user } = await stopped();
      await user.click(addButton());
      await waitFor(() => expect(api.previewAddSongs).toHaveBeenCalledTimes(2));
      expect(api.applyAddSongs).not.toHaveBeenCalled();
    });

    it("offers no Skip when every song is stuck, and says to fix them", async () => {
      await stopped(addPlan([addSong({ ref_id: REF[0]!, title: "Holy Forever", action: "blocked", reason: "x" })]));
      expect(screen.queryByRole("button", { name: "Skip those and import the rest" })).toBeNull();
      expect(screen.getByTestId("add-songs-reason")).toHaveTextContent("1 can't be imported yet: fix them, or select other songs");
    });

    it("stops, in a few words, when the playlist side is blocked (the engine's sentence is for screen readers), and Skip would not help", async () => {
      const blocked = copyPlan();
      blocked.playlist.blocked = "That header is in the playlist twice.";
      await stopped(blocked);
      expect(screen.getByTestId("add-songs-reason")).toHaveTextContent("Playlist is blocked");
      // The one case where the counts line is red: something blocks the whole import.
      expect(screen.getByTestId("add-songs-summary")).toHaveTextContent("Playlist is blocked");
      expect(screen.getByTestId("add-songs-summary").className).toMatch(/danger/);
      expect(screen.getByTestId("add-songs-playlist-blocked")).toHaveTextContent("That header is in the playlist twice.");
      expect(screen.getByTestId("add-songs-playlist-blocked")).toHaveClass("sr-only");
      expect(screen.queryByRole("button", { name: "Skip those and import the rest" })).toBeNull();
      expect(api.applyAddSongs).not.toHaveBeenCalled();
    });

    it("stops, with the sentence, when there is nothing to add", async () => {
      await stopped(addPlan([addSong({ ref_id: REF[0]!, title: "Holy Forever", action: "reuse", already_in_playlist: true })], { can_apply: false, to_add: 0 }));
      expect(screen.getByTestId("add-songs-reason")).toHaveTextContent("Nothing to import: all in playlist");
      expect(screen.getByTestId("add-songs-summary")).toHaveTextContent("0 to add \u00B7 1 in playlist");
      expect(screen.getByTestId("add-songs-status")).toHaveTextContent("In playlist");
      expect(screen.getByTestId("add-songs-song-sentence")).toHaveTextContent("It is already in this playlist, so it is not added again.");
      expect(api.applyAddSongs).not.toHaveBeenCalled();
    });

    it("a song that is playing says so in one short line at the top, before and after the preview; pressing again when it has ended adds", async () => {
      const first = view({ playing: true });
      await ready();
      expect(screen.getByTestId("add-songs-playing")).toHaveTextContent(/^Song playing: wait$/);
      first.unmount();
      const { user, again } = await stopped(copyPlan(), { playing: true });
      expect(api.applyAddSongs).not.toHaveBeenCalled();
      // After the preview the same words are the reason above the button: one line, not two.
      expect(screen.getByTestId("add-songs-reason")).toHaveTextContent(/^Song playing: wait$/);
      expect(screen.queryByTestId("add-songs-playing")).toBeNull();
      expect(addButton()).toBeEnabled();
      expect(screen.queryByRole("button", { name: "Skip those and import the rest" })).toBeNull();
      again({ playing: false });
      expect(screen.queryByTestId("add-songs-playing")).toBeNull();
      await user.click(addButton());
      await screen.findByTestId("add-songs-progress");
      expect(api.applyAddSongs).toHaveBeenCalledTimes(1);
    });

    it("goes stale when a song, the playlist or the section changes: Skip goes, and the preview says to press again", async () => {
      const { user } = await stopped();
      await user.click(screen.getByRole("checkbox", { name: "Gratitude" }));
      expect(screen.getByTestId("add-songs-stale")).toHaveTextContent("Out of date: press Import again");
      expect(screen.queryByRole("button", { name: "Skip those and import the rest" })).toBeNull();
      await user.click(screen.getByRole("checkbox", { name: "Gratitude" }));
      expect(screen.queryByTestId("add-songs-stale")).toBeNull();
      expect(screen.getByRole("button", { name: "Skip those and import the rest" })).toBeEnabled();
      await user.selectOptions(screen.getByLabelText("Section"), "Welcome");
      expect(screen.getByTestId("add-songs-stale")).toBeInTheDocument();
      await user.selectOptions(screen.getByLabelText("Section"), "Worship");
      await user.selectOptions(screen.getByLabelText("Playlist"), "PL-WED");
      expect(screen.getByTestId("add-songs-stale")).toBeInTheDocument();
      expect(api.applyAddSongs).not.toHaveBeenCalled();
    });

    it("keeps how sections were matched, the notes and the warnings out of sight: only for a screen reader and the hover text; no key sentence, no version", async () => {
      const line = "\u201CCount Off\u201D: matched by name to the blank group, the only group with that name.";
      const made = addPlan([
        addSong({ ref_id: REF[0]!, title: "Holy Forever", notes: [line, "The key G is saved in the copy. ProPresenter does not show it for these songs; StagePilot does."], mapping: [{ section: "Count Off", group: null, how: "only", line }, { section: "Verse 1", group: "Verse 1", how: "full", line: null }] }),
        addSong({ ref_id: REF[1]!, title: "Battle Belongs", action: "blocked", reason: "x" }),
      ], { warnings: ["ProPresenter is older than the one StagePilot was tried with."] });
      await stopped(made);
      const first = screen.getAllByTestId("add-songs-preview-song")[0]!;
      const said = within(first).getByTestId("add-songs-song-sentence");
      expect(said).toHaveClass("sr-only");
      expect(said.textContent).toContain("All 2 sections matched exactly to the song's own groups.");
      expect(said.textContent).toContain(line);
      expect(screen.getAllByText(line, { exact: false })).toHaveLength(1);
      expect(within(first).getByTestId("add-songs-status")).toHaveAttribute("title", said.textContent!);
      expect(screen.getByTestId("add-songs-warnings")).toHaveClass("sr-only");
      expect(screen.getByTestId("add-songs-warnings")).toHaveTextContent("ProPresenter is older than the one StagePilot was tried with.");
      // The key is on the card; the sentence about it is gone, and the version is only in the result.
      expect(section().textContent).not.toMatch(/key G is saved|ProPresenter does not show/);
      expect(screen.queryByTestId("add-songs-version")).toBeNull();
      expect(screen.queryByRole("list", { name: "How Holy Forever was matched" })).toBeNull();
      expect(section().textContent).not.toMatch(REMOVED);
      expect(screen.queryByTestId("add-songs-nothing-changes")).toBeNull();
      expect(screen.queryByTestId("add-songs-imports")).toBeNull();
      expect(screen.getByTestId("add-songs-playlist-line")).toHaveTextContent("Sunday Morning Service \u203A Worship");
      expect(screen.getByTestId("add-songs-summary")).toHaveTextContent("1 to add \u00B7 1 can't import");
    });

    it("renders every outcome as a row with ONE chip (a mark and a word) and at most one short phrase", async () => {
      await stopped(addPlan([
        addSong({ ref_id: REF[0]!, title: "Holy Forever", action: "reuse", file: "Holy Forever (1)" }),
        addSong({ ref_id: REF[1]!, title: "Battle Belongs", action: "copy", file: "Battle Belongs (2)", sections: 2 }),
        addSong({ ref_id: REF[2]!, title: "Gratitude", action: "import_then_copy", file: "Gratitude (1)", import_as: "Gratitude", sections: 2 }),
        addSong({ ref_id: "4", title: "Odd Song", action: "needs_choice", reason: "Two files are this song.", choices: ["Songs / A.pro", "Songs / B.pro"] }),
        addSong({ ref_id: "5", title: "Broken Song", action: "blocked", reason: "The song has no group called Bridge." }),
        addSong({ ref_id: "6", title: "Plain Song", action: "as_is", file: "Plain Song", sections: 0, key: null }),
      ]));
      const lines = screen.getAllByTestId("add-songs-preview-song");
      expect(lines.map((line) => line.getAttribute("data-action"))).toEqual(["reuse", "copy", "import_then_copy", "needs_choice", "blocked", "as_is"]);
      const said = (i: number) => within(lines[i]!).getByTestId("add-songs-song-sentence").textContent;
      expect(said(0)).toBe("Already in ProPresenter as \u201CHoly Forever (1)\u201D.");
      expect(said(1)).toBe("New copy \u201CBattle Belongs (2)\u201D with Playback's order (2 sections).");
      expect(said(2)).toBe("Will be loaded from ProPresenter search first (as \u201CGratitude\u201D), then a new copy \u201CGratitude (1)\u201D with Playback's order (2 sections).");
      expect(said(3)).toBe("Needs a choice: Two files are this song. Files: \u201CSongs / A.pro\u201D, \u201CSongs / B.pro\u201D.");
      expect(said(4)).toBe("Blocked: The song has no group called Bridge.");
      expect(said(5)).toBe("Added as it is in ProPresenter as \u201CPlain Song\u201D: no copy, no new arrangement, no key change.");
      expect(lines.map((line) => line.querySelector("[data-tone]")?.textContent)).toEqual(["+Ready", "+New copy", "\u2193Will import", "?Pick one", "!Can't import", "+Ready"]);
      expect(lines.map((line) => line.querySelector("[data-testid=add-songs-phrase]")?.textContent ?? null)).toEqual(["Copy exists", "Playback order, \u201C(2)\u201D", null, null, null, "Used as is"]);
      expect(within(lines[3]!).getAllByRole("listitem").map((file) => file.textContent)).toEqual(["Songs / A.pro", "Songs / B.pro"]);
      for (const line of lines) expect(line.querySelector("[data-tone]")?.textContent?.replace(/[^A-Za-z ]/g, "")).toMatch(/[A-Za-z]{4}/);
      // The chip text is the status, not the colour: the visible text of a row has no sentence.
      for (const line of lines) expect(visibleText(line), visibleText(line)).not.toMatch(/[.!?]($|\s)/);
      expect(within(lines[0]!).getByTestId("add-songs-status")).toHaveAttribute("title", said(0)!);
    });
  });

  it("refuses at once, the sentence is shown and the preview stays; a preview that is no longer true is marked", async () => {
    for (const [code, status, text, gone] of [
      ["song_playing", 409, "A song is playing. Wait until it has ended, then try again.", false],
      ["plan_changed", 409, "ProPresenter or the playlist changed since the preview. Look at the preview again.", true],
      ["unknown_plan", 404, "That preview is too old or not known. Look at the preview again.", true],
      ["songs_blocked", 409, "Some songs cannot be added. Leave them out first.", true],
      ["busy", 409, "StagePilot is already adding songs. Wait until it has finished.", false],
      ["link_not_ready", 409, "The Presenter Link is not ready.", false],
    ] as const) {
      const { unmount } = view();
      const user = await ready();
      await choose(user, "PL-SUNDAY", "Worship");
      vi.mocked(api.applyAddSongs).mockRejectedValueOnce(new api.ApiError(text, status, code));
      await press(user);
      expect(await screen.findByTestId("add-songs-problem")).toHaveTextContent(text);
      expect(screen.getByTestId("add-songs-problem")).toHaveAttribute("data-code", code);
      expect(screen.queryByTestId("add-songs-progress")).toBeNull();
      expect(screen.getByTestId("add-songs-preview")).toBeInTheDocument();
      expect(api.getAddSongsJob).not.toHaveBeenCalled();
      expect(Boolean(screen.queryByTestId("add-songs-stale"))).toBe(gone);
      expect(addButton()).toBeEnabled();
      unmount();
      vi.clearAllMocks();
      vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
      vi.mocked(api.previewAddSongs).mockResolvedValue(copyPlan());
    }
  });

  it("a request that never reached StagePilot is the offline sentence", async () => {
    view();
    const user = await ready();
    await choose(user);
    vi.mocked(api.applyAddSongs).mockRejectedValueOnce(new TypeError("Failed to fetch"));
    await press(user);
    expect(await screen.findByTestId("add-songs-problem")).toHaveTextContent(/Can't reach StagePilot/);
  });
});

describe("progress, the result and Try again, in the section", () => {
  const start = async (setup: Setup = {}) => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    view(setup);
    await ready(user);
    await choose(user, "PL-SUNDAY", "Worship");
    return user;
  };
  const tick = (ms = JOB_POLL_MS) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

  it("shows progress per song and per step under the button, announces the newest thing politely, and cannot be pressed again while it runs", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValueOnce(jobOf({
      steps: [step("check", "done", "ProPresenter says: ProPresenter 19.0.1."), step("backup", "running", "Saving the playlist.")],
      songs: [
        { ref_id: REF[0]!, title: "Holy Forever", steps: [step("copy", "done", "Made \"Holy Forever (1)\".")], result: null },
        { ref_id: REF[2]!, title: "Gratitude", steps: [], result: null },
      ],
      events: [{ song: null, step: "backup", state: "running", text: "Saving the playlist." }],
    }));
    await press(user);
    await tick(5);
    const progress = screen.getByTestId("add-songs-progress");
    expect(within(progress).getByRole("heading", { name: "Importing" })).toHaveFocus();
    expect(screen.getByTestId("add-songs-now")).toHaveTextContent("Now: Saving the playlist.");
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent("Saving the playlist.");
    expect(within(screen.getByRole("list", { name: "Steps for the playlist" })).getAllByRole("listitem").map((li) => li.getAttribute("data-state"))).toEqual(["done", "running"]);
    expect(screen.getAllByTestId("add-songs-job-song").map((song) => song.querySelector("span")?.textContent)).toEqual(["Holy Forever", "Gratitude"]);
    // One short word per step, in a chip with a check, a spinner or a cross: no sentence on the page (what a step said is the hover and screen-reader text).
    expect(within(screen.getByRole("list", { name: "Steps for Holy Forever" })).getByText("Copy")).toBeInTheDocument();
    const steps = within(screen.getByRole("list", { name: "Steps for the playlist" })).getAllByRole("listitem");
    expect(steps.map((li) => visibleText(li))).toEqual(["\u2713ProPresenter", "Backup"]);
    expect(steps[1]!.querySelector("[data-part=spinner]")).not.toBeNull();
    expect(steps[0]!.querySelector("[data-part=spinner]")).toBeNull();
    expect(steps[0]).toHaveTextContent("Done");
    expect(steps[1]).toHaveTextContent("Working");
    expect(visibleText(progress)).not.toMatch(/Saving the playlist|ProPresenter says|Made \u201C|Now:/);
    // No way to press it again while it runs.
    expect(screen.queryByRole("button", ADD)).toBeNull();
    expect(screen.getByTestId("add-songs-reason")).toHaveTextContent("Importing: wait here");
    expect(screen.getByLabelText("Playlist")).toBeDisabled();
    // It is under the button's place, in the section itself: no dialog anywhere.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(section()).toContainElement(progress);
  });

  it("announces a changed event on the next look, and finishes with a result that is announced and takes the focus", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob)
      .mockResolvedValueOnce(jobOf({ events: [{ song: REF[0]!, step: "copy", state: "running", text: "Making the copy." }], songs: [{ ref_id: REF[0]!, title: "Holy Forever", steps: [], result: null }] }))
      .mockResolvedValueOnce(jobOf({
        state: "done",
        songs: [{ ref_id: REF[0]!, title: "Holy Forever", steps: [], result: result(REF[0]!, "Holy Forever", "added") }],
        report: report({ songs: [result(REF[0]!, "Holy Forever", "added"), result(REF[2]!, "Gratitude", "skipped", "Left out.")] }),
      }));
    await press(user);
    await tick(5);
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent("Holy Forever: Making the copy.");
    await tick();
    const done = await screen.findByTestId("add-songs-result");
    expect(done).toHaveAttribute("data-outcome", "done");
    expect(within(done).getByRole("heading")).toHaveTextContent("Done: 1 added");
    expect(within(done).getByRole("heading")).toHaveFocus();
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent(/^Done: 1 added\. Added 2 song\(s\) to the playlist/);
    expect(screen.getByTestId("add-songs-counts")).toHaveTextContent("1 added \u00B7 1 skipped");
    expect(within(screen.getAllByTestId("add-songs-result-song")[0]!).getByText("Added")).toBeInTheDocument();
    expect(within(screen.getAllByTestId("add-songs-result-song")[1]!).getByText("Skipped")).toBeInTheDocument();
    // The backend's sentence is not on the page: it is for a screen reader.
    expect(screen.getByTestId("add-songs-result-message")).toHaveClass("sr-only");
    expect(visibleText(done)).not.toMatch(/Added 2 song|Left out/);
    // The backup's file name is a small mono line; the version is small and only in the result.
    expect(screen.getByTestId("add-songs-backup")).toHaveTextContent(/^Backup Sunday Morning Service-20261122-081500\.json$/);
    expect(screen.getByTestId("add-songs-backup").querySelector("code")).toHaveClass("font-mono");
    expect(screen.getByTestId("add-songs-version")).toHaveTextContent("ProPresenter 19.0.1 (API v1)");
    expect(screen.getByTestId("add-songs-version")).toHaveClass("text-xs");
    expect(screen.queryByTestId("add-songs-differences")).toBeNull();
    // A finished job offers no Try again, and the button is there again for the next time.
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    // It does not look again once it has an answer.
    const looks = vi.mocked(api.getAddSongsJob).mock.calls.length;
    await tick(JOB_POLL_MS * 3);
    expect(vi.mocked(api.getAddSongsJob).mock.calls.length).toBe(looks);
  });

  it("lists, after the import, what was guessed for a song: 'Check in ProPresenter:' and nothing to tick", async () => {
    const user = await start();
    const how = { ...result(REF[0]!, "How Awesome", "added"), guesses: [guess(), guess({ section: "Count Off", position: 1, group_name: "", group_index: 1, rule: "in_order", text: "x" })] };
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", songs: [{ ref_id: REF[0]!, title: "How Awesome", steps: [], result: how }], report: report({ songs: [how, result(REF[1]!, "Plain Song", "added")] }) }));
    await press(user);
    await tick(5);
    const done = await screen.findByTestId("add-songs-result");
    const notes = within(done).getAllByTestId("add-songs-guess-note");
    expect(notes).toHaveLength(1);
    // The Check chip and one short line per guess; the whole sentence (and the rule) is the hover and screen-reader text.
    expect(within(notes[0]!).getByText("Check")).toBeInTheDocument();
    expect(within(notes[0]!).getAllByRole("listitem").map((li) => [visibleText(li), li.querySelector(".sr-only")?.textContent])).toEqual([
      ["Interlude (3rd use): 2nd group", "Interlude (3rd use) uses the 2nd \u201CInterlude\u201D group (Playback uses it once more than the song has groups, so the last group is used again)"],
      ["Count Off (1st use): 1st group", "Count Off (1st use) uses the 1st blank-named group"],
    ]);
    expect(within(done).queryByRole("checkbox")).toBeNull();
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent(/Check in ProPresenter: How Awesome: Interlude \(3rd use\)/);
    expect(within(screen.getAllByTestId("add-songs-result-song")[1]!).queryByTestId("add-songs-guess-note")).toBeNull();
  });

  it("lists what ProPresenter shows differently from the plan", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report({ outcome: "done_with_differences", message: "Added, but 2 things are not as planned.", differences: ["\u201CHoly Forever (1)\u201D shows the arrangement \u201CFull\u201D, not \u201CPlayback\u201D.", "The item \u201CMessage\u201D moved from place 5 to place 6.", "Item 7 \"Gratitude\": its duration reads 0 now, it was 80."] }) }));
    await press(user);
    await tick(5);
    const done = await screen.findByTestId("add-songs-result");
    expect(done).toHaveAttribute("data-outcome", "done_with_differences");
    expect(within(done).getByRole("heading")).toHaveTextContent("0 added, 3 differ");
    const lines = within(screen.getByTestId("add-songs-differences")).getAllByRole("listitem");
    // Short lines (`Item 7: duration 80 \u2192 0`); what the engine wrote is the hover text, and a line it does not know keeps its words without the full stop.
    expect(lines.map((li) => li.textContent)).toEqual([
      "\u201CHoly Forever (1)\u201D shows the arrangement \u201CFull\u201D, not \u201CPlayback\u201D",
      "The item \u201CMessage\u201D moved from place 5 to place 6",
      "Item 7: duration 80 \u2192 0",
    ]);
    expect(lines[2]).toHaveAttribute("title", "Item 7 \"Gratitude\": its duration reads 0 now, it was 80.");
  });

  it("needs_library_open: the plain instruction and Try again, which makes a fresh preview and applies it when it is clean", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValueOnce(jobOf({
      state: "done",
      report: report({ outcome: "needs_library_open", message: "ProPresenter has not loaded the new songs yet. Nothing was added to the playlist.", playlist_changed: false, songs: [result(REF[0]!, "Holy Forever", "not_loaded", "ProPresenter has not loaded this copy yet.")] }),
    }));
    await press(user);
    await tick(5);
    const done = await screen.findByTestId("add-songs-result");
    expect(done).toHaveAttribute("data-outcome", "needs_library_open");
    expect(screen.getByTestId("add-songs-open-library")).toHaveTextContent("Open the Playback library in ProPresenter, then Try again");
    expect(screen.getByRole("alert")).toHaveTextContent("Open the Playback library in ProPresenter, then Try again");
    expect(within(done).getByText("Not loaded yet")).toBeInTheDocument();
    vi.mocked(api.previewAddSongs).mockClear();
    vi.mocked(api.applyAddSongs).mockClear();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf());
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await screen.findByTestId("add-songs-progress");
    expect(api.previewAddSongs).toHaveBeenCalledTimes(1);
    expect(api.previewAddSongs).toHaveBeenCalledWith({ setlist_song_ids: [2000001, 2000002, 2000003], playlist_id: "PL-SUNDAY", after_header: "Worship" });
    expect(api.applyAddSongs).toHaveBeenCalledTimes(1);
  });

  it("Try again stops at a preview that is blocked now, and says so", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValueOnce(jobOf({ state: "done", report: report({ outcome: "stopped", message: "Stopped.", playlist_changed: false }) }));
    await press(user);
    await tick(5);
    await screen.findByTestId("add-songs-result");
    vi.mocked(api.applyAddSongs).mockClear();
    vi.mocked(api.previewAddSongs).mockResolvedValue(stuckPlan());
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await screen.findByTestId("add-songs-preview");
    expect(api.applyAddSongs).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Skip those and import the rest" })).toBeEnabled();
    expect(screen.queryByTestId("add-songs-result")).toBeNull();
  });

  it.each([
    ["stopped", "Stopped"],
    ["song_playing", "Song playing: wait"],
    ["live_on_screen", "Live on screen: clear it, then try again"],
    ["playlist_refused", "ProPresenter refused it"],
    ["playlist_failed", "Playlist not changed"],
    ["backup_failed", "Stopped: no backup"],
    ["not_reachable", "ProPresenter not answering"],
  ] as const)("%s: a short headline, the backend's sentence for screen readers only, and Try again", async (outcome, heading) => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report({ outcome, message: `The backend's sentence for ${outcome}.`, playlist_changed: false, backup: null }) }));
    await press(user);
    await tick(5);
    const done = await screen.findByTestId("add-songs-result");
    expect(within(done).getByRole("heading")).toHaveTextContent(heading);
    expect(screen.getByTestId("add-songs-result-message")).toHaveTextContent(`The backend's sentence for ${outcome}.`);
    expect(screen.getByTestId("add-songs-result-message")).toHaveClass("sr-only");
    expect(within(done).getByRole("heading").textContent!.split(/\s+/).length).toBeLessThanOrEqual(9);
    expect(screen.queryByTestId("add-songs-backup")).toBeNull();
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
    // Try again is under the result, in the section.
    expect(done.compareDocumentPosition(screen.getByRole("button", { name: "Try again" })) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("a job that stopped without an answer says so and keeps the sentence it was given", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "failed", error: "The job stopped unexpectedly. Look at the playlist in ProPresenter." }));
    await press(user);
    await tick(5);
    expect(await screen.findByTestId("add-songs-result-message")).toHaveTextContent("The job stopped unexpectedly. Look at the playlist in ProPresenter.");
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent("The job stopped unexpectedly.");
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("keeps looking when a look fails, and says so only after several", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockRejectedValue(new TypeError("Failed to fetch"));
    await press(user);
    await tick(5);
    expect(screen.queryByText(/lost contact/i)).toBeNull();
    await tick(JOB_POLL_MS * 5);
    expect(screen.getByText("Lost contact: still trying")).toBeInTheDocument();
    expect(screen.getByText("Lost contact: still trying")).toHaveAttribute("title", "StagePilot lost contact with itself for a moment. It keeps trying; the songs are still being imported.");
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report() }));
    await tick(JOB_POLL_MS * 2);
    expect(await screen.findByTestId("add-songs-result")).toBeInTheDocument();
    expect(screen.queryByText(/lost contact/i)).toBeNull();
  });

  it("adds Planning Center names as titles, then shows the result in place", async () => {
    const user = await start({ library: undefined });
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report({ songs: [result("t1", "Holy Forever", "added")] }) }));
    await press(user, addPlan([addSong({ ref_id: "t1", title: "Holy Forever", action: "as_is", file: "Holy Forever", sections: 0, key: null })]));
    await tick(5);
    expect(api.previewAddSongs).toHaveBeenCalledWith({ titles: ["Holy Forever", "Battle Belongs", "Gratitude"], playlist_id: "PL-SUNDAY", after_header: "Worship" });
    expect(await screen.findByTestId("add-songs-result")).toHaveAttribute("data-outcome", "done");
  });
});

describe("keyboard and screen reader", () => {
  it("everything can be done without a mouse, and Tab follows the page: options, list, then the one button", async () => {
    view();
    const user = await ready();
    await choose(user, "PL-SUNDAY", "Worship");
    screen.getByLabelText("Playlist").focus();
    const here = () => {
      const node = document.activeElement as HTMLInputElement | HTMLSelectElement | HTMLButtonElement;
      return node.labels?.[0]?.textContent ?? node.getAttribute("aria-label") ?? node.textContent?.replace(/[\u25B8\u25BE]/g, "").trim();
    };
    const seen: (string | null | undefined)[] = [];
    for (let i = 0; i < 7; i += 1) { await user.tab(); seen.push(here()); }
    // No Sections button on a card any more: the order is text on the card, so Tab goes from card to card.
    expect(seen).toEqual(["Section", "Select all", "Select none", "Holy Forever", "Battle Belongs", "Gratitude", "Import songs to ProPresenter"]);
    const box = screen.getByRole("checkbox", { name: "Holy Forever" });
    box.focus();
    await user.keyboard(" ");
    expect(box).not.toBeChecked();
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("2 of 3 selected");
    await user.keyboard("{Enter}");
    expect(box).toBeChecked();
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("3 of 3 selected");
    await user.keyboard(" ");
    addButton().focus();
    vi.mocked(api.previewAddSongs).mockResolvedValue(copyPlan());
    await user.keyboard("{Enter}");
    expect(await screen.findByTestId("add-songs-progress")).toBeInTheDocument();
  });

  it("every control has a name: the options are labelled, the list is a list of cards, each card a named checkbox", async () => {
    view();
    const user = await ready();
    await choose(user);
    expect(screen.getByLabelText("Playlist")).toBeInTheDocument();
    expect(screen.getByLabelText("Section")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Songs to import" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Songs" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Holy Forever" }).closest("li")).not.toBeNull();
    expect(screen.getByRole("group", { name: "Select songs" })).toBeInTheDocument();
    for (const box of screen.getAllByRole("checkbox")) expect(box).toHaveAccessibleName(/\S/);
    await press(user, stuckPlan());
    await screen.findByTestId("add-songs-preview");
    expect(screen.getByRole("list", { name: "What will happen to each song" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Preview" })).toBeInTheDocument();
  });

  it("the key, the number and the order are plain text, so a screen reader reads each song's key", async () => {
    view();
    await ready();
    const box = screen.getByRole("checkbox", { name: "Battle Belongs" });
    const row = box.closest("li")!;
    expect(row).toHaveTextContent("Ab \u00B7 mtID 31877");
    expect(row).toHaveTextContent("Verse 1 \u00B7 Chorus");
    expect(box).toHaveAccessibleDescription(/Ab \u00B7 mtID 31877/);
  });

  it("has one polite live region, announced even before anything is pressed", async () => {
    view();
    await ready();
    const live = screen.getByTestId("add-songs-live");
    expect(live).toHaveAttribute("aria-live", "polite");
    expect(live).toHaveAttribute("role", "status");
    expect(live.className).toMatch(/sr-only/);
  });

  it("moves the focus to the preview when it stops, and to the result when it ends", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    view();
    await ready(user);
    await choose(user, "PL-SUNDAY", "Worship");
    await press(user, stuckPlan());
    expect(await screen.findByRole("heading", { name: "Preview" })).toHaveFocus();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report() }));
    await user.click(screen.getByRole("button", { name: "Skip those and import the rest" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(5); });
    expect(await screen.findByTestId("add-songs-result")).toBeInTheDocument();
    expect(within(screen.getByTestId("add-songs-result")).getByRole("heading")).toHaveFocus();
  });
});
