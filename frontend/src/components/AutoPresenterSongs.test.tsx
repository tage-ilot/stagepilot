import { render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { AUTO_PRESENTER_OPEN_KEY, templateFix } from "../presenterFollow";
import { libraryController, libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import { followController, followSongs, followStatus, NO_CUES_SONG } from "../test/presenterFollowFixtures";
import { helperStatus, linkController, playlists } from "../test/propresenterLinkFixtures";
import { AutoPresenterSection } from "./AutoPresenterSection";
import { PresenterLinkSection } from "./PresenterLinkSection";
import { PRESENTER_LINK_OPEN_KEY } from "./proPresenterLinkState";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getProPresenterPlaylists: vi.fn(),
  lookupAddSongsTitles: vi.fn(),
}));

const TITLES = ["Holy Forever", "Battle Belongs", "Gratitude", "Firm Foundation"];
const plan = () => libraryPlan(TITLES.map((title, i) => librarySongOf(title, i + 1, { library: librarySong({ title, position: i + 1, setlist_song_id: 2000001 + i, content_id: 20000 + i }) })));
const draw = (follow?: ReturnType<typeof followController>, over: { link?: ReturnType<typeof linkController>; library?: ReturnType<typeof libraryController> } = {}) => render(
  <AutoPresenterSection follow={follow} library={over.library ?? libraryController("ready", { plan: plan() })} link={over.link ?? linkController(helperStatus("ready"))} />,
);
const row = (title: string) => screen.getAllByTestId("auto-presenter-song").find((item) => within(item).queryByTitle(title)) as HTMLElement;
const names = (...titles: string[]) => ({ songs: titles.map((title, order) => ({ id: `s${order}`, title, duration_seconds: 300, order, is_generic: false, source_song_id: null })) });
const chip = (title: string) => within(row(title)).getByTestId("follow-song-chip");

beforeEach(() => {
  window.localStorage.setItem(AUTO_PRESENTER_OPEN_KEY, "true");
  vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
  vi.mocked(api.lookupAddSongsTitles).mockImplementation(async (titles) => ({ titles: titles.map((title) => ({ title, state: "found" as const })) }));
});
afterEach(() => { vi.clearAllMocks(); window.localStorage.clear(); });

describe("the song list of the Auto-Presenter", () => {
  it("is one row per plan song, in Playback's order, each with a readiness chip; no chips without a status yet", () => {
    draw(followController({ status: null }));
    expect(screen.getAllByTestId("auto-presenter-song").map((item) => within(item).getByTitle(/./).textContent)).toEqual(TITLES);
    for (const title of TITLES) expect(chip(title)).toHaveTextContent("Checking");
  });

  it("says in the backend's words whether each song is followed, by cues, by bars, or why not, each with a mark", () => {
    draw(followController({
      status: followStatus({
        songs: followSongs(
          {},
          { code: "cues_do_not_match", text: "Cues do not match the file (cues go to slide 30, file has 54 cues)", following: "bars", advice: "In Playback choose the ProPresenter 2 Line Template for this song", template: "ProPresenter 2 Line Template" },
          { code: "arrangement_changed", text: "Arrangement was changed in ProPresenter", following: null },
          { code: "not_in_playlist", text: "Not in the ProPresenter playlist", following: null },
        ),
      }),
    }));
    expect(chip("Holy Forever")).toHaveTextContent("Following by Playback cues");
    expect(chip("Battle Belongs")).toHaveTextContent("Cues do not match the file (cues go to slide 30, file has 54 cues)");
    expect(within(row("Battle Belongs")).getByTestId("follow-song-phrase")).toHaveTextContent("Followed by bars");
    expect(within(row("Battle Belongs")).getByTestId("follow-song-fix")).toHaveTextContent("Lyrics Bus, ProPresenter 2 Line Template, Update.");
    expect(chip("Gratitude")).toHaveTextContent("Arrangement was changed in ProPresenter");
    expect(chip("Firm Foundation")).toHaveTextContent("Not in the ProPresenter playlist");
    for (const item of screen.getAllByTestId("follow-song-chip")) expect(item.querySelector("[aria-hidden=true]")?.textContent).toMatch(/\S/);
  });

  it("says ONE chip, No cues: following by bars, with the one sentence that names the template that fits the file", () => {
    const twoLine = { ...NO_CUES_SONG, lines_per_slide: 2, template: "ProPresenter 2 Line Template", advice: "No cues: in Playback choose the ProPresenter 2 Line Template for this song" };
    draw(followController({ status: followStatus({ songs: followSongs({}, twoLine) }) }));
    expect(within(row("Battle Belongs")).getAllByTestId("follow-song-chip")).toHaveLength(1);
    expect(chip("Battle Belongs")).toHaveTextContent("No cues: following by bars");
    expect(chip("Battle Belongs")).toHaveAttribute("data-tone", "info");
    expect(chip("Battle Belongs").getAttribute("title")).toContain("Following by bars (no Playback cues)");
    const fix = within(row("Battle Belongs")).getByTestId("follow-song-fix");
    expect(fix).toHaveTextContent(templateFix(twoLine));
    for (const word of ["Edit", "MIDI OUT", "Lyrics Bus", "ProPresenter 2 Line Template", "Update"]) expect(fix.textContent).toContain(word);
    expect(within(row("Holy Forever")).queryByTestId("follow-song-no-cues")).toBeNull();
    expect(chip("Holy Forever")).toHaveTextContent("Following by Playback cues");
  });

  it("marks a song followed with extra slides in the file as one to check", () => {
    draw(followController({ status: followStatus({ songs: followSongs({ code: "cues_aligned", text: "Following by Playback cues, aligned with extra slides (check the first slide of Intro)" }) }) }));
    expect(chip("Holy Forever")).toHaveTextContent("aligned with extra slides (check the first slide of Intro)");
    expect(chip("Holy Forever").querySelector("[aria-hidden=true]")).toHaveTextContent("!");
  });

  it("marks a song followed by lines as one to check, in words with both numbers, and says which template is still best", () => {
    draw(followController({ status: followStatus({ songs: followSongs({ code: "cues_by_lines", text: "Aligned by lines (Playback 2 per slide, the file 1 per slide)", advice: "Matching the template is still best: in Playback choose a ProPresenter 1 Line Template for this song", lines_per_slide: 1, playback_lines: 2, template: "ProPresenter 1 Line Template" }) }) }));
    expect(chip("Holy Forever")).toHaveTextContent("Aligned by lines (Playback 2 per slide, the file 1 per slide)");
    expect(chip("Holy Forever").querySelector("[aria-hidden=true]")).toHaveTextContent("!");
    expect(chip("Holy Forever")).toHaveAttribute("data-tone", "warning");
    expect(chip("Holy Forever").getAttribute("title")).toContain("Matching the template is still best");
    expect(within(row("Holy Forever")).getByTestId("follow-song-fix")).toHaveTextContent("Lyrics Bus, ProPresenter 1 Line Template, Update.");
    // It is followed (by cues), so nothing says bars.
    expect(within(row("Holy Forever")).queryByTestId("follow-song-phrase")).toBeNull();
  });

  it("is on the plan's own names too, until Playback has given the songs", () => {
    const follow = followController({ status: followStatus({ songs: followSongs({ code: "switch_off", text: "Switch is off", following: null }).map((song, i) => ({ ...song, key: `s${i}` })) }) });
    render(<AutoPresenterSection follow={follow} library={libraryController("ready", { plan: null })} link={linkController(helperStatus("ready"))} plan={names("Holy Forever", "Battle Belongs")} />);
    expect(chip("Holy Forever")).toHaveTextContent("Switch is off");
    expect(chip("Battle Belongs")).toHaveTextContent("Switch is off");
  });

  it("says in one sentence which plan songs have no Playback song behind them, and nothing when all are paired or while locked", () => {
    const some = libraryController("ready", { plan: libraryPlan([librarySongOf("Holy Forever", 1), librarySongOf("Oh My Soul", 2, { library: null, confidence: null, status: "missing" }), librarySongOf("Give Thanks", 3, { library: null, confidence: null, status: "missing" })]) });
    const view = draw(followController({ status: null }), { library: some });
    expect(screen.getAllByTestId("auto-presenter-song")).toHaveLength(1);
    expect(screen.getByTestId("auto-presenter-unpaired")).toHaveTextContent("Not followed: Oh My Soul, Give Thanks. They are not in Playback's setlist, or not paired with a Playback song yet. Pair a song by hand in the Song order section");
    view.unmount();
    draw(followController({ status: null }));
    expect(screen.queryByTestId("auto-presenter-unpaired")).toBeNull();
  });

  it("while locked the rows are only the cover and title, greyed: no chip, no fix, the reason is said once above", () => {
    render(<AutoPresenterSection follow={followController({ status: followStatus({ songs: followSongs().map((song, i) => ({ ...song, key: `s${i}` })) }) })} library={libraryController("unreachable")} link={linkController(helperStatus("ready"))} plan={names(...TITLES)} />);
    expect(screen.getAllByTestId("auto-presenter-song")).toHaveLength(4);
    expect(screen.queryByTestId("follow-song-chip")).toBeNull();
    expect(screen.queryByTestId("follow-song-fix")).toBeNull();
    expect(screen.queryByTestId("follow-song-phrase")).toBeNull();
    expect(document.body.textContent).not.toContain("Library Link not ready");
    for (const item of screen.getAllByTestId("auto-presenter-song")) expect(item).toHaveClass("opacity-60");
    expect(screen.getAllByTestId("presenter-follow-reason")).toHaveLength(1);
  });
});

describe("the Presenter Link's song cards", () => {
  it("carry no Auto-Presenter chips", async () => {
    window.localStorage.setItem(PRESENTER_LINK_OPEN_KEY, "true");
    render(<PresenterLinkSection library={libraryController("ready", { plan: plan() })} link={linkController(helperStatus("ready"))} />);
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    expect(screen.getAllByRole("checkbox").length).toBe(4);
    expect(screen.queryByTestId("follow-song")).toBeNull();
    expect(screen.queryByTestId("follow-song-chip")).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
  });
});
