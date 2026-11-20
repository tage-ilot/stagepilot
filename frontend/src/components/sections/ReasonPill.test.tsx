import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ReasonPill } from "./index";

describe("ReasonPill", () => {
  it("is a small neutral pill: an icon that is not read, then the words", () => {
    render(<ReasonPill id="why" testId="pill">Choose a playlist</ReasonPill>);
    const pill = screen.getByTestId("pill");
    expect(pill).toHaveAttribute("id", "why");
    expect(pill).toHaveTextContent("Choose a playlist");
    expect(pill).toHaveClass("inline-flex", "items-center", "rounded-ds-full", "border", "border-edge", "bg-surface-950");
    expect(pill.className).not.toMatch(/danger|warning|red/);
    expect(pill.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    // Not a heading and not a role that is announced by itself: the button it sits beside points at it.
    expect(pill).not.toHaveAttribute("role");
  });

  it("wraps its words instead of widening the row on a phone", () => {
    render(<ReasonPill testId="pill">A long reason that must wrap on a narrow screen</ReasonPill>);
    expect(screen.getByTestId("pill")).toHaveClass("min-w-0", "max-w-full");
  });

  it("draws nothing without words", () => {
    const { container } = render(<ReasonPill>{null}</ReasonPill>);
    expect(container).toBeEmptyDOMElement();
  });
});
