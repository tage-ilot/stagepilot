import { render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { Frame } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The Presenter Link states of the section gallery: each one is reached by pressing the real buttons (made-up answers, no backend). */
const real = window.fetch;
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });
// The gallery presses every sample's real buttons one after another; the slow macOS Intel runner needs well over 15 s for the later ones (release dry run, twice, a different test each time).
const WAIT = { timeout: 45000 };
const sample = (id: string) => document.querySelector(`[data-sample="presenter-link-${id}"]`) as HTMLElement;
const hook = (id: string, testId: string) => () => { const found = sample(id)?.querySelector(`[data-testid="${testId}"]`); expect(found, `${id}: ${testId}`).not.toBeNull(); return found as HTMLElement; };
const draw = () => render(<Frame only={["presenter-link"]} />);

describe("the Presenter Link states", { timeout: 60000 }, () => {
  it("lists every state, as the task names them", () => {
    draw();
    const ids = Array.from(document.querySelectorAll('[data-group="presenter-link"] [data-sample]')).map((node) => node.getAttribute("data-sample")?.replace("presenter-link-", ""));
    expect(ids).toEqual(["no-playlists", "options", "end-of-playlist", "no-sections", "two-headers", "names", "playback", "mixed", "no-songs", "playback-stopped", "pick-one", "picked", "picked-by-name", "statuses", "playing", "swap", "previewing", "progress", "result", "result-differs", "result-stopped", "result-names", "live", "library", "try-again", "this-computer", "not-ready"]);
  });

  it("no playlists yet: nothing to choose, the button off with its reason", async () => {
    draw();
    await waitFor(() => expect(within(sample("no-playlists")).getByTestId("add-songs-reason")).toHaveTextContent("Choose a playlist"), WAIT);
    expect(within(sample("no-playlists")).getByRole("button", { name: "Import songs to ProPresenter" })).toBeDisabled();
    expect(within(sample("no-playlists")).getByLabelText("Playlist").querySelectorAll("option")).toHaveLength(1);
  });

  it("options chosen: the playlist and the section are picked and the button is on", async () => {
    draw();
    await waitFor(() => expect(within(sample("options")).getByLabelText("Section")).toHaveValue("Worship"), WAIT);
    expect(within(sample("options")).getByRole("button", { name: "Import songs to ProPresenter" })).toBeEnabled();
    expect(Array.from(within(sample("options")).getByLabelText("Section").querySelectorAll("option")).map((o) => o.textContent)).toEqual(["Welcome", "Worship", "Message", "Closing"]);
  });

  it("the Section box: the last section first (End of Playlist only with no section), and two headers of one name show the engine's line and stop the button", async () => {
    draw();
    await waitFor(() => expect(within(sample("end-of-playlist")).getByLabelText("Playlist")).toHaveValue("PL-SUNDAY"), WAIT);
    expect(within(sample("end-of-playlist")).getByLabelText("Section")).toHaveValue("Closing");
    await waitFor(() => expect(within(sample("no-sections")).getByLabelText("Playlist")).toHaveValue("PL-WED"), WAIT);
    expect(Array.from(within(sample("no-sections")).getByLabelText("Section").querySelectorAll("option")).map((o) => o.textContent)).toEqual(["End of Playlist"]);
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

  it("Playback data: four songs with key, mtID and the order written on the card, never Playback's own song number", async () => {
    draw();
    await waitFor(() => expect(within(sample("playback")).getAllByTestId("add-songs-order")).toHaveLength(4), WAIT);
    const rows = within(sample("playback")).getAllByTestId("add-songs-row");
    expect(rows).toHaveLength(4);
    expect(rows.map((row) => within(row).getByTestId("add-songs-mtid").textContent)).toEqual(["43302", "23126", "31877", "8412"]);
    expect(rows[0]).toHaveTextContent("G (was F) mtID-43302");
    expect(rows[0]).toHaveTextContent("Count Off \u00B7 Intro \u00B7 Verse 1 \u00B7 Chorus");
    expect(rows[0]).not.toHaveTextContent("Radio");
    expect(rows[0]?.textContent).not.toMatch(/3038971\d/);
    expect(within(rows[0]!).queryByRole("button", { name: /Sections/ })).toBeNull();
    expect(within(rows[0]!).getByTestId("library-bar")).toBeInTheDocument();
  });

  it("the reason beside the button is a pill: no playlist chosen, and no song ticked", async () => {
    draw();
    await waitFor(() => expect(within(sample("no-playlists")).getByTestId("add-songs-reason")).toHaveAttribute("data-reason-pill", "true"), WAIT);
    await waitFor(() => expect(within(sample("no-songs")).getByTestId("add-songs-reason")).toHaveTextContent("Select a song"), WAIT);
    const pill = within(sample("no-songs")).getByTestId("add-songs-reason");
    expect(pill).toHaveAttribute("data-reason-pill", "true");
    expect(within(sample("no-songs")).getByRole("button", { name: "Import songs to ProPresenter" })).toBeDisabled();
    expect(pill.closest("[data-part=buttons]")).toContainElement(within(sample("no-songs")).getByRole("button", { name: "Import songs to ProPresenter" }));
  }, 30000);

  it("mixed selection: two cards off, the count says so", async () => {
    draw();
    await waitFor(() => expect(hook("mixed", "add-songs-count")()).toHaveTextContent("2 of 4 selected"), WAIT);
    expect(within(sample("mixed")).getByRole("checkbox", { name: "Battle Belongs" })).not.toBeChecked();
    expect(within(sample("mixed")).getByRole("checkbox", { name: "Holy Forever" })).toBeChecked();
  });

  it("one press stops at a preview with a blocked song: the reason, the guess line and Skip", async () => {
    draw();
    await waitFor(() => expect(within(sample("playback-stopped")).getByRole("button", { name: "Skip those and import the rest" })).toBeEnabled(), WAIT);
    expect(hook("playback-stopped", "add-songs-reason")()).toHaveTextContent("1 can't be imported yet");
    expect(hook("playback-stopped", "add-songs-stuck")()).toHaveTextContent("Can't map");
    expect(hook("playback-stopped", "add-songs-guessed")()).toHaveTextContent("Check");
    expect(hook("playback-stopped", "add-songs-summary")()).toHaveTextContent("3 to add \u00B7 1 can't map");
    const phrases = within(sample("playback-stopped")).getAllByTestId("add-songs-phrase").map((node) => node.textContent);
    expect(phrases).toContain("Interlude (3rd use): 2nd group");
    expect(phrases).toContain("Missing: Bridge");
  });

  it("Pick one: the card has the files as radio buttons under the title and nothing was imported", async () => {
    draw();
    await waitFor(() => expect(within(sample("pick-one")).getByRole("radiogroup", { name: "Firm Foundation: use file" })).toBeInTheDocument(), WAIT);
    const radios = within(sample("pick-one")).getAllByRole("radio");
    expect(radios.map((radio) => radio.textContent)).toEqual(["Firm Foundation (Live)", "Firm Foundation"]);
    expect(within(sample("pick-one")).getByTestId("add-songs-summary")).toHaveTextContent("1 pick one");
    expect(within(sample("pick-one")).getByRole("checkbox", { name: "Firm Foundation" }).closest("li")).toContainElement(radios[0]!);
    expect(within(sample("pick-one")).queryByRole("dialog")).toBeNull();
  });

  it("Pick one, picked: the card is New copy with its quiet phrase and Change, and the buttons are gone", async () => {
    draw();
    await waitFor(() => expect(within(sample("picked")).getByTestId("add-songs-using")).toHaveTextContent("Using \u201CFirm Foundation (Live)\u201D"), WAIT);
    const li = within(sample("picked")).getByRole("checkbox", { name: "Firm Foundation" }).closest("li") as HTMLElement;
    expect(within(li).getByTestId("add-songs-using-chip")).toHaveTextContent("New copy");
    expect(within(li).getByRole("button", { name: "Firm Foundation: change the file" })).toHaveTextContent("Change");
    expect(within(li).queryByRole("radiogroup")).toBeNull();
    expect(window.localStorage.getItem("stagepilot.presenter-song-file.8412")).toBe("Songs / Firm Foundation (Live).pro");
  });

  it("Pick one, decided by the name rule: the same phrase and Change, and Change brings the buttons back with the file checked", async () => {
    draw();
    await waitFor(() => expect(within(sample("picked-by-name")).getByTestId("add-songs-using")).toHaveTextContent("Using \u201CFirm Foundation\u201D"), WAIT);
    const li = within(sample("picked-by-name")).getByRole("checkbox", { name: "Firm Foundation" }).closest("li") as HTMLElement;
    expect(within(li).getByTestId("add-songs-using-chip").getAttribute("title")).toContain("its name matches the Playback title");
    within(li).getByRole("button", { name: "Firm Foundation: change the file" }).click();
    await waitFor(() => expect(within(li).getAllByRole("radio").map((radio) => radio.getAttribute("aria-checked"))).toEqual(["false", "true"]), WAIT);
  });

  it("every status in one preview: the four of the old screenshot, then will import, can't map, no mtID, check, ready and new copy", async () => {
    draw();
    await waitFor(() => expect(hook("statuses", "add-songs-preview")()).toBeInTheDocument(), WAIT);
    const rows = within(sample("statuses")).getAllByTestId("add-songs-preview-song");
    expect(rows.map((row) => [row.querySelector("h3, span")?.textContent, within(row).getByTestId("add-songs-status").textContent?.replace(/^[^A-Za-z]+/, "")])).toEqual([
      ["Holy Forever", "In playlist"], ["Battle Belongs", "Can't import"], ["Gratitude", "In playlist"], ["Firm Foundation", "Pick one"],
      ["Living Hope", "Will import"], ["Graves Into Gardens", "Can't map"], ["Goodness of God", "No mtID"], ["What a Beautiful Name", "Check"],
      ["Great Are You Lord", "Ready"], ["Build My Life", "New copy"],
    ]);
    expect(rows.map((row) => row.querySelector("[data-testid=add-songs-phrase]")?.textContent ?? null)).toEqual([
      null, "Needs a Mac", null, null, null, "Missing: Bridge 2", null, "Interlude (3rd use): 2nd group", "Copy exists", "Playback order, \u201C(2)\u201D",
    ]);
    expect(within(rows[3]!).getAllByRole("listitem").map((file) => file.textContent)).toEqual(["Songs / Firm Foundation (Live).pro", "Songs / Firm Foundation.pro"]);
    expect(within(sample("statuses")).getByTestId("add-songs-summary")).toHaveTextContent("4 to add \u00B7 2 in playlist \u00B7 1 can't import \u00B7 1 can't map \u00B7 1 no mtID \u00B7 1 pick one");
    expect(within(sample("statuses")).getByTestId("add-songs-playlist-line")).toHaveTextContent("Sunday Morning Service \u203A Worship");
    expect(within(sample("statuses")).queryByTestId("add-songs-version")).toBeNull();
    // The cards are all ten songs, and the one with no number says so.
    expect(within(sample("statuses")).getAllByTestId("add-songs-row")).toHaveLength(10);
    expect(within(sample("statuses")).getByRole("checkbox", { name: "Goodness of God" }).closest("li")).toHaveTextContent("Bb mtID\u2014");
  });

  it("a song is playing: one short line at the top", async () => {
    draw();
    await waitFor(() => expect(hook("playing", "add-songs-playing")()).toHaveTextContent(/^Song playing: wait$/), WAIT);
  });

  it("the swap keeps the person's tick: Battle Belongs stays off when Playback gives the songs, and again when it loses them", async () => {
    draw();
    const box = (name: string) => ({ checked: within(sample("swap")).getByRole("checkbox", { name }).getAttribute("aria-checked") === "true" });
    await waitFor(() => expect(sample("swap").querySelector('[data-testid="add-songs-songs"]')).toHaveAttribute("data-source", "playback"), WAIT);
    expect(box("Battle Belongs").checked).toBe(false);
    expect(box("Holy Forever").checked).toBe(true);
    expect(within(sample("swap")).getAllByTestId("add-songs-mtid")).toHaveLength(4);
    within(sample("swap")).getByRole("button", { name: "Playback loses the songs" }).click();
    await waitFor(() => expect(sample("swap").querySelector('[data-testid="add-songs-songs"]')).toHaveAttribute("data-source", "planning-center"), WAIT);
    expect(box("Battle Belongs").checked).toBe(false);
  });

  it("previewing, progress, result, the refusal and the library instruction are each reached by pressing the button", async () => {
    draw();
    await waitFor(() => expect(within(sample("previewing")).getByRole("button", { name: /Importing/ })).toBeDisabled(), WAIT);
    await waitFor(() => expect(within(sample("progress")).getByTestId("add-songs-progress")).toBeInTheDocument(), WAIT);
    await waitFor(() => expect(within(sample("result")).getByTestId("add-songs-result")).toHaveAttribute("data-outcome", "done"), WAIT);
    expect(within(sample("result")).getByTestId("add-songs-result").querySelector("h3")).toHaveTextContent("Done: 4 added");
    expect(within(sample("result")).getByTestId("add-songs-guess-note")).toHaveTextContent("Check");
    expect(within(sample("result")).getByTestId("add-songs-guess-note")).toHaveTextContent("Interlude (3rd use): 2nd group");
    await waitFor(() => expect(within(sample("result-differs")).getByTestId("add-songs-result").querySelector("h3")).toHaveTextContent("3 added, 1 differs"), WAIT);
    expect(within(sample("result-differs")).getByTestId("add-songs-differences")).toHaveTextContent("Item 7: duration 80 \u2192 0");
    await waitFor(() => expect(within(sample("result-stopped")).getByTestId("add-songs-result").querySelector("h3")).toHaveTextContent("Stopped: Can't import \u201CBattle Belongs\u201D"), WAIT);
    await waitFor(() => expect(within(sample("result-names")).getByTestId("add-songs-result")).toHaveAttribute("data-outcome", "done"), WAIT);
    await waitFor(() => expect(within(sample("live")).getByTestId("add-songs-result")).toHaveAttribute("data-outcome", "live_on_screen"), WAIT);
    expect(within(sample("live")).getByTestId("add-songs-result").querySelector("h3")).toHaveTextContent("Live on screen: clear it, then try again");
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
