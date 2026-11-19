import { render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { Frame } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The Presenter Link states of the section gallery: each one is reached by pressing the real buttons (made-up answers, no backend). */
const real = window.fetch;
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });
const WAIT = { timeout: 15000 };
const sample = (id: string) => document.querySelector(`[data-sample="presenter-link-${id}"]`) as HTMLElement;
const hook = (id: string, testId: string) => () => { const found = sample(id)?.querySelector(`[data-testid="${testId}"]`); expect(found, `${id}: ${testId}`).not.toBeNull(); return found as HTMLElement; };
const draw = () => render(<Frame only={["presenter-link"]} />);

describe("the Presenter Link states", () => {
  it("lists every state, as the task names them", () => {
    draw();
    const ids = Array.from(document.querySelectorAll('[data-group="presenter-link"] [data-sample]')).map((node) => node.getAttribute("data-sample")?.replace("presenter-link-", ""));
    expect(ids).toEqual(["no-playlists", "options", "end-of-playlist", "two-headers", "names", "playback", "mixed", "playback-stopped", "swap", "previewing", "progress", "result", "result-names", "live", "library", "try-again", "this-computer", "not-ready"]);
  });

  it("no playlists yet: nothing to choose, the button off with its reason", async () => {
    draw();
    await waitFor(() => expect(within(sample("no-playlists")).getByTestId("add-songs-reason")).toHaveTextContent("Choose a playlist."), WAIT);
    expect(within(sample("no-playlists")).getByRole("button", { name: "Import songs to ProPresenter" })).toBeDisabled();
    expect(within(sample("no-playlists")).getByLabelText("Playlist").querySelectorAll("option")).toHaveLength(1);
  });

  it("options chosen: the playlist and the section are picked and the button is on", async () => {
    draw();
    await waitFor(() => expect(within(sample("options")).getByLabelText("Section")).toHaveValue("Worship"), WAIT);
    expect(within(sample("options")).getByRole("button", { name: "Import songs to ProPresenter" })).toBeEnabled();
    expect(Array.from(within(sample("options")).getByLabelText("Section").querySelectorAll("option")).map((o) => o.textContent)).toEqual(["Welcome", "Worship", "Message", "Closing", "End of Playlist"]);
  });

  it("the Section box: End of Playlist first, and two headers of one name show the engine's line and stop the button", async () => {
    draw();
    await waitFor(() => expect(within(sample("end-of-playlist")).getByLabelText("Playlist")).toHaveValue("PL-SUNDAY"), WAIT);
    expect(within(sample("end-of-playlist")).getByLabelText("Section")).toHaveValue("");
    await waitFor(() => expect(hook("two-headers", "add-songs-two-headers")()).toHaveTextContent("More than one header in this playlist is named \u201CWorship\u201D."), WAIT);
    expect(within(sample("two-headers")).getByRole("button", { name: "Import songs to ProPresenter" })).toBeDisabled();
  });

  it("Planning Center names only: cards with a Planning Center chip; one is Not in ProPresenter and cannot be selected", async () => {
    draw();
    await waitFor(() => expect(sample("names").querySelector('[data-blocked="missing"]')).not.toBeNull(), WAIT);
    const blocked = sample("names").querySelector('[data-blocked="missing"]') as HTMLElement;
    expect(blocked).toHaveTextContent("Firm Foundation");
    expect(blocked).toHaveTextContent("Not in ProPresenter");
    expect(within(blocked).getByRole("checkbox")).toBeDisabled();
    expect(sample("names").querySelector('[data-testid="add-songs-songs"]')).toHaveAttribute("data-source", "planning-center");
    expect(within(sample("names")).getAllByTestId("add-songs-source-chip")).toHaveLength(4);
  });

  it("Playback data: four songs with id, key and arrangement, and a section order that opens", async () => {
    draw();
    await waitFor(() => expect(within(sample("playback")).getByTestId("add-songs-order")).toBeInTheDocument(), WAIT);
    const rows = within(sample("playback")).getAllByTestId("add-songs-row");
    expect(rows).toHaveLength(4);
    expect(rows[0]).toHaveTextContent("5000101");
    expect(rows[0]).toHaveTextContent("Key G (was F)");
    expect(rows[0]).toHaveTextContent("Radio");
    expect(within(rows[0]!).getByTestId("library-bar")).toBeInTheDocument();
  });

  it("mixed selection: two cards off, the count says so", async () => {
    draw();
    await waitFor(() => expect(hook("mixed", "add-songs-count")()).toHaveTextContent("2 of 4 selected"), WAIT);
    expect(within(sample("mixed")).getByRole("checkbox", { name: "Battle Belongs" })).not.toBeChecked();
    expect(within(sample("mixed")).getByRole("checkbox", { name: "Holy Forever" })).toBeChecked();
  });

  it("one press stops at a preview with a blocked song: the reason, the guess line and Skip", async () => {
    draw();
    await waitFor(() => expect(within(sample("playback-stopped")).getByRole("button", { name: "Skip those and import the rest" })).toBeEnabled(), WAIT);
    expect(hook("playback-stopped", "add-songs-reason")()).toHaveTextContent("1 song cannot be imported yet (Firm Foundation)");
    expect(hook("playback-stopped", "add-songs-stuck")()).toHaveTextContent("Blocked");
    expect(hook("playback-stopped", "add-songs-guessed")()).toHaveTextContent("Guessed");
    expect(within(sample("playback-stopped")).getAllByTestId("add-songs-mapping").map((node) => node.textContent).join(" ")).toContain("Interlude (1st use) uses the 2nd");
  });

  it("the swap keeps the person's tick: Battle Belongs stays off when Playback gives the songs, and again when it loses them", async () => {
    draw();
    const box = (name: string) => ({ checked: within(sample("swap")).getByRole("checkbox", { name }).getAttribute("aria-checked") === "true" });
    await waitFor(() => expect(sample("swap").querySelector('[data-testid="add-songs-songs"]')).toHaveAttribute("data-source", "playback"), WAIT);
    expect(box("Battle Belongs").checked).toBe(false);
    expect(box("Holy Forever").checked).toBe(true);
    expect(within(sample("swap")).getAllByTestId("add-songs-multitracks")).toHaveLength(4);
    within(sample("swap")).getByRole("button", { name: "Playback loses the songs" }).click();
    await waitFor(() => expect(sample("swap").querySelector('[data-testid="add-songs-songs"]')).toHaveAttribute("data-source", "planning-center"), WAIT);
    expect(box("Battle Belongs").checked).toBe(false);
  });

  it("previewing, progress, result, the refusal and the library instruction are each reached by pressing the button", async () => {
    draw();
    await waitFor(() => expect(within(sample("previewing")).getByRole("button", { name: /Importing/ })).toBeDisabled(), WAIT);
    await waitFor(() => expect(within(sample("progress")).getByTestId("add-songs-progress")).toBeInTheDocument(), WAIT);
    await waitFor(() => expect(within(sample("result")).getByTestId("add-songs-result")).toHaveAttribute("data-outcome", "done"), WAIT);
    expect(within(sample("result")).getByTestId("add-songs-guess-note")).toHaveTextContent("Check in ProPresenter:");
    await waitFor(() => expect(within(sample("result-names")).getByTestId("add-songs-result")).toHaveAttribute("data-outcome", "done"), WAIT);
    await waitFor(() => expect(within(sample("live")).getByTestId("add-songs-result")).toHaveAttribute("data-outcome", "live_on_screen"), WAIT);
    expect(within(sample("live")).getByRole("button", { name: "Try again" })).toBeInTheDocument();
    await waitFor(() => expect(within(sample("library")).getByTestId("add-songs-open-library")).toBeInTheDocument(), WAIT);
  });

  it("Try again makes a new preview and the second job runs", async () => {
    draw();
    // The first job was refused (live on screen); the second is running: its newest step is on screen and the first result is gone.
    await waitFor(() => expect(within(sample("try-again")).getByTestId("add-songs-now")).toHaveTextContent("Making the copy"), WAIT);
    expect(within(sample("try-again")).queryByTestId("add-songs-result")).toBeNull();
    expect(within(sample("try-again")).queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("on this computer the section is the same, and with the Link not ready nothing is drawn", async () => {
    draw();
    await waitFor(() => expect(within(sample("this-computer")).getByLabelText("Section")).toHaveValue("Worship"), WAIT);
    expect(sample("not-ready").querySelector('[data-testid="presenter-link-section"]')).toBeNull();
    expect(sample("not-ready").textContent).not.toMatch(/Import songs to ProPresenter/);
  });
});
