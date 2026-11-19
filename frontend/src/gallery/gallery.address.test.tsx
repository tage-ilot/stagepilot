import { render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { Frame } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

/** The address box in the gallery: every state, in all three Live Link blocks, and the real blocks' arrow lists. */
const sample = (id: string) => document.querySelector(`[data-sample="${id}"]`) as HTMLElement;
const ids = (group: string) => Array.from(document.querySelectorAll(`[data-group="${group}"] [data-sample]`)).map((node) => node.getAttribute("data-sample")?.replace(`${group}-`, ""));
const boxes = (root: HTMLElement) => within(root).getAllByRole("combobox");
const WAIT = { timeout: 10000 };

beforeEach(() => { installMadeUpApi(); window.localStorage.clear(); });

describe("the gallery: the address box", () => {
  it("lists every state the review asked for", () => {
    render(<Frame only={["template-address"]} />);
    expect(ids("template-address")).toEqual(["empty", "typed", "picked-recent", "both", "only-recent", "only-scanned", "none", "keyboard", "long", "error", "held", "disabled"]);
  });

  it("draws the same box in the three blocks: Lighting, Playback and ProPresenter", () => {
    render(<Frame only={["template-address"]} />);
    const empty = sample("template-address-empty");
    expect(boxes(empty).map((box) => box.getAttribute("aria-label"))).toEqual(["Lighting host or IP address", "Playback computer", "Host"]);
    for (const box of boxes(empty)) {
      expect(box.tagName).toBe("INPUT");
      expect(box).toHaveClass("font-mono", "!h-10");
      expect(box.parentElement?.querySelector("[data-part=address-arrow]")).not.toBeNull();
    }
    expect(within(empty).queryAllByRole("listbox")).toHaveLength(0);
  });

  it("shows the groups: Scanned and Recent, only Recent, none, with a check on the address in the box", () => {
    render(<Frame only={["template-address"]} />);
    const both = sample("template-address-both");
    expect(within(both).getAllByRole("group", { name: "Scanned" })).toHaveLength(3);
    expect(within(both).getAllByRole("group", { name: "Recent" })).toHaveLength(3);
    // 192.0.2.21 is both scanned and recent: once.
    expect(within(both).getAllByRole("option", { name: /192\.0\.2\.21/ })).toHaveLength(3);
    expect(both.querySelectorAll("[data-current]").length).toBe(3);
    const onlyRecent = sample("template-address-only-recent");
    expect(within(onlyRecent).queryAllByRole("group", { name: "Scanned" })).toHaveLength(0);
    expect(within(onlyRecent).getAllByRole("group", { name: "Recent" })).toHaveLength(3);
    expect(within(sample("template-address-none")).getAllByText("No recent or scanned addresses yet.")).toHaveLength(3);
    expect(within(sample("template-address-picked-recent")).getAllByRole("option").filter((option) => option.hasAttribute("data-current"))).toHaveLength(3);
  });

  it("a long list is as high as the list allows and scrolls", () => {
    render(<Frame only={["template-address"]} />);
    const long = sample("template-address-long");
    expect(within(long).getAllByRole("option")).toHaveLength(3 * 15);
    for (const list of Array.from(long.querySelectorAll("[data-part=address-list]"))) expect(list).toHaveClass("max-h-60", "overflow-y-auto");
  });

  it("reaches error, held change and disabled as the blocks do", () => {
    render(<Frame only={["template-address"]} />);
    for (const box of boxes(sample("template-address-error"))) expect(box).toBeInvalid();
    expect(within(sample("template-address-held")).getAllByText("Not saved yet")).toHaveLength(3);
    for (const box of boxes(sample("template-address-held"))) expect(box).toHaveClass("!border-warning");
    for (const box of boxes(sample("template-address-disabled"))) expect(box).toBeDisabled();
  });

  it("the keyboard state opens the list and moves to the second option", async () => {
    render(<Frame only={["template-address"]} />);
    const keyboard = sample("template-address-keyboard");
    await waitFor(() => expect(keyboard.querySelectorAll("[role=option][aria-selected=true]").length).toBe(3), WAIT);
    for (const box of boxes(keyboard)) expect(box).toHaveAttribute("aria-activedescendant");
  });

  it("each real block's arrow lists the made-up scan and what worked before, in its own state", async () => {
    render(<Frame only={["presentation", "playback", "lighting"]} />);
    for (const id of ["presentation-address-list", "playback-address-list", "lighting-address-list"]) {
      await waitFor(() => expect(within(sample(id)).getByRole("listbox")).toBeInTheDocument(), WAIT);
      const list = within(sample(id)).getByRole("listbox");
      expect(within(list).getByRole("group", { name: "Scanned" })).toBeInTheDocument();
      expect(within(list).getByRole("group", { name: "Recent" })).toBeInTheDocument();
    }
  }, 30000);
});
