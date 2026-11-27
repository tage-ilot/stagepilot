import { render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { NO_CUES_FIX } from "../presenterFollow";
import { libraryController, libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import { followController, followSongs, followStatus, NO_CUES_SONG } from "../test/presenterFollowFixtures";
import { helperStatus, linkController, playlists } from "../test/propresenterLinkFixtures";
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
  <PresenterLinkSection follow={follow} library={over.library ?? libraryController("ready", { plan: plan() })} link={over.link ?? linkController(helperStatus("ready"))} />,
);
const card = (title: string) => screen.getByRole("checkbox", { name: title }).closest("li") as HTMLElement;

beforeEach(() => {
  window.localStorage.setItem(PRESENTER_LINK_OPEN_KEY, "true");
  vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
  vi.mocked(api.lookupAddSongsTitles).mockImplementation(async (titles) => ({ titles: titles.map((title) => ({ title, state: "found" as const })) }));
});
afterEach(() => { vi.clearAllMocks(); window.localStorage.clear(); });

describe("the readiness chip on each song", () => {
  it("is not drawn while the window has no follow status, and the box is not drawn at all without a controller", async () => {
    draw(undefined);
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    expect(screen.queryByTestId("follow-song")).toBeNull();
    expect(screen.queryByRole("switch", { name: "Follow Playback in ProPresenter" })).toBeNull();
  });

  it("says in the backend's words whether each song is followed, by cues, by bars, or why not", async () => {
    draw(followController({
      status: followStatus({
        songs: followSongs(
          {},
          { code: "cues_do_not_match", text: "Cues do not match the file (cues go to slide 30, file has 54 cues)", following: "bars" },
          { code: "arrangement_changed", text: "Arrangement was changed in ProPresenter", following: null },
          { code: "not_in_playlist", text: "Not in the ProPresenter playlist", following: null },
        ),
      }),
    }));
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    expect(within(card("Holy Forever")).getByTestId("follow-song-chip")).toHaveTextContent("Following by Playback cues");
    expect(within(card("Battle Belongs")).getByTestId("follow-song-chip")).toHaveTextContent("Cues do not match the file (cues go to slide 30, file has 54 cues)");
    expect(within(card("Battle Belongs")).getByTestId("follow-song-phrase")).toHaveTextContent("Followed by bars");
    expect(within(card("Gratitude")).getByTestId("follow-song-chip")).toHaveTextContent("Arrangement was changed in ProPresenter");
    expect(within(card("Firm Foundation")).getByTestId("follow-song-chip")).toHaveTextContent("Not in the ProPresenter playlist");
    // The state is the mark and the words, not the colour: every chip carries a mark.
    for (const chip of screen.getAllByTestId("follow-song-chip")) expect(chip.querySelector("[aria-hidden=true]")?.textContent).toMatch(/\S/);
  });

  it("says No cues, with the one sentence that tells where to make them in Playback", async () => {
    draw(followController({ status: followStatus({ songs: followSongs({}, NO_CUES_SONG) }) }));
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    expect(within(card("Battle Belongs")).getByTestId("follow-song-chip")).toHaveTextContent("Following by bars (no Playback cues)");
    expect(within(card("Battle Belongs")).getByTestId("follow-song-no-cues")).toHaveTextContent("No cues");
    const fix = within(card("Battle Belongs")).getByTestId("follow-song-fix");
    expect(fix).toHaveTextContent(NO_CUES_FIX);
    for (const word of ["Edit", "MIDI OUT", "Lyrics Bus", "ProPresenter 1 Line Template", "Update"]) expect(fix.textContent).toContain(word);
    expect(within(card("Holy Forever")).queryByTestId("follow-song-no-cues")).toBeNull();
  });

  it("is on the plan's own names too, until Playback has given the songs", async () => {
    const names = { songs: ["Holy Forever", "Battle Belongs"].map((title, order) => ({ id: `s${order}`, title, duration_seconds: 300, order, is_generic: false, source_song_id: null })) };
    const follow = followController({ status: followStatus({ songs: followSongs({ code: "switch_off", text: "Switch is off", following: null }).map((song, i) => ({ ...song, key: `s${i}` })) }) });
    render(<PresenterLinkSection follow={follow} library={undefined} link={linkController(helperStatus("ready"))} plan={names} />);
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    expect(within(card("Holy Forever")).getByTestId("follow-song-chip")).toHaveTextContent("Switch is off");
    expect(within(card("Battle Belongs")).getByTestId("follow-song-chip")).toHaveTextContent("Switch is off");
  });
});

describe("Follow Playback in the Presenter Link area", () => {
  it("sits under the section, locked with the reason while the Presenter Link is not ready (the section stays drawn)", () => {
    draw(followController({ enabled: false }), { link: linkController(helperStatus("not_installed")) });
    expect(screen.getByTestId("presenter-link-section")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Follow Playback in ProPresenter" })).toBeDisabled();
    expect(screen.getByTestId("presenter-follow-reason")).toHaveTextContent("Presenter Link not ready");
  });

  it("is locked with its own reason while Playback's Library Link is not ready", () => {
    draw(followController({ enabled: false }), { library: libraryController("unreachable") });
    expect(screen.getByRole("switch", { name: "Follow Playback in ProPresenter" })).toBeDisabled();
    expect(screen.getByTestId("presenter-follow-reason")).toHaveTextContent("Playback's Library Link not ready");
  });

  it("is usable when both links are ready", () => {
    draw(followController({ enabled: false }));
    expect(screen.getByRole("switch", { name: "Follow Playback in ProPresenter" })).toBeEnabled();
    expect(screen.queryByTestId("presenter-follow-reason")).toBeNull();
  });

  it("puts what it is doing in the folded header while following is on", () => {
    window.localStorage.setItem(PRESENTER_LINK_OPEN_KEY, "false");
    draw(followController({ status: followStatus({ state: "following", section: { index: 1, name: "Chorus" }, slide: 2 }) }));
    expect(screen.getByTestId("presenter-link-summary")).toHaveTextContent("Following: Chorus, slide 3");
  });

  it("leaves the header as it was while the switch is off", () => {
    window.localStorage.setItem(PRESENTER_LINK_OPEN_KEY, "false");
    draw(followController({ enabled: false, status: followStatus({ enabled: false, state: "off" }) }));
    expect(screen.getByTestId("presenter-link-summary").textContent).not.toMatch(/Following/);
  });
});
