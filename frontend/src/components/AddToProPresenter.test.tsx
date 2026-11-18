import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { ADD_SONGS_CHOICE_KEY, JOB_POLL_MS } from "../hooks/useAddSongs";
import { guess, helperStatus, jobOf, linkController, localStatus, noComputerStatus, playlists, report, result, step } from "../test/propresenterLinkFixtures";
import { addPlan, addSong } from "../test/propresenterLinkFixtures";
import { libraryController, libraryPlan, librarySong, librarySongOf, libraryStatus } from "../test/libraryFixtures";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { AddSongsPlan } from "../propresenterLink";
import { AddToProPresenter } from "./AddToProPresenter";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getProPresenterPlaylists: vi.fn(),
  previewAddSongs: vi.fn(),
  applyAddSongs: vi.fn(),
  getAddSongsJob: vi.fn(),
}));

const SONGS = [
  { title: "Holy Forever", key: "G", arrangement: "Default" },
  { title: "Battle Belongs", key: "Ab", arrangement: "Long ending" },
  { title: "Gratitude", key: "A", arrangement: "Default" },
];
const plan = () => libraryPlan(SONGS.map((s, i) => librarySongOf(s.title, i + 1, { library: librarySong({ title: s.title, position: i + 1, setlist_song_id: 2000001 + i, key: s.key, arrangement: s.arrangement, sections: [{ name: "Verse 1", start: 0, end: 1 }, { name: "Chorus", start: 1, end: 2 }] }) })));
const library = (over: Partial<PlaybackLibraryController> = {}) => libraryController("ready", { plan: plan(), ...over });
const REF = ["2000001", "2000002", "2000003"];

const copyPlan = (over: Partial<AddSongsPlan> = {}) => addPlan([
  addSong({ ref_id: REF[0]!, title: "Holy Forever", file: "Holy Forever (1)", mapping: [{ section: "Verse 1", group: "Verse 1", how: "full", line: null }, { section: "Chorus", group: "Chorus", how: "full", line: null }] }),
  addSong({ ref_id: REF[1]!, title: "Battle Belongs", file: "Battle Belongs (2)" }),
  addSong({ ref_id: REF[2]!, title: "Gratitude", file: "Gratitude (1)" }),
], over);

type Setup = { link?: ProPresenterLinkController; library?: PlaybackLibraryController; playing?: boolean };
const view = ({ link = linkController(helperStatus("ready")), library: lib = library(), playing = false }: Setup = {}) => {
  const utils = render(<AddToProPresenter library={lib} link={link} playing={playing} />);
  const again = (next: Setup) => utils.rerender(<AddToProPresenter library={next.library ?? lib} link={next.link ?? link} playing={next.playing ?? playing} />);
  return { ...utils, again };
};
const OPEN = { name: "Add to ProPresenter" };
const dialogButton = (name: string | RegExp) => within(screen.getByRole("dialog")).getByRole("button", { name });

async function open(user = userEvent.setup()) {
  await user.click(screen.getByRole("button", OPEN));
  await screen.findByRole("dialog");
  await screen.findByRole("option", { name: "Sunday Morning Service" });
  return user;
}
async function choose(user: ReturnType<typeof userEvent.setup>, playlist = "PL-SUNDAY", place?: string) {
  await user.selectOptions(screen.getByLabelText("Playlist"), playlist);
  if (place !== undefined) await user.selectOptions(screen.getByLabelText("Place"), place);
}
async function preview(user: ReturnType<typeof userEvent.setup>, made: AddSongsPlan = copyPlan()) {
  vi.mocked(api.previewAddSongs).mockResolvedValue(made);
  await user.click(dialogButton("Preview"));
  await screen.findByTestId("add-songs-preview");
}

beforeEach(() => {
  window.localStorage.clear();
  vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
  vi.mocked(api.previewAddSongs).mockResolvedValue(copyPlan());
  vi.mocked(api.applyAddSongs).mockResolvedValue({ job_id: "fedcba9876543210fedcba9876543210", plan_id: "p", state: "running" });
});
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); window.localStorage.clear(); });

describe("when the button is drawn: the ProPresenter Link AND the Library Link AND a paired setlist", () => {
  it("is there when all three hold, for another computer and for this one", () => {
    const { unmount } = view();
    expect(screen.getByRole("button", OPEN)).toBeInTheDocument();
    unmount();
    view({ link: linkController(localStatus()) });
    expect(screen.getByRole("button", OPEN)).toBeInTheDocument();
  });

  it("a Library Link that is behind still works, so the button is there", () => {
    view({ library: libraryController("outdated", { plan: plan() }) });
    expect(screen.getByRole("button", OPEN)).toBeInTheDocument();
  });

  it.each([
    ["no status read yet", { link: linkController(null) }],
    ["no ProPresenter computer saved", { link: linkController(noComputerStatus()) }],
    ["Link not installed", { link: linkController(helperStatus("not_installed")) }],
    ["Link needs an update (the backend wants it ready)", { link: linkController(helperStatus("outdated")) }],
    ["Link needs the command again", { link: linkController(helperStatus("needs_command")) }],
    ["Link not answering", { link: linkController(helperStatus("not_answering")) }],
    ["Link being checked", { link: linkController(helperStatus("checking")) }],
    ["Library Link not installed", { library: libraryController("not_installed") }],
    ["Library Link not answering", { library: libraryController("unreachable") }],
    ["Library Link needs the command again", { library: libraryController("needs_key") }],
    ["Library Link data is an old copy", { library: libraryController("ready", { status: libraryStatus("ready", { stale: true }), plan: plan() }) }],
    ["no plan read yet", { library: libraryController("ready", { plan: null }) }],
    ["the plan is not matched to a setlist", { library: libraryController("ready", { plan: libraryPlan([librarySongOf("A", 1)], { setlist: null }) }) }],
    ["the plan says no setlist", { library: libraryController("ready", { plan: libraryPlan([], { state: "no_setlist" }) }) }],
    ["the setlist is empty", { library: libraryController("ready", { plan: libraryPlan([]) }) }],
    ["no plan song is paired with a Playback song", { library: libraryController("ready", { plan: libraryPlan([librarySongOf("A", 1, { status: "missing", confidence: null, library: null })]) }) }],
  ] as [string, Setup][])("draws nothing when: %s", (_name, setup) => {
    const { container } = view(setup as Setup);
    expect(screen.queryByRole("button", OPEN)).toBeNull();
    expect(container).toBeEmptyDOMElement();
  });
});

describe("without the ProPresenter Link at all", () => {
  it("draws nothing when no controller is given (a panel that does not know the Link)", () => {
    const { container } = render(<AddToProPresenter library={library()} />);
    expect(container).toBeEmptyDOMElement();
    const again = render(<AddToProPresenter link={linkController(helperStatus("ready"))} />);
    expect(again.container).toBeEmptyDOMElement();
  });
});

describe("the window: songs, playlist, place", () => {
  it("opens as a named dialog with the three songs ticked, each with title, key chip and arrangement", async () => {
    view();
    await open();
    const dialog = screen.getByRole("dialog", { name: "Add to ProPresenter" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAccessibleDescription(/Playback songs into a ProPresenter playlist/);
    const rows = within(screen.getByRole("list", { name: "Songs to add" })).getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(rows.map((row) => row.textContent)).toEqual(["Holy ForeverKey GDefault", "Battle BelongsKey AbLong ending", "GratitudeKey ADefault"]);
    for (const box of within(screen.getByRole("list", { name: "Songs to add" })).getAllByRole("checkbox")) expect(box).toBeChecked();
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("3 of 3 ticked");
    expect(screen.getByTestId("add-songs-key-note")).toHaveTextContent("ProPresenter does not show the key for these songs; it is written into the copy and shown here.");
  });

  it("a song can be ticked off by its name, and all of them at once", async () => {
    view();
    const user = await open();
    await user.click(screen.getByRole("checkbox", { name: /Battle Belongs/ }));
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("2 of 3 ticked");
    await user.click(screen.getByRole("button", { name: "Tick all" }));
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("3 of 3 ticked");
    await user.click(screen.getByRole("button", { name: "Untick all" }));
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("0 of 3 ticked");
  });

  it("a song Playback gave no number cannot be added, and says so", async () => {
    const lib = library({ plan: libraryPlan([librarySongOf("Holy Forever", 1), librarySongOf("Odd One", 2, { library: librarySong({ title: "Odd One", position: 2, setlist_song_id: null }) })]) });
    view({ library: lib });
    await open();
    expect(screen.getByTestId("add-songs-unnamed")).toHaveTextContent("Odd One");
    expect(screen.getByTestId("add-songs-unnamed")).toHaveTextContent("Playback gave this song no number, so it cannot be added.");
    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
  });

  it("lists the playlists (a folder is named) and, once one is chosen, After each header and At the end", async () => {
    view();
    const user = await open();
    const playlist = screen.getByLabelText("Playlist");
    expect(within(playlist).getAllByRole("option").map((o) => o.textContent)).toEqual(["Sunday Morning Service", "Wednesday Night", "Special services / Youth Night"]);
    // The place waits for a playlist.
    expect(playlist).toHaveValue("");
    expect(screen.getByLabelText("Place")).toBeDisabled();
    await choose(user);
    expect(within(screen.getByLabelText("Place")).getAllByRole("option").map((o) => o.textContent)).toEqual(["After \u201CWelcome\u201D", "After \u201CWorship\u201D", "After \u201CMessage\u201D", "At the end"]);
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-WED");
    expect(within(screen.getByLabelText("Place")).getAllByRole("option").map((o) => o.textContent)).toEqual(["At the end"]);
  });

  it("says when the playlists cannot be read, and Look again reads them", async () => {
    vi.mocked(api.getProPresenterPlaylists).mockRejectedValueOnce(new api.ApiError("ProPresenter did not answer on 192.168.1.60.", 502, "propresenter_unreachable"));
    view();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", OPEN));
    expect(await screen.findByText("ProPresenter did not answer on 192.168.1.60.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Look again" }));
    expect(await screen.findByRole("option", { name: "Sunday Morning Service" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Look again" })).toBeNull();
  });
});

describe("the last playlist and place are remembered in this browser, and nowhere else", () => {
  it("saves each choice to browser storage and restores it the next time", async () => {
    const { unmount } = view();
    const user = await open();
    await choose(user, "PL-SUNDAY", "Worship");
    expect(JSON.parse(window.localStorage.getItem(ADD_SONGS_CHOICE_KEY) ?? "null")).toEqual({ playlist_id: "PL-SUNDAY", place: "Worship" });
    unmount();
    view();
    await open();
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    expect(screen.getByLabelText("Place")).toHaveValue("Worship");
  });

  it("falls back to the end when that header is not in the playlist any more, and to nothing when the playlist is gone", async () => {
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify({ playlist_id: "PL-SUNDAY", place: "Old header" }));
    const { unmount } = view();
    await open();
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    expect(screen.getByLabelText("Place")).toHaveValue("");
    unmount();
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, JSON.stringify({ playlist_id: "GONE", place: "" }));
    view();
    await open();
    expect(screen.getByLabelText("Playlist")).toHaveValue("");
  });

  it("ignores a damaged value, and works when storage refuses to write", async () => {
    window.localStorage.setItem(ADD_SONGS_CHOICE_KEY, "{not json");
    const spy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("quota"); });
    view();
    const user = await open();
    expect(screen.getByLabelText("Playlist")).toHaveValue("");
    await choose(user);
    expect(screen.getByLabelText("Playlist")).toHaveValue("PL-SUNDAY");
    spy.mockRestore();
  });

  it("sends nothing about the choice to StagePilot", async () => {
    view();
    const user = await open();
    await choose(user, "PL-SUNDAY", "Worship");
    expect(api.previewAddSongs).not.toHaveBeenCalled();
    expect(api.applyAddSongs).not.toHaveBeenCalled();
  });
});

describe("Preview", () => {
  it("is off with its reason until a playlist is chosen, and while no song is ticked", async () => {
    view();
    const user = await open();
    expect(dialogButton("Preview")).toBeDisabled();
    expect(screen.getByTestId("add-songs-reason")).toHaveTextContent("Choose a playlist.");
    await choose(user);
    expect(dialogButton("Preview")).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Untick all" }));
    expect(dialogButton("Preview")).toBeDisabled();
    expect(screen.getByTestId("add-songs-reason")).toHaveTextContent("Tick at least one song.");
    expect(dialogButton("Preview")).toHaveAccessibleDescription("Tick at least one song.");
  });

  it("says what pressing Add songs does, beside the buttons and in the preview, with the playlist's name", async () => {
    view();
    const user = await open();
    const sentence = "This imports songs, arrangements and keys into";
    // Before a playlist is chosen: the plain words, beside the buttons.
    expect(screen.getByTestId("add-songs-what-it-does")).toHaveTextContent(`${sentence} the playlist. The playlist's contents are replaced with what the preview shows.`);
    await choose(user, "PL-SUNDAY", "Worship");
    expect(screen.getByTestId("add-songs-what-it-does")).toHaveTextContent(`${sentence} Sunday Morning Service. The playlist's contents are replaced with what the preview shows.`);
    await preview(user, copyPlan());
    expect(screen.getByTestId("add-songs-imports")).toHaveTextContent(`${sentence} Sunday Morning Service. The playlist's contents are replaced with what the preview shows.`);
    expect(screen.getByTestId("add-songs-nothing-changes")).toHaveTextContent("Nothing in ProPresenter changes until you press Add songs.");
    // Nothing about hand-chosen arrangements is added.
    expect(screen.getByTestId("add-songs-window").textContent).not.toMatch(/hand-?(chosen|picked)|reset/i);
  });

  it("lists the guesses in the preview and still lets the songs be added", async () => {
    view();
    const user = await open();
    await choose(user, "PL-SUNDAY", "Worship");
    const song = addSong({
      ref_id: REF[0]!, title: "How Awesome", file: "How Awesome (1)", guesses: [guess()],
      mapping: [
        { section: "Verse 1", group: "Verse 1", how: "full", line: null },
        { section: "Interlude", group: "Interlude", how: "guessed", line: guess().text },
      ],
    });
    await preview(user, addPlan([song]));
    const lines = screen.getByTestId("add-songs-mapping");
    expect(lines).toHaveTextContent("1 of 2 sections matched exactly, 1 guessed (check it in ProPresenter).");
    expect(lines).toHaveTextContent("Interlude (3rd use) uses the 2nd \"Interlude\" group: check it in ProPresenter.");
    // A guess is no block: no skip box, no extra tick, and Add songs is on.
    expect(screen.queryByTestId("add-songs-skip")).toBeNull();
    expect(dialogButton("Add songs")).toBeEnabled();
    expect(screen.queryByTestId("add-songs-reason")).toBeNull();
  });

  it("asks for the ticked songs by Playback's numbers, in setlist order, after the chosen header", async () => {
    view();
    const user = await open();
    await choose(user, "PL-SUNDAY", "Worship");
    await user.click(screen.getByRole("checkbox", { name: /Battle Belongs/ }));
    await preview(user);
    expect(api.previewAddSongs).toHaveBeenCalledWith({ setlist_song_ids: [2000001, 2000003], playlist_id: "PL-SUNDAY", after_header: "Worship" });
  });

  it("asks for the end of the playlist with place: end", async () => {
    view();
    const user = await open();
    await choose(user, "PL-WED");
    await preview(user);
    expect(api.previewAddSongs).toHaveBeenCalledWith({ setlist_song_ids: [2000001, 2000002, 2000003], playlist_id: "PL-WED", place: "end" });
  });

  it("says nothing changes until Add songs, and what the playlist side does, in the owner's words", async () => {
    view();
    const user = await open();
    await choose(user, "PL-SUNDAY", "Worship");
    await preview(user, copyPlan({ to_add: 4 }));
    expect(screen.getByTestId("add-songs-nothing-changes")).toHaveTextContent("Nothing in ProPresenter changes until you press Add songs.");
    expect(screen.getByTestId("add-songs-playlist-line")).toHaveTextContent("Adds 4 songs after Worship in Sunday Morning Service. Other items stay as they are. They go between \u201CWorship\u201D and \u201CMessage\u201D.");
    expect(screen.getByTestId("add-songs-version")).toHaveTextContent("ProPresenter 19.0.1 (API v1)");
  });

  it("renders every outcome as a plain checklist line with a stamp (a mark and a word)", async () => {
    view();
    const user = await open();
    await choose(user, "PL-SUNDAY", "Worship");
    const made = addPlan([
      addSong({ ref_id: REF[0]!, title: "Holy Forever", action: "reuse", file: "Holy Forever (1)", already_in_playlist: false }),
      addSong({ ref_id: REF[1]!, title: "Battle Belongs", action: "copy", file: "Battle Belongs (2)", sections: 2 }),
      addSong({ ref_id: REF[2]!, title: "Gratitude", action: "import_then_copy", file: "Gratitude (1)", import_as: "Gratitude", sections: 2 }),
      addSong({ ref_id: "4", title: "Odd Song", action: "needs_choice", reason: "Two files are this song.", choices: ["Songs / A.pro", "Songs / B.pro"] }),
      addSong({ ref_id: "5", title: "Broken Song", action: "blocked", reason: "The song has no group called Bridge." }),
    ]);
    await preview(user, made);
    const lines = screen.getAllByTestId("add-songs-preview-song");
    expect(lines.map((line) => line.getAttribute("data-action"))).toEqual(["reuse", "copy", "import_then_copy", "needs_choice", "blocked"]);
    const sentence = (i: number) => within(lines[i]!).getByTestId("add-songs-song-sentence").textContent;
    expect(sentence(0)).toBe("Already in ProPresenter as \u201CHoly Forever (1)\u201D.");
    expect(sentence(1)).toBe("New copy \u201CBattle Belongs (2)\u201D with Playback's order (Verse 1, Chorus).");
    expect(sentence(2)).toBe("Will be loaded from ProPresenter search first (as \u201CGratitude\u201D), then a new copy \u201CGratitude (1)\u201D with Playback's order (Verse 1, Chorus).");
    expect(sentence(3)).toBe("Needs a choice: Two files are this song. Files: \u201CSongs / A.pro\u201D, \u201CSongs / B.pro\u201D.");
    expect(sentence(4)).toBe("Blocked: The song has no group called Bridge.");
    expect(lines.map((line) => line.querySelector("[data-tone]")?.textContent)).toEqual(["\u2713Already there", "+New copy", "\u2193Load first", "?Needs a choice", "!Blocked"]);
    // Every stamp carries a word: state is never colour alone.
    for (const line of lines) expect(line.querySelector("[data-tone]")?.textContent?.replace(/[^A-Za-z ]/g, "")).toMatch(/[A-Za-z]{4}/);
  });

  it("says a song already in the playlist is not added twice", async () => {
    view();
    const user = await open();
    await choose(user);
    await preview(user, addPlan([addSong({ ref_id: REF[0]!, title: "Holy Forever", action: "reuse", file: "Holy Forever (1)", already_in_playlist: true })], { to_add: 0, can_apply: false }));
    expect(screen.getByTestId("add-songs-song-sentence")).toHaveTextContent("It is already in this playlist, so it is not added again.");
    expect(screen.getByTestId("add-songs-playlist-line")).toHaveTextContent("Adds nothing to Sunday Morning Service. Other items stay as they are.");
  });

  it("shows how sections were matched (each sentence once) and the notes", async () => {
    view();
    const user = await open();
    await choose(user);
    const line = "\u201CCount Off\u201D: matched by name to the blank group, the only group with that name.";
    await preview(user, addPlan([addSong({
      ref_id: REF[0]!, title: "Holy Forever", notes: [line, "The key G is saved in the copy."],
      mapping: [{ section: "Count Off", group: null, how: "only", line }, { section: "Verse 1", group: "Verse 1", how: "full", line: null }],
    })]));
    expect(screen.getByTestId("add-songs-mapping")).toHaveTextContent("All 2 sections matched exactly to the song's own groups.");
    expect(screen.getAllByText(line)).toHaveLength(1);
    expect(screen.getByTestId("add-songs-notes")).toHaveTextContent("The key G is saved in the copy.");
    expect(screen.getByRole("list", { name: "How Holy Forever was matched" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Notes about Holy Forever" })).toBeInTheDocument();
  });

  it("shows the plan's warnings and a blocked playlist side", async () => {
    view();
    const user = await open();
    await choose(user, "PL-SUNDAY", "Worship");
    const made = copyPlan({ warnings: ["ProPresenter is older than the one StagePilot was tried with."] });
    made.playlist.blocked = "The header \u201CWorship\u201D is in the playlist twice.";
    await preview(user, made);
    expect(screen.getByTestId("add-songs-warnings")).toHaveTextContent("ProPresenter is older than the one StagePilot was tried with.");
    expect(screen.getByTestId("add-songs-playlist-blocked")).toHaveTextContent("Blocked: The header \u201CWorship\u201D is in the playlist twice.");
  });

  it("the window before the first preview says what a preview is, and that nothing changes", async () => {
    view();
    await open();
    expect(screen.getByTestId("add-songs-no-preview")).toHaveTextContent("Press Preview to see what would happen to each song. Nothing in ProPresenter changes until you press Add songs.");
  });

  it("an error on Preview is a sentence, never a raw status, and the choice stays", async () => {
    view();
    const user = await open();
    await choose(user, "PL-SUNDAY", "Worship");
    vi.mocked(api.previewAddSongs).mockRejectedValueOnce(new api.ApiError("That playlist is not in ProPresenter any more.", 404, "playlist_not_found"));
    await user.click(dialogButton("Preview"));
    expect(await screen.findByTestId("add-songs-problem")).toHaveTextContent("That playlist is not in ProPresenter any more.");
    expect(screen.getByTestId("add-songs-problem")).toHaveAttribute("data-code", "playlist_not_found");
    expect(screen.getByLabelText("Place")).toHaveValue("Worship");
    vi.mocked(api.previewAddSongs).mockRejectedValueOnce(new api.ApiError("{\"x\":1}", 502));
    await user.click(dialogButton("Preview"));
    expect(await screen.findByText("StagePilot hit an error. Try again.")).toBeInTheDocument();
    vi.mocked(api.previewAddSongs).mockRejectedValueOnce(new TypeError("Failed to fetch"));
    await user.click(dialogButton("Preview"));
    expect(await screen.findByText(/Can't reach StagePilot/)).toBeInTheDocument();
  });

  it("announces the preview politely, and moves the focus to it", async () => {
    view();
    const user = await open();
    await choose(user);
    await preview(user, copyPlan({ to_add: 1 }));
    expect(screen.getByTestId("add-songs-live")).toHaveAttribute("aria-live", "polite");
    expect(screen.getByTestId("add-songs-live")).toHaveAttribute("role", "status");
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent("Preview ready. 1 song would be added. Nothing in ProPresenter changes until you press Add songs.");
    expect(screen.getByRole("heading", { name: "Preview" })).toHaveFocus();
  });
});

describe("Add songs: when it is off, and why", () => {
  const reason = () => screen.getByTestId("add-songs-reason").textContent;
  const add = () => dialogButton("Add songs");

  it("is off until a preview has been looked at", async () => {
    view();
    const user = await open();
    await choose(user);
    expect(add()).toBeDisabled();
    expect(reason()).toBe("Press Preview first to see what will change.");
    expect(api.applyAddSongs).not.toHaveBeenCalled();
    await user.click(add());
    expect(api.applyAddSongs).not.toHaveBeenCalled();
  });

  it("is on after a preview with something to add", async () => {
    view();
    const user = await open();
    await choose(user);
    await preview(user);
    expect(add()).toBeEnabled();
    expect(screen.queryByTestId("add-songs-reason")).toBeNull();
  });

  it("goes stale when a song, the playlist or the place changes: Add songs is off until Preview again", async () => {
    view();
    const user = await open();
    await choose(user, "PL-SUNDAY", "Worship");
    await preview(user);
    await user.click(screen.getByRole("checkbox", { name: /Gratitude/ }));
    expect(add()).toBeDisabled();
    expect(screen.getByTestId("add-songs-stale")).toHaveTextContent("You changed something since this preview. Press Preview again.");
    expect(reason()).toBe("You changed something. Press Preview again.");
    await preview(user);
    expect(add()).toBeEnabled();
    await user.selectOptions(screen.getByLabelText("Place"), "");
    expect(add()).toBeDisabled();
    await preview(user);
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-WED");
    expect(add()).toBeDisabled();
  });

  it("is off while a song is playing, with that reason, and on again when it ends", async () => {
    const { again } = view({ playing: true });
    const user = await open();
    await choose(user);
    await preview(user);
    expect(add()).toBeDisabled();
    expect(reason()).toBe("A song is playing. Wait until it has ended, then try again.");
    expect(screen.getByTestId("add-songs-playing")).toHaveTextContent("A song is playing. You can look at a preview now");
    // A preview writes nothing, so it is still allowed.
    expect(dialogButton("Preview")).toBeEnabled();
    again({ playing: false });
    expect(add()).toBeEnabled();
    expect(screen.queryByTestId("add-songs-playing")).toBeNull();
  });

  it("is off while anything is blocked and not skipped; Skip blocked songs turns it on, and the skipped song says so", async () => {
    view();
    const user = await open();
    await choose(user);
    await preview(user, addPlan([
      addSong({ ref_id: REF[0]!, title: "Holy Forever" }),
      addSong({ ref_id: REF[1]!, title: "Battle Belongs", action: "blocked", reason: "Not in ProPresenter." }),
      addSong({ ref_id: REF[2]!, title: "Gratitude", action: "needs_choice", reason: "Two files." }),
    ]));
    expect(add()).toBeDisabled();
    expect(reason()).toBe("Some songs are blocked. Tick \u201CSkip blocked songs\u201D to add the others.");
    const skip = screen.getByRole("checkbox", { name: "Skip blocked songs (2)" });
    expect(skip).not.toBeChecked();
    await user.click(skip);
    expect(add()).toBeEnabled();
    expect(screen.getAllByTestId("add-songs-song-skipped")).toHaveLength(2);
    await user.click(skip);
    expect(add()).toBeDisabled();
  });

  it("is off when there is nothing to add, and when the playlist side is blocked", async () => {
    view();
    const user = await open();
    await choose(user);
    await preview(user, addPlan([addSong({ ref_id: REF[0]!, title: "Holy Forever", action: "reuse", already_in_playlist: true })], { can_apply: false, to_add: 0 }));
    expect(add()).toBeDisabled();
    expect(reason()).toBe("There is nothing to add.");
    const blocked = copyPlan();
    blocked.playlist.blocked = "That header is in the playlist twice.";
    await preview(user, blocked);
    expect(add()).toBeDisabled();
    expect(reason()).toBe("Blocked: That header is in the playlist twice.");
  });

  it("is off when the ProPresenter Link stops being ready while the window is open", async () => {
    const { again } = view();
    const user = await open();
    await choose(user);
    await preview(user);
    again({ link: linkController(helperStatus("not_answering")) });
    expect(add()).toBeDisabled();
    expect(dialogButton("Preview")).toBeDisabled();
    expect(reason()).toBe("The ProPresenter Link is not ready. Check it in the Presentation panel.");
    expect(screen.getByTestId("add-songs-link-gone")).toHaveTextContent("The ProPresenter Link is not ready now. Nothing was changed.");
  });
});

describe("Add songs: the job, step by step", () => {
  const start = async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    view();
    await open(user);
    await choose(user, "PL-SUNDAY", "Worship");
    await preview(user);
    return user;
  };
  const tick = (ms = JOB_POLL_MS) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

  it("applies exactly the preview, with nothing skipped when nothing was stuck", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf());
    await user.click(dialogButton("Add songs"));
    expect(api.applyAddSongs).toHaveBeenCalledWith("0123456789abcdef0123456789abcdef", []);
  });

  it("names the stuck songs when they are skipped", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    view();
    await open(user);
    await choose(user);
    await preview(user, addPlan([addSong({ ref_id: REF[0]!, title: "Holy Forever" }), addSong({ ref_id: REF[1]!, title: "Battle Belongs", action: "blocked", reason: "x" })]));
    await user.click(screen.getByRole("checkbox", { name: "Skip blocked songs (1)" }));
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf());
    await user.click(dialogButton("Add songs"));
    expect(api.applyAddSongs).toHaveBeenCalledWith("0123456789abcdef0123456789abcdef", [REF[1]]);
  });

  it("shows progress per song and per step, announces the newest thing politely, and cannot be closed while it runs", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValueOnce(jobOf({
      steps: [step("check", "done", "ProPresenter says: ProPresenter 19.0.1."), step("backup", "running", "Saving the playlist.")],
      songs: [
        { ref_id: REF[0]!, title: "Holy Forever", steps: [step("copy", "done", "Made \"Holy Forever (1)\".")], result: null },
        { ref_id: REF[2]!, title: "Gratitude", steps: [], result: null },
      ],
      events: [{ song: null, step: "backup", state: "running", text: "Saving the playlist." }],
    }));
    await user.click(dialogButton("Add songs"));
    await tick(5);
    const progress = screen.getByTestId("add-songs-progress");
    expect(within(progress).getByRole("heading", { name: "Adding songs" })).toHaveFocus();
    expect(screen.getByTestId("add-songs-now")).toHaveTextContent("Now: Saving the playlist.");
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent("Saving the playlist.");
    expect(within(screen.getByRole("list", { name: "Steps for the playlist" })).getAllByRole("listitem").map((li) => li.getAttribute("data-state"))).toEqual(["done", "running"]);
    const songs = screen.getAllByTestId("add-songs-job-song");
    expect(songs.map((song) => song.querySelector("span")?.textContent)).toEqual(["Holy Forever", "Gratitude"]);
    expect(within(screen.getByRole("list", { name: "Steps for Holy Forever" })).getByText("Making the copy")).toBeInTheDocument();
    // Each step has a word as well as a mark.
    expect(within(progress).getAllByText("Done").length).toBeGreaterThan(0);
    expect(within(progress).getByText("Working")).toBeInTheDocument();
    // No way out while it runs: no Close, no Add songs, Escape does nothing.
    expect(within(screen.getByRole("dialog")).queryByRole("button", { name: "Close" })).toBeNull();
    expect(within(screen.getByRole("dialog")).queryByRole("button", { name: "Add songs" })).toBeNull();
    expect(screen.getByTestId("add-songs-reason")).toHaveTextContent("Adding songs. Keep this window open until it says it is finished.");
    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
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
    await user.click(dialogButton("Add songs"));
    await tick(5);
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent("Holy Forever: Making the copy.");
    await tick();
    const done = await screen.findByTestId("add-songs-result");
    expect(done).toHaveAttribute("data-outcome", "done");
    expect(within(done).getByRole("heading")).toHaveTextContent("Added");
    expect(within(done).getByRole("heading")).toHaveFocus();
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent(/^Added\. Added 2 song\(s\) to the playlist/);
    expect(screen.getByTestId("add-songs-counts")).toHaveTextContent("1 added, 1 skipped");
    expect(within(screen.getAllByTestId("add-songs-result-song")[0]!).getByText("Added")).toBeInTheDocument();
    expect(screen.getByTestId("add-songs-backup")).toHaveTextContent("The playlist was saved first as Sunday Morning Service-20261122-081500.json, beside StagePilot's settings.");
    expect(screen.queryByTestId("add-songs-differences")).toBeNull();
    // Closing is possible again, and a finished job offers no Try again.
    expect(dialogButton("Close")).toBeEnabled();
    expect(within(screen.getByRole("dialog")).queryByRole("button", { name: "Try again" })).toBeNull();
    // It does not look again once it has an answer.
    const looks = vi.mocked(api.getAddSongsJob).mock.calls.length;
    await tick(JOB_POLL_MS * 3);
    expect(vi.mocked(api.getAddSongsJob).mock.calls.length).toBe(looks);
  });

  it("lists, after the import, what was guessed for a song: 'Check in ProPresenter:' and nothing to tick", async () => {
    const user = await start();
    const how = { ...result(REF[0]!, "How Awesome", "added"), guesses: [guess(), guess({ section: "Count Off", position: 1, group_name: "", group_index: 1, rule: "in_order", text: "x" })] };
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({
      state: "done",
      songs: [{ ref_id: REF[0]!, title: "How Awesome", steps: [], result: how }],
      report: report({ songs: [how, result(REF[1]!, "Plain Song", "added")] }),
    }));
    await user.click(dialogButton("Add songs"));
    await tick(5);
    const done = await screen.findByTestId("add-songs-result");
    const notes = within(done).getAllByTestId("add-songs-guess-note");
    expect(notes).toHaveLength(1);
    expect(notes[0]).toHaveTextContent("Check in ProPresenter:");
    expect(within(notes[0]!).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "Interlude (3rd use) uses the 2nd \u201CInterlude\u201D group (Playback uses it once more than the song has groups, so the last group is used again)",
      "Count Off (1st use) uses the 1st blank-named group",
    ]);
    expect(within(done).queryByRole("checkbox")).toBeNull();
    expect(done).toHaveAttribute("data-outcome", "done");
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent(/Check in ProPresenter: How Awesome: Interlude \(3rd use\)/);
    expect(within(screen.getAllByTestId("add-songs-result-song")[1]!).queryByTestId("add-songs-guess-note")).toBeNull();
  });

  it("lists what ProPresenter shows differently from the plan", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({
      state: "done",
      report: report({ outcome: "done_with_differences", message: "Added, but 2 things are not as planned.", differences: ["\u201CHoly Forever (1)\u201D shows the arrangement \u201CFull\u201D, not \u201CPlayback\u201D.", "The item \u201CMessage\u201D moved from place 5 to place 6."] }),
    }));
    await user.click(dialogButton("Add songs"));
    await tick(5);
    const done = await screen.findByTestId("add-songs-result");
    expect(done).toHaveAttribute("data-outcome", "done_with_differences");
    expect(within(done).getByRole("heading")).toHaveTextContent("Added, but ProPresenter shows something different");
    const differences = screen.getByTestId("add-songs-differences");
    expect(differences).toHaveTextContent("ProPresenter shows something different from what was planned:");
    expect(within(differences).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "\u201CHoly Forever (1)\u201D shows the arrangement \u201CFull\u201D, not \u201CPlayback\u201D.",
      "The item \u201CMessage\u201D moved from place 5 to place 6.",
    ]);
  });

  it("needs_library_open: the plain instruction and Try again, which makes a fresh preview (nothing is added by it)", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({
      state: "done",
      report: report({ outcome: "needs_library_open", message: "ProPresenter has not loaded the new songs yet. Nothing was added to the playlist.", playlist_changed: false, songs: [result(REF[0]!, "Holy Forever", "not_loaded", "ProPresenter has not loaded this copy yet.")] }),
    }));
    await user.click(dialogButton("Add songs"));
    await tick(5);
    const done = await screen.findByTestId("add-songs-result");
    expect(done).toHaveAttribute("data-outcome", "needs_library_open");
    expect(screen.getByTestId("add-songs-open-library")).toHaveTextContent("Open the Playback library in ProPresenter once, then press Try again.");
    expect(screen.getByRole("alert")).toHaveTextContent("Open the Playback library in ProPresenter once, then press Try again.");
    expect(within(done).getByText("Not loaded yet")).toBeInTheDocument();
    vi.mocked(api.previewAddSongs).mockClear();
    vi.mocked(api.applyAddSongs).mockClear();
    await user.click(dialogButton("Try again"));
    await screen.findByTestId("add-songs-preview");
    expect(api.previewAddSongs).toHaveBeenCalledTimes(1);
    expect(api.previewAddSongs).toHaveBeenCalledWith({ setlist_song_ids: [2000001, 2000002, 2000003], playlist_id: "PL-SUNDAY", after_header: "Worship" });
    expect(api.applyAddSongs).not.toHaveBeenCalled();
    expect(dialogButton("Add songs")).toBeEnabled();
  });

  it.each([
    ["stopped", "Stopped"],
    ["song_playing", "Stopped: a song is playing"],
    ["live_on_screen", "Stopped: ProPresenter is showing something on screen"],
    ["playlist_refused", "ProPresenter did not accept the change"],
    ["playlist_failed", "The playlist could not be changed"],
    ["backup_failed", "Nothing was changed: the playlist could not be saved first"],
    ["not_reachable", "ProPresenter did not answer"],
  ] as const)("%s: a headline, the backend's sentence and Try again", async (outcome, heading) => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report({ outcome, message: `The backend's sentence for ${outcome}.`, playlist_changed: false, backup: null }) }));
    await user.click(dialogButton("Add songs"));
    await tick(5);
    const done = await screen.findByTestId("add-songs-result");
    expect(within(done).getByRole("heading")).toHaveTextContent(heading);
    expect(screen.getByTestId("add-songs-result-message")).toHaveTextContent(`The backend's sentence for ${outcome}.`);
    expect(screen.queryByTestId("add-songs-backup")).toBeNull();
    expect(dialogButton("Try again")).toBeEnabled();
  });

  it("a job that stopped without an answer says so and keeps the sentence it was given", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "failed", error: "The job stopped unexpectedly. Look at the playlist in ProPresenter." }));
    await user.click(dialogButton("Add songs"));
    await tick(5);
    expect(await screen.findByTestId("add-songs-result-message")).toHaveTextContent("The job stopped unexpectedly. Look at the playlist in ProPresenter.");
    expect(screen.getByTestId("add-songs-live")).toHaveTextContent("The job stopped unexpectedly.");
  });

  it("keeps looking when a look fails, and says so only after several", async () => {
    const user = await start();
    vi.mocked(api.getAddSongsJob).mockRejectedValue(new TypeError("Failed to fetch"));
    await user.click(dialogButton("Add songs"));
    await tick(5);
    expect(screen.queryByText(/lost contact/)).toBeNull();
    await tick(JOB_POLL_MS * 5);
    expect(screen.getByText(/StagePilot lost contact with itself for a moment/)).toBeInTheDocument();
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report() }));
    await tick(JOB_POLL_MS * 2);
    expect(await screen.findByTestId("add-songs-result")).toBeInTheDocument();
    expect(screen.queryByText(/lost contact/)).toBeNull();
  });

  it.each([
    ["song_playing", 409, "A song is playing. Wait until it has ended, then try again.", false],
    ["plan_changed", 409, "ProPresenter or the playlist changed since the preview. Look at the preview again.", true],
    ["unknown_plan", 404, "That preview is too old or not known. Look at the preview again.", true],
    ["songs_blocked", 409, "Some songs cannot be added. Leave them out first.", true],
    ["busy", 409, "StagePilot is already adding songs. Wait until it has finished.", false],
    ["link_not_ready", 409, "The ProPresenter Link is not ready.", false],
  ])("refused at once with %s: the sentence, back to the preview, stale when it is no longer true", async (code, status, text, stale) => {
    const user = await start();
    vi.mocked(api.applyAddSongs).mockRejectedValueOnce(new api.ApiError(text, status, code));
    await user.click(dialogButton("Add songs"));
    expect(await screen.findByTestId("add-songs-problem")).toHaveTextContent(text);
    expect(screen.getByTestId("add-songs-problem")).toHaveAttribute("data-code", code);
    expect(screen.queryByTestId("add-songs-progress")).toBeNull();
    expect(screen.getByTestId("add-songs-preview")).toBeInTheDocument();
    expect(api.getAddSongsJob).not.toHaveBeenCalled();
    if (stale) {
      expect(dialogButton("Add songs")).toBeDisabled();
      expect(screen.getByTestId("add-songs-stale")).toBeInTheDocument();
    } else {
      expect(dialogButton("Add songs")).toBeEnabled();
    }
  });

  it("a request that never reached StagePilot is the offline sentence", async () => {
    const user = await start();
    vi.mocked(api.applyAddSongs).mockRejectedValueOnce(new TypeError("Failed to fetch"));
    await user.click(dialogButton("Add songs"));
    expect(await screen.findByTestId("add-songs-problem")).toHaveTextContent(/Can't reach StagePilot/);
  });
});

describe("keyboard and screen reader", () => {
  it("Escape closes the window when nothing is running, and the focus goes back to the button", async () => {
    view();
    const user = await open();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", OPEN)).toHaveFocus();
  });

  it("the first thing focused is inside the window, and Tab stays inside it", async () => {
    view();
    const user = await open();
    expect(screen.getByRole("dialog")).toContainElement(document.activeElement as HTMLElement);
    for (let i = 0; i < 30; i += 1) await user.tab();
    expect(screen.getByRole("dialog")).toContainElement(document.activeElement as HTMLElement);
  });

  it("everything can be done without a mouse", async () => {
    view();
    const user = await open();
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-SUNDAY");
    const box = screen.getByRole("checkbox", { name: /Holy Forever/ });
    box.focus();
    await user.keyboard(" ");
    expect(box).not.toBeChecked();
    dialogButton("Preview").focus();
    vi.mocked(api.previewAddSongs).mockResolvedValue(copyPlan());
    await user.keyboard("{Enter}");
    expect(await screen.findByTestId("add-songs-preview")).toBeInTheDocument();
  });

  it("every list and region has a name, and the song rows are real checkboxes inside labels", async () => {
    view();
    const user = await open();
    await choose(user);
    await preview(user);
    for (const name of ["Songs to add", "What will happen to each song"]) expect(screen.getByRole("list", { name })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Where the songs go" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Songs" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Preview" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /Holy Forever/ }).closest("label")).not.toBeNull();
  });

  it("the key and arrangement are text, so a screen reader reads each song's key", async () => {
    view();
    await open();
    expect(screen.getByRole("checkbox", { name: /Battle Belongs.*Key Ab.*Long ending/ })).toBeInTheDocument();
  });
});
