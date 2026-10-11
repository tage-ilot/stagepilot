import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StatusDot } from "./StatusDot";
import type { Tone } from "./types";

const tones: Tone[] = ["services", "presentation", "playback", "lights", "success", "warning", "danger", "info", "idle"];

describe("StatusDot", () => {
  it("is decorative by default", () => {
    const { container } = render(<StatusDot tone="success" />);
    const dot = container.firstElementChild!;
    expect(dot).toHaveAttribute("aria-hidden", "true");
    expect(dot).not.toHaveAttribute("role");
  });

  it("becomes an image with a label when labelled", () => {
    render(<StatusDot tone="danger" label="Error" />);
    const dot = screen.getByRole("img", { name: "Error" });
    expect(dot).not.toHaveAttribute("aria-hidden");
  });

  it("is 8px (sm, default) or 10px (md)", () => {
    const { container, rerender } = render(<StatusDot tone="info" />);
    expect(container.firstElementChild).toHaveClass("size-2");
    expect(container.firstElementChild).toHaveAttribute("data-size", "sm");
    rerender(<StatusDot tone="info" size="md" />);
    expect(container.firstElementChild).toHaveClass("size-2.5");
  });

  it.each(tones)("renders tone %s with a tone background and no animation", (tone) => {
    const { container } = render(<StatusDot tone={tone} />);
    const dot = container.firstElementChild!;
    expect(dot).toHaveAttribute("data-tone", tone);
    expect(dot.className).toMatch(/\bbg-[a-z-]+\b/);
    expect(dot.className).not.toMatch(/animate-|pulse/);
    expect(dot).toHaveClass("rounded-ds-full");
  });
});
