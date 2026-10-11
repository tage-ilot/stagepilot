import { createRef } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Button, type ButtonVariant } from "./Button";
import type { LiveTone, Tone } from "./types";

const variants: ButtonVariant[] = ["primary", "secondary", "tinted", "destructive", "destructive-soft", "live"];
const tones: Tone[] = ["services", "presentation", "playback", "lights", "success", "warning", "danger", "info", "idle"];
const liveTones: LiveTone[] = ["go", "back", "reload", "stop"];
const stock = /\b(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3}\b|#[0-9a-f]{3,8}\b/i;
const shadow = (el: HTMLElement) => el.className.split(/\s+/).includes("text-shadow-label");

describe("Button", () => {
  it.each(variants)("renders data-variant=%s", (variant) => {
    render(<Button variant={variant} liveTone="go">Go</Button>);
    expect(screen.getByRole("button")).toHaveAttribute("data-variant", variant);
  });

  it("defaults to secondary, type=button, control size", () => {
    render(<Button>Save</Button>);
    const b = screen.getByRole("button");
    expect(b).toHaveAttribute("data-variant", "secondary");
    expect(b).toHaveAttribute("type", "button");
    expect(b).toHaveAttribute("data-size", "control");
  });

  it("allows type=submit", () => {
    render(<Button type="submit">Save</Button>);
    expect(screen.getByRole("button")).toHaveAttribute("type", "submit");
  });

  it("calls the click handler", async () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Save</Button>);
    await userEvent.click(screen.getByRole("button"));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("blocks clicks and dims when disabled", async () => {
    const onClick = vi.fn();
    render(<Button disabled onClick={onClick}>Save</Button>);
    const b = screen.getByRole("button");
    await userEvent.click(b);
    expect(onClick).not.toHaveBeenCalled();
    expect(b).toBeDisabled();
    expect(b).toHaveAttribute("aria-disabled", "true");
    expect(b).toHaveClass("opacity-40", "cursor-not-allowed");
  });

  it("blocks clicks while loading, is aria-busy, not dimmed, swaps the label", async () => {
    const onClick = vi.fn();
    render(<Button loading loadingLabel="Saving…" onClick={onClick}>Save</Button>);
    const b = screen.getByRole("button", { name: "Saving…" });
    await userEvent.click(b);
    await userEvent.keyboard("{Enter}");
    expect(onClick).not.toHaveBeenCalled();
    expect(b).toHaveAttribute("aria-busy", "true");
    expect(b).toHaveAttribute("aria-disabled", "true");
    expect(b).toHaveAttribute("data-loading", "true");
    expect(b).not.toHaveClass("opacity-40");
  });

  it("keeps the label when loading without loadingLabel", () => {
    render(<Button loading>Save</Button>);
    expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument();
  });

  it("label shadow: present for secondary, tinted, destructive, live; absent for primary and destructive-soft", () => {
    for (const variant of ["secondary", "tinted", "destructive"] as const) {
      const { unmount } = render(<Button variant={variant}>x</Button>);
      expect(shadow(screen.getByRole("button"))).toBe(true);
      unmount();
    }
    for (const variant of ["primary", "destructive-soft"] as const) {
      const { unmount } = render(<Button variant={variant}>x</Button>);
      expect(shadow(screen.getByRole("button"))).toBe(false);
      unmount();
    }
  });

  it("live: light text keeps the shadow; dark hover text (go, back) removes it; reload and stop keep it", () => {
    for (const t of liveTones) {
      const { unmount } = render(<Button variant="live" liveTone={t}>x</Button>);
      const b = screen.getByRole("button");
      expect(shadow(b)).toBe(true);
      const dark = t === "go" || t === "back";
      expect(b.className.includes("can-hover:[text-shadow:none]")).toBe(dark);
      expect(b.className.includes("can-hover:text-on-primary")).toBe(dark);
      expect(b.className.includes("can-hover:text-on-danger")).toBe(!dark);
      unmount();
    }
  });

  it("44px: coarse on every size, touch always, live always", () => {
    const { rerender } = render(<Button>x</Button>);
    expect(screen.getByRole("button")).toHaveClass("min-h-ds-control", "coarse:min-h-ds-touch-target");
    rerender(<Button size="touch">x</Button>);
    expect(screen.getByRole("button")).toHaveClass("min-h-ds-touch-target");
    rerender(<Button variant="live" liveTone="stop">x</Button>);
    expect(screen.getByRole("button")).toHaveClass("min-h-ds-touch-target");
    expect(screen.getByRole("button")).toHaveAttribute("data-size", "touch");
  });

  it("shared classes: touch-manipulation, 140ms colour transition, reduced motion", () => {
    render(<Button>x</Button>);
    expect(screen.getByRole("button")).toHaveClass("touch-manipulation", "transition-colors", "motion-reduce:transition-none", "rounded-ds-lg");
  });

  it("hover styling is can-hover scoped and dropped when inert", () => {
    const { rerender } = render(<Button variant="primary">x</Button>);
    expect(screen.getByRole("button").className).toContain("can-hover:bg-primary-hover");
    expect(screen.getByRole("button").className).not.toMatch(/(^|\s)hover:/);
    rerender(<Button variant="primary" disabled>x</Button>);
    expect(screen.getByRole("button").className).not.toContain("can-hover:");
  });

  it("forwards ref and appends className last", () => {
    const ref = createRef<HTMLButtonElement>();
    render(<Button ref={ref} className="extra-last">x</Button>);
    expect(ref.current).toBe(screen.getByRole("button"));
    expect(screen.getByRole("button").className.endsWith("extra-last")).toBe(true);
  });

  it("fullWidth and native props pass through", () => {
    render(<Button fullWidth aria-describedby="why" aria-label="Close">x</Button>);
    const b = screen.getByRole("button", { name: "Close" });
    expect(b).toHaveClass("w-full");
    expect(b).toHaveAttribute("aria-describedby", "why");
  });

  it.each(tones)("tinted tone %s uses tokens only", (tone) => {
    render(<Button variant="tinted" tone={tone}>x</Button>);
    const b = screen.getByRole("button");
    expect(b).toHaveAttribute("data-tone", tone);
    expect(b.className).not.toMatch(stock);
    expect(b.className).toMatch(/bg-[a-z-]+\/10/);
  });

  it("tinted defaults to info", () => {
    render(<Button variant="tinted">x</Button>);
    expect(screen.getByRole("button")).toHaveAttribute("data-tone", "info");
  });

  it.each(liveTones)("live tone %s uses tokens only", (t) => {
    render(<Button variant="live" liveTone={t}>x</Button>);
    const b = screen.getByRole("button");
    expect(b).toHaveAttribute("data-tone", t);
    expect(b.className).not.toMatch(stock);
    expect(b.className).toContain(`border-live-${t}/`);
    expect(b.className).toContain(`text-live-${t}-text`);
  });
});
