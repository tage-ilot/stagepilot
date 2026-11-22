import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { SONG_FILE_KEY_PREFIX } from "../hooks/songFilePicks";
import type { AddSongsPlan, AddSongsPreviewRequest, AddSongsSong } from "../propresenterLink";
import { libraryController, libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import { addPlan, addSong, helperStatus, jobOf, linkController, playlists } from "../test/propresenterLinkFixtures";
import { PresenterLinkSection } from "./PresenterLinkSection";
import { PRESENTER_LINK_OPEN_KEY } from "./proPresenterLinkState";

/**
 * "Pick one": the person chooses the song's ProPresenter file on the card. The buttons are a radio group under the title; pressing one
 * asks for a new preview with `sources`; the pick is remembered in this browser by the song's MultiTracks number; the name rule can
 * decide without asking and says so; Change brings the buttons back.
 */
vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getProPresenterPlaylists: vi.fn(),
  lookupAddSongsTitles: vi.fn(),
  previewAddSongs: vi.fn(),
  applyAddSongs: vi.fn(),
  getAddSongsJob: vi.fn(),
}));

const LIVE = "Songs / Firm Foundation (Live).pro";
const PLAIN = "Songs / Firm Foundation.pro";
const FIRM_REF = "2000002";
const FIRM_MTID = 8412;
const MEMORY = `${SONG_FILE_KEY_PREFIX}${FIRM_MTID}`;

const songs = (firmId: number | null = FIRM_MTID) => libraryPlan([
  librarySongOf("Holy Forever", 1, { library: librarySong({ title: "Holy Forever", position: 1, setlist_song_id: 2000001, content_id: 23126, key: "G" }) }),
  librarySongOf("Firm Foundation", 2, { library: librarySong({ title: "Firm Foundation", position: 2, setlist_song_id: 2000002, content_id: firmId, key: "E" }) }),
]);
const draw = (plan = songs()) => render(<PresenterLinkSection library={libraryController("ready", { plan })} link={linkController(helperStatus("ready"))} />);

const holy = () => addSong({ ref_id: "2000001", title: "Holy Forever", content_id: 23126, file: "Holy Forever (1)" });
const firm = (over: Partial<AddSongsSong> = {}) => addSong({ ref_id: FIRM_REF, title: "Firm Foundation", content_id: FIRM_MTID, ...over });
/** Two files are this song and the name rule does not decide. */
const asking = () => addPlan([holy(), firm({ action: "needs_choice", file: null, source: null, reason: "Two files are this song. Pick one.", choices: [LIVE, PLAIN] })]);
/** The plan with the song settled on `file`. */
const using = (file: string, by: "pick" | "title", over: Partial<AddSongsSong> = {}) => addPlan([holy(), firm({
  action: "copy", file: `${file.slice(8, -4)} (1)`, source: file, choices: [LIVE, PLAIN], chosen_by: by, chosen_file: file,
  notes: [`Used "${file.slice(8, -4)}": ${by === "pick" ? "the file you picked." : "its name matches the Playback title."}`], ...over,
})]);
/** What the route answers: the pick in `sources` when it is one of the files, else it asks. */
const route = (request: AddSongsPreviewRequest): AddSongsPlan => {
  const file = request.sources?.[FIRM_REF];
  return file === LIVE || file === PLAIN ? using(file, "pick") : asking();
};

const ADD = { name: "Import songs to ProPresenter" };
const previewRequests = () => vi.mocked(api.previewAddSongs).mock.calls.map(([request]) => request);
const card = (name: string) => screen.getByRole("checkbox", { name }).closest("li") as HTMLElement;
const group = () => screen.getByRole("radiogroup", { name: "Firm Foundation: use file" });

/** The section with the options chosen and the one button pressed: the preview stops at the song that needs a pick. */
async function previewed(plan = songs(), waitForPreview = true) {
  const user = userEvent.setup();
  draw(plan);
  await screen.findByRole("option", { name: "Sunday Morning Service" });
  await user.selectOptions(screen.getByLabelText("Playlist"), "PL-SUNDAY");
  await user.click(screen.getByRole("button", ADD));
  if (waitForPreview) await waitFor(() => expect(screen.getByTestId("add-songs-preview")).toBeInTheDocument());
  return user;
}

beforeEach(() => {
  window.localStorage.clear();
  // The person has opened the section (it starts folded): these tests are about what is inside it.
  window.localStorage.setItem(PRESENTER_LINK_OPEN_KEY, "true");
  vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
  vi.mocked(api.lookupAddSongsTitles).mockImplementation(async (titles) => ({ titles: titles.map((title) => ({ title, state: "found" as const })) }));
  vi.mocked(api.previewAddSongs).mockImplementation(async (request) => route(request));
  vi.mocked(api.applyAddSongs).mockResolvedValue({ job_id: "fedcba9876543210fedcba9876543210", plan_id: "p", state: "running" });
  vi.mocked(api.getAddSongsJob).mockResolvedValue(jobOf());
});
afterEach(() => { cleanup(); vi.clearAllMocks(); window.localStorage.clear(); });

describe("Pick one: the files are buttons on the card", () => {
  it("shows the chip and one small radio button per file, named by the file without .pro, in a group named for the song", async () => {
    await previewed();
    const li = card("Firm Foundation");
    expect(within(li).getByTestId("add-songs-stuck")).toHaveTextContent("Pick one");
    const radios = within(group()).getAllByRole("radio");
    expect(radios.map((radio) => radio.textContent)).toEqual(["Firm Foundation (Live)", "Firm Foundation"]);
    expect(radios.map((radio) => radio.getAttribute("aria-checked"))).toEqual(["false", "false"]);
    // Under the title, inside the card's border, and never inside the card's own checkbox button.
    expect(li).toContainElement(group());
    expect(screen.getByRole("checkbox", { name: "Firm Foundation" })).not.toContainElement(group());
    // The song that needs nothing has no buttons.
    expect(within(card("Holy Forever")).queryByRole("radiogroup")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("is 44 px tall on a phone and wraps: the buttons are touch-size and the group wraps onto more lines", async () => {
    await previewed();
    expect(group().className).toMatch(/\bflex-wrap\b/);
    for (const radio of within(group()).getAllByRole("radio")) {
      expect(radio).toHaveAttribute("data-size", "touch");
      expect(radio.className).toMatch(/min-h-ds-touch-target/);
    }
  });

  it("says the library too, only when two libraries have a file of the same name", async () => {
    vi.mocked(api.previewAddSongs).mockImplementation(async () => addPlan([holy(), firm({ action: "needs_choice", file: null, source: null, choices: ["Songs / Firm Foundation.pro", "Other / Firm Foundation.pro", "Songs / Firm Foundation (Live).pro"] })]));
    await previewed();
    expect(within(group()).getAllByRole("radio").map((radio) => radio.textContent)).toEqual(["Songs / Firm Foundation", "Other / Firm Foundation", "Firm Foundation (Live)"]);
  });

  it("is one tab stop; the arrow keys, Home and End move between the buttons and do not ask for anything", async () => {
    const user = await previewed();
    const [live, plain] = within(group()).getAllByRole("radio");
    expect([live!.tabIndex, plain!.tabIndex]).toEqual([0, -1]);
    live!.focus();
    await user.keyboard("{ArrowRight}");
    expect(plain).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(live).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(plain).toHaveFocus();
    await user.keyboard("{Home}");
    expect(live).toHaveFocus();
    await user.keyboard("{End}");
    expect(plain).toHaveFocus();
    expect(previewRequests()).toHaveLength(1);
    await user.keyboard(" ");
    await waitFor(() => expect(previewRequests()).toHaveLength(2));
  });

  it("keeps Import from going on: the preview stopped, nothing was applied, and the count line says 1 pick one", async () => {
    await previewed();
    expect(api.applyAddSongs).not.toHaveBeenCalled();
    expect(screen.getByTestId("add-songs-summary")).toHaveTextContent("1 to add · 1 pick one");
    expect(screen.getByTestId("add-songs-reason")).toHaveTextContent("1 can't be imported yet");
    expect(screen.getByRole("button", { name: "Skip those and import the rest" })).toBeEnabled();
  });
});

describe("pressing a file", () => {
  it("asks for a new preview with sources, and the card turns into its real status with a quiet phrase and Change", async () => {
    const user = await previewed();
    await user.click(within(group()).getByRole("radio", { name: "Firm Foundation (Live)" }));
    await waitFor(() => expect(previewRequests()).toHaveLength(2));
    expect(previewRequests()[0]).not.toHaveProperty("sources");
    expect(previewRequests()[1]).toMatchObject({ setlist_song_ids: [2000001, 2000002], sources: { [FIRM_REF]: LIVE } });
    const li = card("Firm Foundation");
    await waitFor(() => expect(within(li).getByTestId("add-songs-using-chip")).toHaveTextContent("New copy"));
    expect(within(li).getByTestId("add-songs-using")).toHaveTextContent("Using “Firm Foundation (Live)”");
    expect(within(li).queryByRole("radiogroup")).toBeNull();
    expect(within(li).queryByTestId("add-songs-stuck")).toBeNull();
    // The chip's hover text keeps the full sentence.
    expect(within(li).getByTestId("add-songs-using-chip").getAttribute("title")).toContain("Used \"Firm Foundation (Live)\": the file you picked.");
    // The preview is the new one, nothing was imported, and the focus is on Change.
    expect(screen.getByTestId("add-songs-summary")).toHaveTextContent("2 to add");
    expect(api.applyAddSongs).not.toHaveBeenCalled();
    expect(within(li).getByRole("button", { name: "Firm Foundation: change the file" })).toHaveFocus();
  });

  it("shows the loading sweep on the pressed button while it waits, and nothing else moves", async () => {
    const user = await previewed();
    let answer: (plan: AddSongsPlan) => void = () => undefined;
    vi.mocked(api.previewAddSongs).mockImplementation(() => new Promise<AddSongsPlan>((resolve) => { answer = resolve; }));
    await user.click(within(group()).getByRole("radio", { name: "Firm Foundation" }));
    const pressed = within(group()).getByRole("radio", { name: "Firm Foundation" });
    expect(pressed).toHaveAttribute("data-loading", "true");
    expect(within(group()).getByRole("radio", { name: "Firm Foundation (Live)" })).not.toHaveAttribute("data-loading");
    // The page is as it was: the preview is still there, the button does not say Importing and the cards are on.
    expect(screen.getByTestId("add-songs-preview")).toBeInTheDocument();
    expect(screen.getByRole("button", ADD)).toHaveTextContent("Import songs to ProPresenter");
    expect(screen.getByRole("checkbox", { name: "Holy Forever" })).toBeEnabled();
    await act(async () => { answer(using(PLAIN, "pick")); });
    await waitFor(() => expect(within(card("Firm Foundation")).getByTestId("add-songs-using")).toHaveTextContent("Using “Firm Foundation”"));
  });

  it("is imported on the next press of the button, with the pick sent again (a preview, then the apply of THAT plan)", async () => {
    const user = await previewed();
    await user.click(within(group()).getByRole("radio", { name: "Firm Foundation (Live)" }));
    await waitFor(() => expect(within(card("Firm Foundation")).getByTestId("add-songs-using")).toBeInTheDocument());
    await user.click(screen.getByRole("button", ADD));
    await waitFor(() => expect(api.applyAddSongs).toHaveBeenCalledTimes(1));
    expect(previewRequests()[2]).toMatchObject({ sources: { [FIRM_REF]: LIVE } });
    expect(vi.mocked(api.applyAddSongs).mock.calls[0]![1]).toEqual([]);
  });

  it("Change brings the buttons back with the file in use checked; another file replaces the pick; the same file just closes them", async () => {
    const user = await previewed();
    await user.click(within(group()).getByRole("radio", { name: "Firm Foundation (Live)" }));
    const change = await screen.findByRole("button", { name: "Firm Foundation: change the file" });
    expect(change).toHaveTextContent("Change");
    await user.click(change);
    const radios = within(group()).getAllByRole("radio");
    expect(radios.map((radio) => radio.getAttribute("aria-checked"))).toEqual(["true", "false"]);
    expect(radios[0]).toHaveTextContent("\u2713");
    expect(screen.queryByRole("button", { name: "Firm Foundation: change the file" })).toBeNull();
    await user.click(radios[0]!);
    expect(previewRequests()).toHaveLength(2);
    expect(screen.queryByRole("radiogroup")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Firm Foundation: change the file" }));
    await user.click(within(group()).getByRole("radio", { name: "Firm Foundation" }));
    await waitFor(() => expect(previewRequests()).toHaveLength(3));
    expect(previewRequests()[2]).toMatchObject({ sources: { [FIRM_REF]: PLAIN } });
    await waitFor(() => expect(within(card("Firm Foundation")).getByTestId("add-songs-using")).toHaveTextContent("Using “Firm Foundation”"));
    expect(window.localStorage.getItem(MEMORY)).toBe(PLAIN);
  });

  it("a picked file that cannot make the arrangement shows its own words and keeps the buttons so the other can be picked", async () => {
    vi.mocked(api.previewAddSongs).mockImplementation(async (request) => (request.sources?.[FIRM_REF] === PLAIN
      ? addPlan([holy(), firm({ action: "blocked", file: null, source: null, reason: "These sections have no group with that name in the song: Chorus. Nothing is made for this song.", choices: [LIVE, PLAIN], chosen_by: "pick", chosen_file: PLAIN })])
      : asking()));
    const user = await previewed();
    await user.click(within(group()).getByRole("radio", { name: "Firm Foundation" }));
    const li = card("Firm Foundation");
    await waitFor(() => expect(within(li).getByTestId("add-songs-stuck")).toHaveTextContent("Can't map"));
    expect(within(group()).getAllByRole("radio").map((radio) => radio.getAttribute("aria-checked"))).toEqual(["false", "true"]);
    expect(within(li).queryByTestId("add-songs-change-file")).toBeNull();
  });

  it("a failed preview leaves the buttons and the last preview as they were and says so", async () => {
    const user = await previewed();
    vi.mocked(api.previewAddSongs).mockRejectedValue(new Error("down"));
    await user.click(within(group()).getByRole("radio", { name: "Firm Foundation" }));
    await waitFor(() => expect(screen.getByTestId("add-songs-problem")).toBeInTheDocument());
    expect(group()).toBeInTheDocument();
    expect(screen.getByTestId("add-songs-preview")).toBeInTheDocument();
    expect(within(group()).getAllByRole("radio")).toHaveLength(2);
    expect(within(group()).getAllByRole("radio").some((radio) => radio.hasAttribute("data-loading"))).toBe(false);
  });
});

describe("the name rule", () => {
  it("a song the engine settled by its name shows the quiet phrase and Change, without being asked", async () => {
    vi.mocked(api.previewAddSongs).mockImplementation(async () => using(LIVE, "title"));
    await previewed(songs(), false);
    await waitFor(() => expect(within(card("Firm Foundation")).getByTestId("add-songs-using")).toBeInTheDocument());
    const li = card("Firm Foundation");
    expect(within(li).getByTestId("add-songs-using-chip")).toHaveTextContent("New copy");
    expect(within(li).getByTestId("add-songs-using")).toHaveTextContent("Using “Firm Foundation (Live)”");
    expect(within(li).getByTestId("add-songs-using-chip").getAttribute("title")).toContain("its name matches the Playback title");
    expect(within(li).queryByRole("radiogroup")).toBeNull();
    // Nothing needed a pick: the button went on and imported.
    await waitFor(() => expect(api.applyAddSongs).toHaveBeenCalledTimes(1));
  });
});

describe("what is remembered", () => {
  it("is saved in this browser under the song's MultiTracks number, and only then", async () => {
    const user = await previewed();
    expect(window.localStorage.getItem(MEMORY)).toBeNull();
    await user.click(within(group()).getByRole("radio", { name: "Firm Foundation (Live)" }));
    await waitFor(() => expect(window.localStorage.getItem(MEMORY)).toBe(LIVE));
    expect(Object.keys(window.localStorage).filter((key) => key.startsWith(SONG_FILE_KEY_PREFIX))).toEqual([MEMORY]);
  });

  it("a second window with the same storage sends the same pick with its first preview and the card never asks", async () => {
    window.localStorage.setItem(MEMORY, PLAIN);
    await previewed(songs(), false);
    expect(previewRequests()[0]).toMatchObject({ sources: { [FIRM_REF]: PLAIN } });
    expect(previewRequests()[0]).not.toMatchObject({ sources: { "2000001": expect.anything() } });
    await waitFor(() => expect(within(card("Firm Foundation")).getByTestId("add-songs-using")).toHaveTextContent("Using “Firm Foundation”"));
    await waitFor(() => expect(api.applyAddSongs).toHaveBeenCalledTimes(1));
  });

  it("drops a remembered file that is no longer one of the song's files, silently, and the card asks again", async () => {
    window.localStorage.setItem(MEMORY, "Songs / Firm Foundation (Old).pro");
    await previewed();
    expect(previewRequests()[0]).toMatchObject({ sources: { [FIRM_REF]: "Songs / Firm Foundation (Old).pro" } });
    expect(window.localStorage.getItem(MEMORY)).toBeNull();
    expect(group()).toBeInTheDocument();
    expect(within(card("Firm Foundation")).getByTestId("add-songs-stuck")).toHaveTextContent("Pick one");
    expect(screen.queryByTestId("add-songs-problem")).toBeNull();
    expect(api.applyAddSongs).not.toHaveBeenCalled();
  });

  it("a song with no MultiTracks number is not remembered, but its pick lasts while the section is open", async () => {
    vi.mocked(api.previewAddSongs).mockImplementation(async (request) => {
      const file = request.sources?.[FIRM_REF];
      return file ? using(file, "pick", { content_id: null }) : addPlan([holy(), firm({ content_id: null, action: "needs_choice", file: null, source: null, choices: [LIVE, PLAIN] })]);
    });
    const user = await previewed(songs(null));
    await user.click(within(group()).getByRole("radio", { name: "Firm Foundation (Live)" }));
    await waitFor(() => expect(within(card("Firm Foundation")).getByTestId("add-songs-using")).toBeInTheDocument());
    expect(Object.keys(window.localStorage).filter((key) => key.startsWith(SONG_FILE_KEY_PREFIX))).toEqual([]);
    await user.click(screen.getByRole("button", ADD));
    await waitFor(() => expect(previewRequests()).toHaveLength(3));
    expect(previewRequests()[2]).toMatchObject({ sources: { [FIRM_REF]: LIVE } });
  });
});
