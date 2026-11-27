import { render, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { Frame } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The Follow Playback states of the section gallery: made-up answers, no backend; the switch really switches. */
const real = window.fetch;
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });
const WAIT = { timeout: 60000 };
const sample = (id: string) => document.querySelector(`[data-sample="presenter-follow-${id}"]`) as HTMLElement;
const draw = () => render(<Frame only={["presenter-follow"]} />);
const status = (id: string) => within(sample(id)).getByTestId("presenter-follow-status");
const chips = (id: string) => within(sample(id)).getAllByTestId("follow-song-chip").map((chip) => chip.textContent?.replace(/^\S\s*/, "") ?? "");

describe("the Follow Playback states", { timeout: 120000 }, () => {
  it("lists every state, as the task names them", () => {
    draw();
    const ids = Array.from(document.querySelectorAll('[data-group="presenter-follow"] [data-sample]')).map((node) => node.getAttribute("data-sample")?.replace("presenter-follow-", ""));
    expect(ids).toEqual(["off", "following", "held", "paused", "no-cues", "mismatch", "not-followed", "locked-link", "locked-library", "timing", "timing-error"]);
  });

  it("switch off: Off, every card says the switch is off, nothing to press but the switch", async () => {
    draw();
    await waitFor(() => expect(chips("off")).toEqual(Array(4).fill("Switch is off")), WAIT);
    expect(status("off")).toHaveTextContent("Off");
    expect(within(sample("off")).getByRole("switch", { name: "Follow Playback in ProPresenter" })).not.toBeChecked();
    expect(within(sample("off")).queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("pressing the switch in the gallery turns it on, like the real one", async () => {
    draw();
    const box = await waitFor(() => within(sample("off")).getByRole("switch", { name: "Follow Playback in ProPresenter" }), WAIT);
    await userEvent.setup().click(box);
    await waitFor(() => expect(status("off")).toHaveTextContent("Following: Chorus, slide 3"), WAIT);
  });

  it("following, held, paused: the words of each, and Try again only after failures", async () => {
    draw();
    await waitFor(() => expect(status("following")).toHaveTextContent("Following: Chorus, slide 3"), WAIT);
    expect(status("held")).toHaveTextContent("Paused: someone changed the slide in ProPresenter");
    expect(status("paused")).toHaveTextContent("Paused: ProPresenter did not take 5 slide changes in a row");
    expect(within(sample("paused")).getByRole("button", { name: "Try again" })).toBeInTheDocument();
    for (const id of ["off", "following", "held", "no-cues", "mismatch"]) expect(within(sample(id)).queryByRole("button", { name: "Try again" }), id).toBeNull();
  });

  it("no cues and mismatch: the exact words, and the one sentence for the fix", async () => {
    draw();
    await waitFor(() => expect(chips("no-cues")).toEqual(["Following by bars (no Playback cues)", "Following by bars (no Playback cues)", "Following by Playback cues", "Following by bars (no Playback cues)"]), WAIT);
    expect(within(sample("no-cues")).getAllByTestId("follow-song-no-cues")).toHaveLength(3);
    expect(within(sample("no-cues")).getAllByTestId("follow-song-fix")[0]).toHaveTextContent("Edit, the \u201C...\u201D on the song, MIDI OUT, Lyrics Bus, ProPresenter 1 Line Template, Update");
    await waitFor(() => expect(chips("mismatch")).toEqual(["Cues do not match the file (cues go to slide 30, file has 54 cues)", "Arrangement was changed in ProPresenter", "Not in the ProPresenter playlist", "Following by Playback cues"]), WAIT);
  });

  it("locked: greyed, not hidden, with the reason", async () => {
    draw();
    for (const [id, reason] of [["locked-link", "Presenter Link not ready"], ["locked-library", "Playback's Library Link not ready"]] as const) {
      await waitFor(() => expect(within(sample(id)).getByTestId("presenter-follow-reason")).toHaveTextContent(reason), WAIT);
      expect(within(sample(id)).getByRole("switch", { name: "Follow Playback in ProPresenter" })).toBeDisabled();
      expect(within(sample(id)).getByTestId("presenter-follow")).toHaveAttribute("data-locked", "true");
    }
  });

  it("timing: folded until pressed; a number out of range is refused under the box", async () => {
    draw();
    await waitFor(() => expect(within(sample("timing")).getByRole("spinbutton", { name: "Offset (ms)" })).toHaveValue(300), WAIT);
    await waitFor(() => expect(within(sample("timing-error")).getByRole("alert")).toHaveTextContent("Enter a whole number from -2000 to 2000."), WAIT);
    expect(within(sample("following")).getByRole("button", { name: /^Timing/ })).toHaveAttribute("aria-expanded", "false");
  });
});
