import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { Frame } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** Song order rows: the shared row keeps its key and BPM chips by default; the Song order section hides them and keeps the length. */
const real = window.fetch;
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });
const sample = (id: string) => document.querySelector(`[data-sample="template-facts-${id}"]`) as HTMLElement;

describe("the Song order rows in the gallery", () => {
  it("the shared row still has its key and BPM chips; the Song order version has neither, and both keep the length and the stamp", () => {
    render(<Frame only={["template-facts"]} />);
    const withChips = sample("ledger");
    const songOrder = sample("song-order");
    expect(withChips.querySelector("[data-part=facts][aria-label^='Key']")).not.toBeNull();
    expect(withChips.textContent).toMatch(/BPM/);
    expect(songOrder.querySelector("ol")!.textContent).not.toMatch(/BPM/);
    expect(songOrder.querySelector("[data-part=facts]")).toBeNull();
    expect(songOrder.querySelector("[data-part=facts][aria-label^='Key']")).toBeNull();
    expect(songOrder.querySelectorAll(".arr-line").length).toBe(withChips.querySelectorAll(".arr-line").length);
    expect(songOrder.querySelector("ol")!.textContent).toMatch(/\d+:\d{2}/);
    expect(songOrder.querySelectorAll("[data-stamp]").length).toBeGreaterThan(0);
  });
});
