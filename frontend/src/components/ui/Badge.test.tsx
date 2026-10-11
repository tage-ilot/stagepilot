import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Badge, StatusBadge } from "./Badge";
import { statusTone, type StatusWord } from "./statusWord";
import badgeSource from "./Badge.tsx?raw";

describe("Badge", () => {
  it("is tinted by default with tone fill, border and text colour", () => {
    render(<Badge tone="success">Ready</Badge>);
    const el = screen.getByText("Ready");
    expect(el).toHaveAttribute("data-emphasis", "tinted");
    expect(el).toHaveClass("bg-success/10", "border-success/30", "text-success-text", "text-shadow-label");
  });

  it("has the pill recipe", () => {
    render(<Badge tone="info">Info</Badge>);
    expect(screen.getByText("Info")).toHaveClass("rounded-ds-full", "uppercase", "font-bold", "tracking-wider", "px-3", "py-1.5", "text-type-badge");
  });

  it("solid success and warning use dark text and no shadow", () => {
    for (const tone of ["success", "warning"] as const) {
      const { unmount } = render(<Badge tone={tone} emphasis="solid">{tone}</Badge>);
      const el = screen.getByText(tone);
      expect(el).toHaveClass(`bg-${tone}`, "text-on-primary");
      expect(el).not.toHaveClass("text-shadow-label");
      unmount();
    }
  });

  it("solid danger uses danger-solid with light text and the label shadow", () => {
    render(<Badge tone="danger" emphasis="solid">Bad</Badge>);
    expect(screen.getByText("Bad")).toHaveClass("bg-danger-solid", "text-on-danger", "text-shadow-label");
  });

  it("falls back to tinted for tones with no solid recipe", () => {
    render(<Badge tone="info" emphasis="solid">Info</Badge>);
    expect(screen.getByText("Info")).toHaveAttribute("data-emphasis", "tinted");
  });

  it("renders an optional decorative leading dot", () => {
    const { container } = render(<Badge tone="warning" dot>Wait</Badge>);
    expect(container.querySelector("[aria-hidden='true'][data-tone='warning']")).not.toBeNull();
  });

  it("has no raw colours in its source", () => {
    expect(badgeSource).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(/i);
  });
});

describe("StatusBadge", () => {
  const expected: Record<StatusWord, string> = {
    Connected: "success",
    Connecting: "warning",
    "Not connected": "idle",
    Error: "danger",
  };
  it.each(Object.entries(expected))("maps %s to %s", (word, tone) => {
    expect(statusTone[word as StatusWord]).toBe(tone);
    render(<StatusBadge status={word as StatusWord} />);
    const el = screen.getByText(word);
    expect(el).toHaveAttribute("data-tone", tone);
    expect(el).toHaveClass("uppercase");
    expect(el.textContent).toBe(word);
  });
});
