import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { Frame } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The five states of pairing a song by hand (and the chooser for a song with no match), drawn from the real Song order ledger. */
const real = window.fetch;
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });
const sample = (id: string) => document.querySelector(`[data-sample="song-pairs-${id}"]`) as HTMLElement;

describe("the pair-by-hand states in the gallery", () => {
  it("shows an alike song and a song with no match with the plain action, and nothing is pre-picked or paired", async () => {
    render(<Frame only={["song-pairs"]} />);
    for (const id of ["uncertain", "no-match"]) {
      const state = sample(id);
      expect(state.textContent).toMatch(/Pick the Playback song/);
      expect(state.querySelector("[data-testid=pair-chooser]")).toBeNull();
    }
    expect(sample("uncertain").querySelectorAll(".arr-line")[1]!.querySelector("[data-stamp]")!.textContent).toMatch(/Check title/);
    expect(sample("no-match").textContent).toMatch(/Only in plan/);
  });

  it("opens the chooser by pressing the action: radios with the alike song first, Confirm off until one is picked", async () => {
    render(<Frame only={["song-pairs"]} />);
    await new Promise((resolve) => setTimeout(resolve, 120));
    const chooser = sample("chooser-open").querySelector("[data-testid=pair-chooser]")!;
    expect(chooser).not.toBeNull();
    const labels = Array.from(chooser.querySelectorAll("label")).map((label) => label.textContent);
    expect(labels[0]).toMatch(/Battle Belongs.*Looks alike/);
    expect(labels).toHaveLength(3);
    expect(Array.from(chooser.querySelectorAll("button")).map((b) => [b.textContent, b.disabled])).toEqual([["Confirm", true], ["Cancel", false]]);
    expect(sample("chooser-open-no-match").querySelectorAll("[data-testid=pair-chooser] label")).toHaveLength(3);
    expect(sample("chooser-open-no-match").textContent).not.toMatch(/Looks alike/);
  });

  it("shows a pair as Paired by you with Undo, and a song with no candidates as plain words with no button", () => {
    render(<Frame only={["song-pairs"]} />);
    const paired = sample("paired");
    expect(paired.querySelectorAll(".arr-line")[1]!.querySelector("[data-stamp]")!.textContent).toMatch(/Paired by you/);
    expect(paired.querySelector("[data-testid=pair-by-hand] button")!.textContent).toBe("Undo");
    const none = sample("no-candidates");
    expect(none.querySelector("[data-testid=pair-no-choices]")!.textContent).toBe("Playback has no other song to pair");
    expect(none.textContent).not.toMatch(/Pick the Playback song/);
  });
});
