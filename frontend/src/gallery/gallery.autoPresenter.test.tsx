import { render, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { Frame } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The Auto-Presenter states of the section gallery: made-up answers, no backend; the switch really switches. */
const real = window.fetch;
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });
const WAIT = { timeout: 60000 };
const sample = (id: string) => document.querySelector(`[data-sample="auto-presenter-${id}"]`) as HTMLElement;
const draw = () => render(<Frame only={["auto-presenter"]} />);
const summary = (id: string) => within(sample(id)).getByTestId("auto-presenter-summary");
const chips = (id: string) => within(sample(id)).getAllByTestId("follow-song-chip").map((chip) => chip.textContent?.replace(/^\S\s*/, "") ?? "");
const toggle = (id: string) => within(sample(id)).getByRole("button", { name: /^Auto-Presenter/ });
const box = (id: string) => within(sample(id)).getByRole("switch", { name: "Auto-Presenter" });

describe("the Auto-Presenter states", { timeout: 120000 }, () => {
  it("lists every state, as the task names them", () => {
    draw();
    const ids = Array.from(document.querySelectorAll('[data-group="auto-presenter"] [data-sample]')).map((node) => node.getAttribute("data-sample")?.replace("auto-presenter-", ""));
    expect(ids).toEqual(["off", "starts-folded", "following", "folded", "held", "paused", "no-cues", "mismatch", "aligned", "not-followed", "locked-library", "locked-keychain", "locked-link", "waiting", "timing-changed", "timing", "timing-error"]);
  });

  it("off: Off, every row says the switch is off, nothing to press but the switch; a new install starts folded", async () => {
    draw();
    await waitFor(() => expect(chips("off")).toEqual(Array(4).fill("Switch is off")), WAIT);
    expect(summary("off")).toHaveTextContent("Off");
    expect(box("off")).not.toBeChecked();
    expect(within(sample("off")).queryByRole("button", { name: "Try again" })).toBeNull();
    expect(toggle("starts-folded")).toHaveAttribute("aria-expanded", "false");
    expect(within(sample("starts-folded")).getByTestId("presenter-link-summary")).toBeInTheDocument();
  });

  it("pressing the switch in the gallery turns it on, like the real one", async () => {
    draw();
    await waitFor(() => expect(box("off")).toBeEnabled(), WAIT);
    await userEvent.setup().click(box("off"));
    await waitFor(() => expect(summary("off")).toHaveTextContent("Following: Chorus, slide 3"), WAIT);
  });

  it("following, folded, held, paused: the words of each, and Try again only after failures", async () => {
    draw();
    await waitFor(() => expect(summary("following")).toHaveTextContent("Following: Chorus, slide 3"), WAIT);
    expect(summary("folded")).toHaveTextContent("Following: Chorus, slide 3");
    expect(toggle("folded")).toHaveAttribute("aria-expanded", "false");
    expect(summary("held")).toHaveTextContent("Paused: someone changed the slide in ProPresenter");
    expect(summary("paused")).toHaveTextContent("Paused: ProPresenter did not take 5 slide changes in a row");
    expect(toggle("paused")).toHaveAttribute("aria-expanded", "true");
    expect(within(sample("paused")).getByRole("button", { name: "Try again" })).toBeInTheDocument();
    for (const id of ["off", "following", "held", "no-cues", "mismatch", "aligned"]) expect(within(sample(id)).queryByRole("button", { name: "Try again" }), id).toBeNull();
  });

  it("no cues, mismatch, aligned: the exact words, and the sentence that names the template", async () => {
    draw();
    await waitFor(() => expect(chips("no-cues")).toEqual(["No cues: following by bars", "No cues: following by bars", "Following by Playback cues", "No cues: following by bars"]), WAIT);
    expect(within(sample("no-cues")).queryAllByTestId("follow-song-no-cues")).toHaveLength(0);
    expect(within(sample("no-cues")).getAllByTestId("follow-song-chip")).toHaveLength(4);
    const fixes = within(sample("no-cues")).getAllByTestId("follow-song-fix").map((fix) => fix.textContent);
    expect(fixes[0]).toContain("Edit, the \u201C...\u201D on the song, MIDI OUT, Lyrics Bus, ProPresenter 1 Line Template, Update");
    expect(fixes[1]).toContain("Lyrics Bus, ProPresenter 2 Line Template, Update");
    expect(fixes[2]).toContain("Lyrics Bus, a ProPresenter Line Template that matches the file, Update");
    await waitFor(() => expect(chips("mismatch")).toEqual(["Cues do not match the file (cues go to slide 30, file has 54 cues)", "Arrangement was changed in ProPresenter", "Not in the ProPresenter playlist", "Following by Playback cues"]), WAIT);
    expect(within(sample("mismatch")).getByTestId("follow-song-fix")).toHaveTextContent("ProPresenter 2 Line Template");
    await waitFor(() => expect(chips("aligned")[1]).toBe("Following by Playback cues, aligned with extra slides (check the first slide of Verse 1)"), WAIT);
  });

  it("locked: visibly locked with ONE reason; a switch saved as on says Waiting for ... and claims nothing", async () => {
    draw();
    for (const [id, words, where] of [["locked-library", "Needs the Playback Library Link", "Configuration \u2192 Playback"], ["locked-link", "Needs the Presenter Link", "Configuration \u2192 Presentation"]] as const) {
      await waitFor(() => expect(summary(id)).toHaveTextContent(words), WAIT);
      expect(within(sample(id)).getByTestId("presenter-follow-reason")).toHaveTextContent(where);
      expect(box(id)).toBeDisabled();
      expect(within(sample(id)).getByTestId("auto-presenter")).toHaveAttribute("data-locked", "true");
    }
    // Right after an install the cause is the keychain prompt: the reason says so instead of sending the person to the setup.
    await waitFor(() => expect(within(sample("locked-keychain")).getByTestId("presenter-follow-reason")).toHaveTextContent("Allow the keychain prompt"), WAIT);
    expect(summary("locked-keychain")).toHaveTextContent("Needs the Playback Library Link");
    for (const id of ["locked-library", "locked-keychain", "locked-link"]) {
      expect(within(sample(id)).queryAllByTestId("follow-song-chip")).toHaveLength(0);
      expect(within(sample(id)).getAllByTestId("auto-presenter-song")).toHaveLength(4);
      expect(within(sample(id)).getByTestId("presenter-follow-locked")).toHaveTextContent("Locked");
    }
    await waitFor(() => expect(summary("waiting")).toHaveTextContent("Waiting for the Playback Library Link"), WAIT);
    expect(within(sample("waiting")).getByTestId("auto-presenter").textContent).not.toMatch(/Following/);
    expect(toggle("waiting")).toHaveAttribute("aria-expanded", "true");
  });

  it("timing, folded with an offset that is not 0: the offset and a quiet changed mark", async () => {
    draw();
    await waitFor(() => expect(within(sample("timing-changed")).getByTestId("presenter-follow-timing-changed")).toHaveTextContent("changed"), WAIT);
    expect(within(sample("timing-changed")).getByRole("button", { name: /^Timing/ })).toHaveTextContent("Offset 300 ms");
    expect(within(sample("following")).queryByTestId("presenter-follow-timing-changed")).toBeNull();
  });

  it("timing: folded until pressed; a number out of range is refused under the box", async () => {
    draw();
    await waitFor(() => expect(within(sample("timing")).getByRole("spinbutton", { name: "Offset (ms)" })).toHaveValue(300), WAIT);
    await waitFor(() => expect(within(sample("timing-error")).getByRole("alert")).toHaveTextContent("Enter a whole number from -2000 to 2000."), WAIT);
    expect(within(sample("following")).getByRole("button", { name: /^Timing/ })).toHaveAttribute("aria-expanded", "false");
  });
});
