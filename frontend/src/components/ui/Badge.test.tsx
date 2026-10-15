import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AccentContext } from "./accent";
import { Badge, StatusBadge } from "./Badge";
import { statusTone, type StatusWord } from "./statusWord";
import badgeSource from "./Badge.tsx?raw";

describe("Badge", () => {
  it("is tinted by default with tone fill, border and text colour", () => {
    render(<Badge tone="success">Ready</Badge>);
    const el = screen.getByText("Ready");
    expect(el).toHaveAttribute("data-emphasis", "tinted");
    expect(el).toHaveClass("bg-success-soft/10", "border-success-soft/40", "text-success-text", "text-shadow-label");
  });

  it("has the pill recipe", () => {
    render(<Badge tone="info">Info</Badge>);
    expect(screen.getByText("Info")).toHaveClass("rounded-ds-full", "uppercase", "font-bold", "tracking-wider", "px-3", "py-1.5", "text-type-badge");
  });

  it("solid warning uses dark text and no shadow; solid success is the deep green with light text", () => {
    const warn = render(<Badge tone="warning" emphasis="solid">x</Badge>).container.firstElementChild!;
    expect(warn).toHaveClass("bg-warning", "text-on-light");
    expect(warn.className).not.toContain("text-shadow");
    const ok = render(<Badge tone="success" emphasis="solid">y</Badge>).container.firstElementChild!;
    expect(ok).toHaveClass("bg-success", "text-ink");
    expect(ok.className).not.toContain("text-on-light");
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
    "Disconnected": "idle",
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

  it("is set in the bold Geist Pixel Square, whatever the tone", () => {
    render(<><StatusBadge status="Connected" /><StatusBadge status="Error" /></>);
    for (const word of ["Connected", "Error"]) expect(screen.getByText(word)).toHaveClass("font-pixel");
  });

  it("keeps its own status colours inside a connection panel, while an ordinary success badge takes the connection's accent", () => {
    render(
      <AccentContext.Provider value="playback">
        <StatusBadge status="Connected" />
        <StatusBadge status="Error" />
        <Badge tone="success">Saved</Badge>
      </AccentContext.Provider>,
    );
    const connected = screen.getByText("Connected");
    expect(connected).toHaveAttribute("data-tone", "success");
    expect(connected).toHaveClass("text-success-text");
    expect(connected.querySelector("[data-tone]")).toHaveAttribute("data-tone", "success");
    expect(screen.getByText("Error")).toHaveAttribute("data-tone", "danger");
    expect(screen.getByText("Saved")).toHaveAttribute("data-tone", "playback");
  });
});
