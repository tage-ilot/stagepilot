import { render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import { libraryController, libraryPlan, librarySong, librarySongOf } from "../test/libraryFixtures";
import { helperStatus, linkController, playlists, presentation } from "../test/propresenterLinkFixtures";
import type { Song } from "../types";
import { ProPresenterSetupPanel } from "./ProPresenterSetupPanel";
import { PRESENTER_LINK_OPEN_KEY } from "./proPresenterLinkState";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getProPresenterPlaylists: vi.fn(),
  lookupAddSongsTitles: vi.fn(),
}));

const song = (title: string, order: number): Song => ({ id: `s${order}`, title, duration_seconds: 240, order, is_generic: false, source_song_id: null });
const plan = { id: "p", title: "Sunday", date: "2026-11-22", service_type: "Sunday", service_type_id: null, service_times: [], duration_source: "plan", songs: [song("Holy Forever", 0), song("Gratitude", 1)] };
const panel = (over: Partial<Parameters<typeof ProPresenterSetupPanel>[0]> = {}) => render(
  <ProPresenterSetupPanel error={null} link={linkController(helperStatus("ready"))} message={null} onClose={vi.fn()} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} plan={plan} propresenter={presentation()} {...over} />,
);
const before = (a: Element, b: Element) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

afterEach(() => vi.clearAllMocks());
beforeEach(() => {
  window.localStorage.clear();
  // The person has opened the section (it starts folded): these tests are about what is inside it.
  window.localStorage.setItem(PRESENTER_LINK_OPEN_KEY, "true");
  vi.mocked(api.getProPresenterPlaylists).mockResolvedValue({ playlists: playlists() });
  vi.mocked(api.lookupAddSongsTitles).mockImplementation(async (titles) => ({ titles: titles.map((title) => ({ title, state: "found" as const })) }));
});

describe("the Presenter Link section in the ProPresenter panel", () => {
  it("sits after the settings and the Refresh row, below the Live Link block, and before the Save bar and the summary", async () => {
    const { container } = panel();
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    const body = container.querySelector("[data-panel-body]")!;
    const parts = Array.from(body.children);
    const live = screen.getByRole("region", { name: "ProPresenter Live Link" });
    const link = screen.getByTestId("presenter-link-section");
    expect(parts[0]).toBe(live);
    expect(parts.indexOf(link)).toBeGreaterThan(parts.findIndex((part) => part.matches("[data-section=actions]")));
    expect(before(screen.getByRole("button", { name: "Refresh timers and Looks" }), link)).toBe(true);
    expect(before(link, screen.getByRole("button", { name: "Save configuration" }))).toBe(true);
    expect(before(link, container.querySelector("[data-section=summary]")!)).toBe(true);
    expect(within(link).getByRole("heading", { name: "Presenter Link" })).toBeInTheDocument();
  });

  it("lists the loaded plan's Planning Center names when there is no Playback data", async () => {
    panel({ library: undefined });
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    expect(screen.getByTestId("add-songs-songs")).toHaveAttribute("data-source", "planning-center");
    expect(screen.getAllByTestId("add-songs-row").map((row) => within(row).getByRole("checkbox").getAttribute("aria-label"))).toEqual(["Holy Forever", "Gratitude"]);
  });

  it("lists Playback's songs, with key, mtID and the order, when the Library Link has given them", async () => {
    const library = libraryController("ready", { plan: libraryPlan([librarySongOf("Holy Forever", 1, { library: librarySong({ title: "Holy Forever", position: 1, setlist_song_id: 30389715, content_id: 5123, key: "E", arrangement: "Radio" }) })]) });
    panel({ library });
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    const row = screen.getByTestId("add-songs-row");
    expect(row).toHaveAttribute("data-source", "playback");
    expect(row).toHaveTextContent("E mtID-5123");
    expect(row).not.toHaveTextContent("30389715");
    // The arrangement's name is not on the card; the order Playback plays is.
    expect(row).not.toHaveTextContent("Radio");
    expect(row).toHaveTextContent("Count Off \u00B7 Verse 1 \u00B7 Vamp on 1");
  });

  it.each([
    ["no Link controller (a page that does not know it)", { link: undefined }],
    ["not installed", { link: linkController(helperStatus("not_installed")) }],
    ["not answering", { link: linkController(helperStatus("not_answering")) }],
  ])("is not drawn, and says nothing about it, when the Link is %s", (_name, over) => {
    panel(over);
    expect(screen.queryByTestId("presenter-link-section")).toBeNull();
    expect(screen.queryByRole("button", { name: "Import songs to ProPresenter" })).toBeNull();
    expect(screen.queryByText("Presenter Link", { selector: "h3" })).toBeNull();
    expect(api.getProPresenterPlaylists).not.toHaveBeenCalled();
  });

  it("is still not the way the Playback panel adds songs: the button is in this panel only", async () => {
    panel();
    await screen.findByRole("option", { name: "Sunday Morning Service" });
    expect(screen.getAllByRole("button", { name: "Import songs to ProPresenter" })).toHaveLength(1);
  });
});
