import { fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { Frame } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The cover slot state: a song card always has a cover slot, the picture or the placeholder, and the three cards keep the same 28 px slot. */
const real = window.fetch;
beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });
afterEach(() => { window.fetch = real; });

describe("the cover slot in the gallery", () => {
  it("draws a picture, a placeholder and a failed picture (which becomes the placeholder) in the same slot", () => {
    render(<Frame only={["template-facts"]} />);
    const state = document.querySelector("[data-sample=\"template-facts-cover-slot\"]") as HTMLElement;
    const cards = Array.from(state.querySelectorAll("li"));
    expect(cards).toHaveLength(3);
    expect(cards[0]!.querySelector("img")).not.toBeNull();
    expect(cards[1]!.querySelector("[data-part=cover-placeholder]")).not.toBeNull();
    // jsdom does not load pictures: the broken address fails the way a 404, a 503 or a blocked address does in the app.
    fireEvent.error(cards[2]!.querySelector("img")!);
    expect(cards[2]!.querySelector("img")).toBeNull();
    expect(cards[2]!.querySelector("[data-part=cover-placeholder]")).not.toBeNull();
    for (const card of cards) expect(card.querySelectorAll("img, [data-part=cover-placeholder]")).toHaveLength(1);
  });
});
