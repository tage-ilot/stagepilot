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
 * check mark, all selected at first (no header row, no Select all / Select none), and small chips for what a card must say.
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

  it("shows the selection by the card's look, not by a mark in the corner and not by colour alone: solid border, tinted fill and outline against a dashed, dimmed card", async () => {
    draw();
    const user = await ready();
    const picked = card("Battle Belongs");
    // No check mark is drawn (it used to shorten the colored bar).
    expect(picked.querySelector("[data-part=check]")).toBeNull();
    expect(picked).toHaveClass("border-integration-presentation-soft", "bg-integration-presentation-soft/20", "outline", "outline-2", "outline-ink");
    expect(picked).not.toHaveClass("border-dashed");
    await user.click(screen.getByRole("checkbox", { name: "Battle Belongs" }));
    expect(screen.getByRole("checkbox", { name: "Battle Belongs" })).toHaveAttribute("aria-checked", "false");
    const unpicked = card("Battle Belongs");
    expect(unpicked).toHaveClass("border-dashed", "bg-surface-900/40");
    expect(unpicked).not.toHaveClass("outline");
    expect(unpicked).not.toHaveClass("bg-integration-presentation-soft/20");
    expect(unpicked.querySelector("[data-part=check]")).toBeNull();
  });

  it("the colored bar runs the whole width of the card, as in Lighting: nothing is reserved at the right for a mark", async () => {
    draw();
    await ready();
    const button = screen.getByRole("checkbox", { name: "Holy Forever" });
    expect(button.className).not.toMatch(/\bpr-\d/);
    expect(button.className).toMatch(/\bpx-2\.5\b/);
    expect(within(button).getByTestId("library-bar").parentElement).toBe(button);
    // Lighting's press-mode card has the same padding, so both bars end at the same edges.
    const strip = render(<SongsStrip chips={[{ key: "a", title: "A", order: 0, looks: 1, handmade: 0, hasLoudness: false }]} liveKey="" noun="looks" onClear={vi.fn()} onSelect={vi.fn()} selectedKey="a" />);
    expect(strip.container.querySelector("button[aria-pressed]")!.className).toMatch(/\bpx-2\.5\b/);
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
    expect(count).toHaveClass("sr-only");
    expect(count).toHaveAttribute("role", "status");
    expect(count).toHaveAttribute("aria-live", "polite");
    await user.keyboard("{Enter}");
    expect(box).toBeChecked();
    expect(count).toHaveTextContent("4 of 4 selected");
  });

  it("has no header row: no Songs heading on screen, no count, no Select all or Select none; the list still has its name and the count is read out", async () => {
    draw();
    await ready();
    const section = screen.getByTestId("add-songs-songs");
    expect(screen.queryByRole("button", { name: "Select all" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Select none" })).toBeNull();
    expect(screen.queryByRole("group", { name: "Select songs" })).toBeNull();
    // The first thing a person sees in the section is the grid of cards; the heading and the count are visually hidden.
    const heading = within(section).getByRole("heading", { name: "Songs" });
    expect(heading).toHaveClass("sr-only");
    expect(section).toHaveAccessibleName("Songs");
    const count = screen.getByTestId("add-songs-count");
    expect(count).toHaveClass("sr-only");
    expect(count).toHaveTextContent("4 of 4 selected");
    expect([...section.children].filter((node) => !node.classList.contains("sr-only")).map((node) => node.tagName)).toEqual(["UL"]);
    expect(screen.getByRole("list", { name: "Songs to import" })).toBeInTheDocument();
  });

  it("each Playback card has the cover (when Playback has one), then the key and `mtID-number` as one square chip each, the arrangement, and its bar", async () => {
    draw();
    await ready();
    const holy = card("Holy Forever");
    expect(holy.querySelector("img")).not.toBeNull();
    expect(card("Battle Belongs").querySelector("img")).toBeNull();
    expect(within(holy).getByTestId("library-bar")).toHaveAttribute("role", "img");
    // Under the title: the key letters, then mtID and the number, each in its own square chip with no dot between; no \"Key\" word on screen.
    const facts = holy.querySelector("[id$='-facts']") as HTMLElement;
    expect(facts).toHaveTextContent(/^G \(was F\) mtID-43302$/);
    expect(facts.textContent).not.toMatch(/[\u00B7\u2022]/);
    expect(within(facts).getAllByRole("listitem").map((chip) => chip.textContent)).toEqual(["G (was F)", "mtID-43302"]);
    expect(within(holy).getByTestId("add-songs-key")).toHaveTextContent(/^G \(was F\)$/);
    expect(within(holy).queryByText(/\bKey\b/)).toBeNull();
    expect(holy.textContent).not.toMatch(/MultiTracks|Default|Sections/);
    expect(within(card("Gratitude")).getByTestId("add-songs-key")).toHaveTextContent(/^A$/);
    // The key is the same square chip as every other song fact.
    expect(within(holy).getByTestId("add-songs-key").className).toMatch(/\bborder\b.*\bbg-surface-950\b/);
    // The full order on hover (the card's title).
    expect(holy).toHaveAttribute("title", "Holy Forever: Count Off, Verse 1, Vamp on 1");
  });

  it("lays the card out top to bottom: the title, the fact chips, the colored section bar, THEN the arrangement text", async () => {
    draw();
    await ready();
    const holy = card("Holy Forever");
    const title = holy.querySelector("[title='Holy Forever']") as HTMLElement;
    const facts = holy.querySelector("[id$='-facts']") as HTMLElement;
    const bar = within(holy).getByTestId("library-bar");
    const order = within(holy).getByTestId("add-songs-order");
    const follows = (a: Element, b: Element) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(follows(title, facts)).toBe(true);
    expect(follows(facts, bar)).toBe(true);
    expect(follows(bar, order)).toBe(true);
    // The text keeps its words and its hover text.
    expect(order).toHaveTextContent("Count Off · Verse 1 · Vamp on 1");
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

  it("a song with no number shows `mtID\u2014`", async () => {
    draw();
    await ready();
    expect(card("Firm Foundation").querySelector("[id$='-facts']")).toHaveTextContent(/^C mtID\u2014$/);
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
    expect(box).toHaveAccessibleDescription(/G \(was F\) mtID-43302/);
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
