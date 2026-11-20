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
// The MultiTracks Song Master ID (`content_id`, 4-6 digits) and Playback's own setlist song number (`setlist_song_id`, a long number) differ on purpose.
const songs = () => libraryPlan([
  librarySongOf("Holy Forever", 1, { library: librarySong({ title: "Holy Forever", position: 1, setlist_song_id: 30389715, content_id: 43302, key: "G", key_was: "F", key_changed_at: "2026-11-19T07:00:00Z" }) }),
  librarySongOf("Battle Belongs", 2, { library: librarySong({ title: "Battle Belongs", position: 2, setlist_song_id: 30389716, content_id: 23126, key: "Ab", artwork: null }) }),
  librarySongOf("Gratitude", 3, { library: librarySong({ title: "Gratitude", position: 3, setlist_song_id: 30389717, content_id: 8412, key: "A" }) }),
  librarySongOf("Firm Foundation", 4, { library: librarySong({ title: "Firm Foundation", position: 4, setlist_song_id: 30389718, content_id: null, key: "C" }) }),
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

  it("each Playback card has the cover (when Playback has one), then `key · mtID number` as plain text, the arrangement, and its bar", async () => {
    draw();
    await ready();
    const holy = card("Holy Forever");
    expect(holy.querySelector("img")).not.toBeNull();
    expect(card("Battle Belongs").querySelector("img")).toBeNull();
    expect(within(holy).getByTestId("library-bar")).toHaveAttribute("role", "img");
    // One compact line under the title: the key letters, a middle dot, mtID and the number: no \"Key\" word, no chip box.
    const facts = holy.querySelector("[id$='-facts']") as HTMLElement;
    expect(facts).toHaveTextContent(/^G \(was F\) \u00B7 mtID 43302$/);
    expect(within(holy).getByTestId("add-songs-key")).toHaveTextContent(/^G \(was F\)$/);
    expect(within(holy).queryByText(/\bKey\b/)).toBeNull();
    expect(holy.textContent).not.toMatch(/MultiTracks|Default|Sections/);
    expect(within(card("Gratitude")).getByTestId("add-songs-key")).toHaveTextContent(/^A$/);
    // The key is the card's own text style, not a box.
    expect(within(holy).getByTestId("add-songs-key").className).not.toMatch(/border|bg-/);
    // The full order on hover (the card's title).
    expect(holy).toHaveAttribute("title", "Holy Forever: Count Off, Verse 1, Vamp on 1");
  });

  it("the card shows the MultiTracks number (content_id), never Playback's own song number", async () => {
    draw();
    await ready();
    // Holy Forever: Playback's setlist song number is 30389715, the Song Master ID is 43302.
    expect(within(card("Holy Forever")).getByTestId("add-songs-mtid")).toHaveTextContent(/^43302$/);
    expect(card("Holy Forever").textContent).not.toContain("30389715");
    expect(within(card("Battle Belongs")).getByTestId("add-songs-mtid")).toHaveTextContent(/^23126$/);
    expect(within(card("Gratitude")).getByTestId("add-songs-mtid")).toHaveTextContent(/^8412$/);
    for (const name of ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"]) expect(card(name).textContent).not.toMatch(/3038971\d/);
  });

  it("a song with no number shows `mtID \u2014`", async () => {
    draw();
    await ready();
    expect(card("Firm Foundation").querySelector("[id$='-facts']")).toHaveTextContent(/^C \u00B7 mtID \u2014$/);
    expect(within(card("Firm Foundation")).getByTestId("add-songs-mtid")).toHaveTextContent(/^\u2014$/);
  });

  it("the order Playback plays is on every card with no button, no heading and no click, and the bar stays", async () => {
    draw();
    await ready();
    const holy = card("Holy Forever");
    expect(within(holy).getByTestId("add-songs-order")).toHaveTextContent("Count Off \u00B7 Verse 1 \u00B7 Vamp on 1");
    expect(within(holy).queryByRole("button", { name: /Sections/ })).toBeNull();
    expect(screen.queryByText("Sections")).toBeNull();
    expect(screen.queryByRole("heading", { name: /Sections/ })).toBeNull();
    expect(holy.querySelector("[aria-expanded]")).toBeNull();
    expect(screen.queryByTestId("add-songs-arrangement")).toBeNull();
    // Written out for every card (the arrangement name, \"Default\", is not on the card), next to the bar.
    for (const name of ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"]) expect(within(card(name)).getByTestId("add-songs-order")).toBeVisible();
    expect(within(holy).getByTestId("library-bar")).toBeInTheDocument();
    // A screen reader hears it with the key and the number.
    const box = screen.getByRole("checkbox", { name: "Holy Forever" });
    expect(box).toHaveAccessibleDescription(/G \(was F\) \u00B7 mtID 43302/);
    expect(box).toHaveAccessibleDescription(/Count Off \u00B7 Verse 1 \u00B7 Vamp on 1/);
  });

  it("a long order wraps onto as many lines as it needs: it is never cut short", async () => {
    const long = ["Count Off", "Intro", "Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge", "Chorus", "Interlude", "Verse 3", "Chorus", "Outro"];
    const library = libraryPlan([librarySongOf("Long One", 1, { library: librarySong({ title: "Long One", position: 1, setlist_song_id: 30389720, content_id: 12000, sections: long.map((name, i) => ({ name, start: i * 10, end: i * 10 + 9 })) }) })]);
    render(<PresenterLinkSection library={libraryController("ready", { plan: library })} link={linkController(helperStatus("ready"))} plan={null} />);
    await ready();
    const text = within(card("Long One")).getByTestId("add-songs-order");
    expect(text).toHaveTextContent(long.join(" \u00B7 "));
    expect(text.className).toContain("break-words");
    expect(text.className).not.toMatch(/line-clamp|truncate/);
    expect(text.parentElement?.className).not.toMatch(/truncate/);
  });

  it("a Planning Center card is just the title and a Planning Center chip", async () => {
    draw(false);
    await ready();
    const holy = card("Holy Forever");
    expect(within(holy).getByTestId("add-songs-source-chip")).toHaveTextContent("Planning Center");
    expect(holy.querySelector("img")).toBeNull();
    expect(within(holy).queryByTestId("library-bar")).toBeNull();
    expect(within(holy).queryByRole("button", { name: /Sections/ })).toBeNull();
    expect(within(holy).queryByTestId("add-songs-mtid")).toBeNull();
    expect(screen.getByRole("checkbox", { name: "Holy Forever" })).toHaveAccessibleDescription("Planning Center");
  });

  it("after a preview that stopped, the cards it stopped for carry its chip, and a guessed song a Check chip", async () => {
    vi.mocked(api.previewAddSongs).mockResolvedValue(addPlan([
      addSong({ ref_id: "30389715", title: "Holy Forever", mapping: [{ section: "Interlude", group: "Interlude", how: "guessed", line: "x" }] }),
      addSong({ ref_id: "30389716", title: "Battle Belongs", action: "blocked", reason: "Not in ProPresenter." }),
      addSong({ ref_id: "30389717", title: "Gratitude", action: "needs_choice", reason: "Two files." }),
      addSong({ ref_id: "30389718", title: "Firm Foundation" }),
    ]));
    draw();
    const user = await ready();
    await user.selectOptions(screen.getByLabelText("Playlist"), "PL-SUNDAY");
    await user.click(screen.getByRole("button", { name: "Import songs to ProPresenter" }));
    await screen.findByTestId("add-songs-preview");
    expect(within(card("Battle Belongs")).getByTestId("add-songs-stuck")).toHaveTextContent("Can't import");
    expect(within(card("Gratitude")).getByTestId("add-songs-stuck")).toHaveTextContent("Pick one");
    expect(within(card("Holy Forever")).getByTestId("add-songs-guessed")).toHaveTextContent("Check");
    expect(within(card("Firm Foundation")).queryByTestId("add-songs-stuck")).toBeNull();
    // A change makes the preview old: its chips go with it.
    await user.click(screen.getByRole("checkbox", { name: "Firm Foundation" }));
    expect(screen.queryByTestId("add-songs-stuck")).toBeNull();
  });

  it("after an import, a song with guessed sections keeps a Check chip on its card", async () => {
    const how = { ...result("30389715", "Holy Forever", "added"), guesses: [guess()] };
    vi.mocked(api.previewAddSongs).mockResolvedValue(addPlan([addSong({ ref_id: "30389715", title: "Holy Forever" })]));
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
