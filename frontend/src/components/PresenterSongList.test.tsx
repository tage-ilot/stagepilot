import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { libraryController, libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import { addPlan, addSong, guess, helperStatus, jobOf, linkController, playlists, report, result } from "../test/propresenterLinkFixtures";
import { PresenterLinkSection } from "./PresenterLinkSection";
import { SONG_GRID, SONG_GRID_CHECK } from "./SongCard";
import { SongsStrip } from "./SongsStrip";

/**
 * The Presenter Link's songs are the Lighting tab's song cards in check mode: the same grid, border, outline and 44 px target, a visible
 * check mark, all selected at first, Select all / Select none above them, and small chips for what a card must say.
 */
vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getProPresenterPlaylists: vi.fn(),
  lookupAddSongsTitles: vi.fn(),
  previewAddSongs: vi.fn(),
  applyAddSongs: vi.fn(),
  getAddSongsJob: vi.fn(),
}));

const NOW = Date.parse("2026-11-19T08:00:00Z");
const songs = () => libraryPlan([
  librarySongOf("Holy Forever", 1, { library: librarySong({ title: "Holy Forever", position: 1, setlist_song_id: 2000001, content_id: 5000001, key: "G", key_was: "F", key_changed_at: "2026-11-19T07:00:00Z" }) }),
  librarySongOf("Battle Belongs", 2, { library: librarySong({ title: "Battle Belongs", position: 2, setlist_song_id: 2000002, content_id: 5000002, key: "Ab", artwork: null }) }),
  librarySongOf("Gratitude", 3, { library: librarySong({ title: "Gratitude", position: 3, setlist_song_id: 2000003, content_id: 5000003, key: "A" }) }),
  librarySongOf("Firm Foundation", 4, { library: librarySong({ title: "Firm Foundation", position: 4, setlist_song_id: 2000004, content_id: 5000004, key: "C" }) }),
]);
const draw = (withPlayback = true) => render(<PresenterLinkSection library={withPlayback ? libraryController("ready", { plan: songs() }) : undefined} link={linkController(helperStatus("ready"))} plan={{ songs: ["Holy Forever", "Battle Belongs"].map((title, order) => ({ id: `s${order}`, title, duration_seconds: 300, order, is_generic: false, source_song_id: null })) }} />);
const card = (name: string) => screen.getByRole("checkbox", { name }).closest("li") as HTMLElement;
const ready = async () => { await screen.findByRole("option", { name: "Sunday Morning Service" }); return userEvent.setup({ advanceTimers: vi.advanceTimersByTime }); };

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true, now: NOW });
  vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
  vi.mocked(api.lookupAddSongsTitles).mockImplementation(async (titles) => ({ titles: titles.map((title) => ({ title, state: "found" as const })) }));
  vi.mocked(api.applyAddSongs).mockResolvedValue({ job_id: "fedcba9876543210fedcba9876543210", plan_id: "p", state: "running" });
  vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf());
});
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); window.localStorage.clear(); });

describe("the song cards", () => {
  it("are the Lighting cards: the same grid, border and 44 px target, one checkbox per card, all selected at first, in Playback's order", async () => {
    draw();
    await ready();
    const list = screen.getByRole("list", { name: "Songs to import" });
    // The Lighting grid (same 240 px auto-fill columns), with room between the outlines because every card can be selected at once.
    expect(list.className).toBe(SONG_GRID_CHECK);
    expect(SONG_GRID_CHECK.replace(/gap-\S+ p-\S+/, "")).toBe(SONG_GRID.replace(/gap-\S+ p-\S+/, ""));
    const boxes = within(list).getAllByRole("checkbox");
    expect(boxes.map((box) => box.getAttribute("aria-label"))).toEqual(["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"]);
    for (const box of boxes) {
      expect(box.tagName).toBe("BUTTON");
      expect(box).toHaveAttribute("aria-checked", "true");
      expect(box).toHaveClass("min-h-[44px]", "touch-manipulation");
      expect(box.closest("li")).toHaveClass("rounded-ds-lg", "border", "outline", "outline-2", "outline-ink");
    }
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("4 of 4 selected");
  });

  it("shows the selection by the outline AND a visible check mark, never colour alone", async () => {
    draw();
    const user = await ready();
    const mark = () => card("Battle Belongs").querySelector("[data-part=check]") as HTMLElement;
    expect(mark()).toHaveTextContent("\u2713");
    expect(mark()).not.toHaveClass("text-transparent");
    await user.click(screen.getByRole("checkbox", { name: "Battle Belongs" }));
    expect(screen.getByRole("checkbox", { name: "Battle Belongs" })).toHaveAttribute("aria-checked", "false");
    expect(card("Battle Belongs")).not.toHaveClass("outline");
    expect(mark()).toHaveClass("text-transparent", "border-ink-muted");
    expect(mark()).toHaveAttribute("aria-hidden", "true");
  });

  it("Space and Enter toggle a card; the count is a polite live region", async () => {
    draw();
    const user = await ready();
    const box = screen.getByRole("checkbox", { name: "Gratitude" });
    box.focus();
    await user.keyboard(" ");
    expect(box).not.toBeChecked();
    const count = screen.getByTestId("add-songs-count");
    expect(count).toHaveTextContent("3 of 4 selected");
    expect(count).toHaveAttribute("role", "status");
    expect(count).toHaveAttribute("aria-live", "polite");
    await user.keyboard("{Enter}");
    expect(box).toBeChecked();
    expect(count).toHaveTextContent("4 of 4 selected");
  });

  it("Select all and Select none are small section buttons above the grid", async () => {
    draw();
    const user = await ready();
    const group = screen.getByRole("group", { name: "Select songs" });
    expect(group.closest("[data-section=actions]")).not.toBeNull();
    expect(group.compareDocumentPosition(screen.getByRole("list", { name: "Songs to import" })) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await user.click(within(group).getByRole("button", { name: "Select none" }));
    expect(screen.getAllByRole("checkbox").every((box) => box.getAttribute("aria-checked") === "false")).toBe(true);
    expect(screen.getByTestId("add-songs-count")).toHaveTextContent("0 of 4 selected");
    await user.click(within(group).getByRole("button", { name: "Select all" }));
    expect(screen.getAllByRole("checkbox").every((box) => box.getAttribute("aria-checked") === "true")).toBe(true);
  });

  it("each Playback card has the cover (when Playback has one), MultiTracks id, the key chip with (was X), the arrangement and its bar", async () => {
    draw();
    await ready();
    const holy = card("Holy Forever");
    expect(holy.querySelector("img")).not.toBeNull();
    expect(card("Battle Belongs").querySelector("img")).toBeNull();
    expect(within(holy).getByTestId("add-songs-multitracks")).toHaveTextContent("5000001");
    expect(within(holy).getByTestId("add-songs-key")).toHaveTextContent("Key G (was F)");
    expect(within(card("Gratitude")).getByTestId("add-songs-key")).toHaveTextContent("Key A");
    expect(within(holy).getByTestId("add-songs-arrangement")).toHaveTextContent("Default");
    expect(within(holy).getByTestId("library-bar")).toHaveAttribute("role", "img");
    // The whole order on hover (the card's title) and for a screen reader (the description); Sections opens it on the card.
    expect(holy).toHaveAttribute("title", "Holy Forever: Count Off, Verse 1, Vamp on 1");
    expect(screen.getByRole("checkbox", { name: "Holy Forever" })).toHaveAccessibleDescription(/sections: Count Off, Verse 1, Vamp on 1/);
    expect(screen.getByRole("checkbox", { name: "Holy Forever" })).toHaveAccessibleDescription(/Key G \(was F\)/);
  });

  it("a Planning Center card is just the title and a Planning Center chip", async () => {
    draw(false);
    await ready();
    const holy = card("Holy Forever");
    expect(within(holy).getByTestId("add-songs-source-chip")).toHaveTextContent("Planning Center");
    expect(holy.querySelector("img")).toBeNull();
    expect(within(holy).queryByTestId("library-bar")).toBeNull();
    expect(within(holy).queryByRole("button", { name: /Sections/ })).toBeNull();
    expect(screen.getByRole("checkbox", { name: "Holy Forever" })).toHaveAccessibleDescription("Planning Center");
  });

  it("after a preview that stopped, the cards it stopped for carry its stamp, and a guessed song a Guessed chip", async () => {
    vi.mocked(api.previewAddSongs).mockResolvedValue(addPlan([
      addSong({ ref_id: "2000001", title: "Holy Forever", mapping: [{ section: "Interlude", group: "Interlude", how: "guessed", line: "x" }] }),
      addSong({ ref_id: "2000002", title: "Battle Belongs", action: "blocked", reason: "Not in ProPresenter." }),
      addSong({ ref_id: "2000003", title: "Gratitude", action: "needs_choice", reason: "Two files." }),
      addSong({ ref_id: "2000004", title: "Firm Foundation" }),
    ]));
    draw();
    const user = await ready();
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-SUNDAY");
    await user.click(screen.getByRole("button", { name: "Import songs to ProPresenter" }));
    await screen.findByTestId("add-songs-preview");
    expect(within(card("Battle Belongs")).getByTestId("add-songs-stuck")).toHaveTextContent("Blocked");
    expect(within(card("Gratitude")).getByTestId("add-songs-stuck")).toHaveTextContent("Needs a choice");
    expect(within(card("Holy Forever")).getByTestId("add-songs-guessed")).toHaveTextContent("Guessed");
    expect(within(card("Firm Foundation")).queryByTestId("add-songs-stuck")).toBeNull();
    // A change makes the preview old: its stamps go with it.
    await user.click(screen.getByRole("checkbox", { name: "Firm Foundation" }));
    expect(screen.queryByTestId("add-songs-stuck")).toBeNull();
  });

  it("after an import, a song with guessed sections keeps a Guessed chip on its card", async () => {
    const how = { ...result("2000001", "Holy Forever", "added"), guesses: [guess()] };
    vi.mocked(api.previewAddSongs).mockResolvedValue(addPlan([addSong({ ref_id: "2000001", title: "Holy Forever" })]));
    vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf({ state: "done", report: report({ songs: [how] }) }));
    draw();
    const user = await ready();
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-SUNDAY");
    await user.click(screen.getByRole("button", { name: "Import songs to ProPresenter" }));
    await vi.advanceTimersByTimeAsync(5);
    await screen.findByTestId("add-songs-result");
    expect(within(card("Holy Forever")).getByTestId("add-songs-guessed")).toBeInTheDocument();
  });
});

describe("the Lighting strip keeps its look and behaviour", () => {
  it("is still one pressed-or-not button per card (no checkbox, no check mark), with the selected outline", () => {
    render(<SongsStrip chips={[{ key: "a", title: "A", order: 0, looks: 1, handmade: 0, hasLoudness: false }, { key: "b", title: "B", order: 1, looks: 0, handmade: 0, hasLoudness: false }]} liveKey="" noun="looks" onClear={vi.fn()} onSelect={vi.fn()} selectedKey="a" />);
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
    expect(document.querySelector("[data-part=check]")).toBeNull();
    const a = screen.getByRole("button", { name: /^A,/ });
    expect(a).toHaveAttribute("aria-pressed", "true");
    expect(a.closest("li")?.className).toBe("relative flex min-w-0 items-stretch rounded-ds-lg border text-sm border-cue-look-captured bg-cue-look-captured/20 text-ink outline outline-2 outline-offset-1 outline-ink");
    expect(screen.getByRole("button", { name: /^B,/ }).closest("li")?.className).toBe("relative flex min-w-0 items-stretch rounded-ds-lg border text-sm border-dashed border-edge-medium bg-surface-900/40 text-ink-muted ");
  });
});
