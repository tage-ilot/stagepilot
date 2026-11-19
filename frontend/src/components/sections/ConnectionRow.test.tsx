import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ConnectionRow } from "./ConnectionRow";

describe("ConnectionRow: the one line of address, port and main buttons", () => {
  it("is one wrapping row with its bottoms aligned and a little room above it", () => {
    render(<ConnectionRow testId="row"><input aria-label="Host" /><button type="button">Test connection</button></ConnectionRow>);
    const row = screen.getByTestId("row");
    expect(row.tagName).toBe("DIV");
    expect(row).toHaveClass("mt-2", "flex", "flex-wrap", "items-end", "gap-x-2.5", "gap-y-2");
    expect(row).toHaveAttribute("data-part", "connection-row");
    expect(Array.from(row.children).map((child) => child.tagName)).toEqual(["INPUT", "BUTTON"]);
  });

  it("can be a form, so Enter in the address field presses Connect", () => {
    const onSubmit = vi.fn((event: React.FormEvent) => event.preventDefault());
    render(<ConnectionRow as="form" id="specific" onSubmit={onSubmit} testId="row"><input aria-label="Host" /></ConnectionRow>);
    const row = screen.getByTestId("row");
    expect(row.tagName).toBe("FORM");
    expect(row).toHaveAttribute("id", "specific");
    fireEvent.submit(row);
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("a row inside another row takes no space above it, and keeps the page's own class", () => {
    render(<ConnectionRow className="live-link-modes" nested testId="row">x</ConnectionRow>);
    expect(screen.getByTestId("row")).not.toHaveClass("mt-2");
    expect(screen.getByTestId("row")).toHaveClass("live-link-modes", "flex", "flex-wrap");
  });
});
