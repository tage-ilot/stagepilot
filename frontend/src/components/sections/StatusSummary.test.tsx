import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Panel } from "../ui/Panel";
import { readIndexCss } from "./readIndexCss";
import { StatusSummary, type SummaryItem } from "./StatusSummary";

const items: SummaryItem[] = [
  { label: "API", value: "Connected" },
  { label: "Configured timer", value: "Song Countdown", tone: "ok" },
  { label: "Last checked", value: "Not checked", tabular: true },
];

describe("StatusSummary", () => {
  it("is a dl of label and words, one group per fact, in the order given", () => {
    render(<StatusSummary items={items} testId="summary" />);
    const list = screen.getByTestId("summary");
    expect(list.tagName).toBe("DL");
    expect(Array.from(list.querySelectorAll("dt")).map((dt) => dt.textContent)).toEqual(["API", "Configured timer", "Last checked"]);
    expect(Array.from(list.querySelectorAll("dd")).map((dd) => dd.textContent)).toEqual(["Connected", "Song Countdown", "Not checked"]);
    // Each fact is a div holding one dt and its dd (the structure `dl` allows).
    expect(Array.from(list.children).every((child) => child.tagName === "DIV" && child.querySelectorAll("dt").length === 1)).toBe(true);
  });

  it("is a list under a rule by default, and says how many facts it has so the grid can fit them", () => {
    render(<StatusSummary items={items} testId="summary" />);
    const list = screen.getByTestId("summary");
    expect(list).toHaveAttribute("data-variant", "list");
    expect(list).toHaveAttribute("data-count", "3");
    expect(list).toHaveClass("section-summary", "grid", "border-t", "border-edge-subtle", "pt-4");
    expect(list.parentElement).toHaveClass("section-summary-wrap");
    expect(list.parentElement).toHaveAttribute("data-section", "summary");
  });

  it("the cards variant draws each fact in its own recessed box, with its second line", () => {
    const { container } = render(<StatusSummary items={[{ label: "WebSocket", value: "Connected", tone: "ok", detail: "ws://x/ws" }, { label: "API endpoint", value: "http://x", mono: true }]} label="Status" variant="cards" />);
    const list = container.querySelector("dl[aria-label=Status]") as HTMLElement;
    expect(list).toHaveAttribute("data-variant", "cards");
    expect(list).not.toHaveClass("border-t");
    const [first, second] = Array.from(list.children) as [HTMLElement, HTMLElement];
    expect(first).toHaveClass("rounded-ds-lg", "border");
    expect(within(first).getAllByText(/Connected|ws:\/\/x\/ws/)).toHaveLength(2);
    expect(within(second).getByText("http://x")).toHaveClass("font-mono", "break-all");
  });

  it("a tone is words AND a colour: ok wears the connection's colour, warn and danger the state colours", () => {
    render(
      <Panel tone="presentation">
        <StatusSummary items={[{ label: "a", value: "Fine", tone: "ok" }, { label: "b", value: "Not found", tone: "warn" }, { label: "c", value: "Down", tone: "danger" }, { label: "d", value: "Plain" }]} />
      </Panel>,
    );
    expect(screen.getByText("Fine")).toHaveClass("text-integration-presentation-text");
    expect(screen.getByText("Not found")).toHaveClass("text-warning-text");
    expect(screen.getByText("Down")).toHaveClass("text-danger-text");
    expect(screen.getByText("Plain")).toHaveClass("text-ink-body");
    expect(screen.getByText("Not found")).toHaveAttribute("data-tone", "warn");
  });

  it("outside a connection's panel ok is the success colour", () => {
    render(<StatusSummary items={[{ label: "a", value: "Fine", tone: "ok" }]} />);
    expect(screen.getByText("Fine")).toHaveClass("text-success-text");
  });

  it("has its breakpoints in index.css: the list 1, 2 or 4 (and 3 or 2 for fewer facts), the cards 1, 2 or 4", async () => {
    const css = await readIndexCss();
    expect(css).toMatch(/@container \(min-width: 23rem\) \{\s*\.section-summary\[data-variant="list"\] \{\s*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\);/);
    expect(css).toMatch(/@container \(min-width: 30rem\) \{\s*\.section-summary\[data-variant="list"\] \{\s*grid-template-columns: repeat\(4, minmax\(max-content, 1fr\)\);/);
    expect(css).toMatch(/\[data-count="3"\] \{\s*grid-template-columns: repeat\(3, minmax\(max-content, 1fr\)\);/);
    expect(css).toMatch(/@container \(min-width: 24rem\) \{\s*\.section-summary\[data-variant="cards"\]/);
    expect(css).toMatch(/@container \(min-width: 52rem\) \{\s*\.section-summary\[data-variant="cards"\]/);
  });
});
