import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { STATUS, type StatusKey } from "./addSongsStatus";
import { StatusChip, StatusRow, STATUS_ROW } from "./AddSongsStatus";

const row = (chip: StatusKey, extra: Partial<Parameters<typeof StatusRow>[0]> = {}) => (
  <ul><StatusRow chip={<StatusChip column {...STATUS[chip]} testId="chip" title="The long sentence." />} detail="The long sentence." testId="row" title="Holy Forever" {...extra} /></ul>
);

describe("the status chip", () => {
  it("is an icon and a word: the word is the status, the icon is hidden from a screen reader, the colour is only a tone", () => {
    render(<StatusChip {...STATUS.cant_import} testId="chip" title="Because." />);
    const chip = screen.getByTestId("chip");
    expect(chip).toHaveTextContent("!Can't import");
    expect(chip).toHaveAttribute("data-tone", "danger");
    expect(chip).toHaveAttribute("title", "Because.");
    expect(chip.firstElementChild).toHaveAttribute("aria-hidden", "true");
    expect(chip.className).toMatch(/border-danger/);
    // Tokens only: no raw colour.
    expect(chip.className).not.toMatch(/#|rgb|red-|green-/);
  });

  it("wears one tone per status: green, teal, red (what stops a song) and grey", () => {
    const tones = (Object.keys(STATUS) as StatusKey[]).map((key) => {
      const { unmount } = render(<StatusChip {...STATUS[key]} testId="chip" />);
      const tone = screen.getByTestId("chip").getAttribute("data-tone");
      unmount();
      return [key, tone];
    });
    expect(Object.fromEntries(tones)).toMatchObject({ in_playlist: "success", ready: "info", check: "info", cant_import: "danger", skipped: "idle" });
  });

  it("a running step is a spinner (still with its word), and says its state to a screen reader", () => {
    render(<StatusChip mark="\u2026" sr=": Working" spinner tone="info" word="Backup" />);
    expect(document.querySelector("[data-part=spinner]")).not.toBeNull();
    expect(document.querySelector("[data-part=spinner]")).toHaveAttribute("aria-hidden", "true");
    expect(document.querySelector("[data-part=spinner]")?.className).toContain("motion-reduce:animate-none");
    expect(screen.getByText(": Working")).toHaveClass("sr-only");
  });
});

describe("a status row: aligned columns on a wide screen, stacked on a narrow one", () => {
  it("is a three-column grid from 640 px (the song, a chip column of one width, the phrase) and one column below", () => {
    render(row("in_playlist"));
    const li = screen.getByTestId("row");
    expect(li.className).toBe(STATUS_ROW);
    expect(STATUS_ROW).toMatch(/\bgrid\b/);
    expect(STATUS_ROW).toMatch(/sm:grid-cols-\[minmax\(10rem,16rem\)_8\.5rem_minmax\(0,1fr\)\]/);
    // Narrow: no column template, so title, chip and phrase stack in that order.
    expect(STATUS_ROW.replace(/sm:\S+/g, "")).not.toMatch(/grid-cols/);
    // The chip fills its column on a wide screen (so they line up) and is as wide as its words on a narrow one.
    expect(screen.getByTestId("chip").className).toMatch(/\bw-fit\b/);
    expect(screen.getByTestId("chip").className).toMatch(/\bsm:w-full\b/);
  });

  it("keeps the order title, chip, phrase in the page (that is the stacked order at 360 px and the column order at 1280 px)", () => {
    render(row("new_copy", { phrase: "Playback order, \u201C(2)\u201D" }));
    const li = screen.getByTestId("row");
    const parts = Array.from(li.children).map((part) => part.getAttribute("data-testid") ?? part.tagName);
    expect(parts.slice(0, 3)).toEqual(["SPAN", "chip", "DIV"]);
    expect(within(li).getByTestId("add-songs-phrase")).toHaveTextContent("Playback order, \u201C(2)\u201D");
    expect(li.firstElementChild).toHaveClass("font-semibold", "text-ink");
  });

  it("draws no phrase at all when there is none, and the long sentence only for a screen reader", () => {
    render(row("in_playlist"));
    const li = screen.getByTestId("row");
    expect(within(li).queryByTestId("add-songs-phrase")).toBeNull();
    expect(within(li).getByTestId("add-songs-song-sentence")).toHaveClass("sr-only");
    expect(within(li).getByTestId("chip")).toHaveAttribute("title", "The long sentence.");
  });

  it("lists the files of a Pick one as small mono chips, one per line, with a name", () => {
    render(row("pick_one", { files: ["Songs / A (Live).pro", "Songs / A.pro"] }));
    const list = screen.getByRole("list", { name: "Files for Holy Forever" });
    const files = within(list).getAllByRole("listitem");
    expect(files.map((file) => file.textContent)).toEqual(["Songs / A (Live).pro", "Songs / A.pro"]);
    for (const file of files) expect(file).toHaveClass("font-mono", "border");
  });

  it("holds a song's extra parts (its steps, what to check) across the whole row", () => {
    render(row("ready", { extra: <p data-testid="more">More</p> }));
    expect(screen.getByTestId("more").parentElement).toHaveClass("sm:col-span-3");
  });
});
