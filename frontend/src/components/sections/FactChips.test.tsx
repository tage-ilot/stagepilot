import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { FactChips, LONG_FACT, type Fact } from "./FactChips";

const key: Fact = { text: "Ab", name: "Key Ab" };
const length: Fact = { text: "4:37", name: "length 4:37" };
const items = () => within(screen.getByRole("list")).getAllByRole("listitem");

describe("FactChips", () => {
  it("draws ONE square chip per fact, in the order given", () => {
    render(<FactChips facts={[key, length, { text: "65 BPM" }]} />);
    expect(items().map((chip) => chip.textContent)).toEqual(["Ab", "4:37", "65 BPM"]);
  });

  it("is the one chip look: square, an outline, the recessed fill, mono tabular words, no rounded corners", () => {
    render(<FactChips facts={[key]} />);
    const chip = items()[0]!;
    for (const token of ["inline-flex", "items-center", "border", "border-edge-medium", "bg-surface-950", "px-1.5", "py-0.5", "font-mono", "text-[0.8125rem]", "tabular-nums", "text-ink"]) expect(chip).toHaveClass(token);
    expect(chip.className).not.toMatch(/\brounded/);
  });

  it("wraps with a small gap, so a narrow card puts the rest on a second row and a wide one keeps one row", () => {
    render(<FactChips facts={[key, length]} />);
    expect(screen.getByRole("list")).toHaveClass("flex", "flex-wrap", "gap-1", "min-w-0");
  });

  it("has no separator character anywhere in what it draws: the box is the separator", () => {
    render(<FactChips facts={[key, length, { text: "65 BPM" }, { text: "mtID-23126" }]} testId="facts" />);
    // The only hyphen is the one inside the chip's own words (`mtID-23126`, Hunter's format); between chips there is nothing.
    expect((screen.getByTestId("facts").textContent ?? "").replace("mtID-", "mtID")).not.toMatch(/[\u00B7\u2022|,;/\-\u2013\u2014]/);
    expect(screen.getByTestId("facts").textContent).toBe("Ab 4:37 65 BPM mtID-23126");
  });

  it("is a list of list items (on spans, so it is also valid inside a button that is a whole card), named by the facts' own plain words", () => {
    render(<FactChips facts={[key, length]} />);
    const list = screen.getByRole("list", { name: "Key Ab, length 4:37" });
    expect(list.tagName).toBe("SPAN");
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
  });

  it("names a fact by its words when it has no other name, and the list by `label` when given", () => {
    const { rerender } = render(<FactChips facts={[{ text: "65 BPM" }, { text: "mtID-23126" }]} />);
    expect(screen.getByRole("list", { name: "65 BPM, mtID-23126" })).toBeInTheDocument();
    rerender(<FactChips facts={[{ text: "65 BPM" }]} label="Facts of Holy Forever" />);
    expect(screen.getByRole("list", { name: "Facts of Holy Forever" })).toBeInTheDocument();
  });

  it("is neutral unless a fact carries a tone, and the tone changes only the outline and the words' colour", () => {
    render(<FactChips facts={[{ text: "2 looks" }, { text: "Not captured", tone: "warning" }, { text: "Failed", tone: "danger" }, { text: "Done", tone: "success" }]} />);
    const [plain, warning, danger, success] = items();
    expect(plain).toHaveAttribute("data-tone", "neutral");
    expect(plain).toHaveClass("border-edge-medium", "text-ink");
    expect(warning).toHaveAttribute("data-tone", "warning");
    expect(warning).toHaveClass("border-warning/60", "text-warning-text");
    expect(danger).toHaveClass("border-danger/60", "text-danger-text");
    expect(success).toHaveClass("border-success-soft", "text-success-text");
    for (const chip of [plain, warning, danger, success]) expect(chip).toHaveClass("bg-surface-950", "font-mono");
  });

  it("draws nothing for no facts", () => {
    const { container } = render(<FactChips facts={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("cuts a long fact inside its own chip and keeps all its words as the hover text; a short one has none unless it asks", () => {
    const long = "Ab (was G) with a very long extra note";
    expect(long.length).toBeGreaterThan(LONG_FACT);
    render(<FactChips facts={[{ text: long }, { text: "4:37" }, { text: "65 BPM", title: "Playback: 65 BPM" }]} />);
    const [first, second, third] = items();
    expect(first).toHaveAttribute("title", long);
    expect(first).toHaveClass("min-w-0", "max-w-full");
    expect(first!.firstElementChild).toHaveClass("truncate", "min-w-0");
    expect(second).not.toHaveAttribute("title");
    expect(third).toHaveAttribute("title", "Playback: 65 BPM");
  });

  it("passes the test id of a fact to its chip, and keeps a fact's own content (a number with its own hook)", () => {
    render(<FactChips facts={[{ text: "G", testId: "key" }, { text: "mtID-7", content: <>mtID-<span data-testid="mtid">7</span></> }]} />);
    expect(screen.getByTestId("key")).toHaveTextContent("G");
    expect(screen.getByTestId("mtid")).toHaveTextContent("7");
    expect(items()[1]).toHaveTextContent("mtID-7");
  });
});
