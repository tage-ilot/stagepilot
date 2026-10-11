import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Badge } from "./Badge";
import { Button } from "./Button";
import { InlineAlert, type InlineAlertTone } from "./InlineAlert";
import { SegmentedControl } from "./SegmentedControl";
import { StatusDot } from "./StatusDot";
import type { Tone } from "./types";

/** The D3 tone table (p2_common). Accent = dot/border colour name, text = light text token. */
const table: Record<Tone, { accent: string; text: string }> = {
  services: { accent: "integration-services", text: "integration-services-text" },
  presentation: { accent: "integration-presentation", text: "integration-presentation-text" },
  playback: { accent: "integration-playback", text: "integration-playback-text" },
  lights: { accent: "integration-lights", text: "info-text" },
  success: { accent: "success", text: "success-text" },
  warning: { accent: "warning", text: "warning-text" },
  danger: { accent: "danger", text: "danger-text" },
  info: { accent: "info", text: "info-text" },
  idle: { accent: "idle", text: "ink-muted" },
};
const tones = Object.keys(table) as Tone[];
const classes = (el: Element) => el.className.split(/\s+/);
const textClasses = (el: Element) => classes(el).filter((c) => /^text-(?!shadow|type|left|center|right)/.test(c) && !c.includes(":"));

describe("tone table: every primitive uses the same accent and text tokens", () => {
  it.each(tones)("Button tinted %s", (tone) => {
    render(<Button variant="tinted" tone={tone}>x</Button>);
    const c = classes(screen.getByRole("button"));
    expect(c).toContain(`border-${table[tone].accent}/30`);
    expect(c).toContain(`bg-${table[tone].accent}/10`);
    expect(textClasses(screen.getByRole("button"))).toEqual([`text-${table[tone].text}`]);
  });

  it.each(tones)("Badge tinted %s", (tone) => {
    const { container } = render(<Badge tone={tone}>x</Badge>);
    const el = container.firstElementChild!;
    expect(classes(el)).toContain(`border-${table[tone].accent}/30`);
    expect(classes(el)).toContain(`bg-${table[tone].accent}/10`);
    expect(textClasses(el)).toEqual([`text-${table[tone].text}`]);
  });

  it.each(["warning", "danger", "success", "info"] as InlineAlertTone[])("InlineAlert %s", (tone) => {
    const { container } = render(<InlineAlert tone={tone}>x</InlineAlert>);
    const el = container.firstElementChild!;
    expect(classes(el)).toContain(`border-${table[tone].accent}/30`);
    expect(classes(el)).toContain(`bg-${table[tone].accent}/10`);
    expect(textClasses(el)).toEqual([`text-${table[tone].text}`]);
  });

  it.each(tones)("SegmentedControl selected %s", (tone) => {
    render(<SegmentedControl legend="L" name="n" value="a" onChange={() => {}} tone={tone} options={[{ value: "a", label: "Aaa" }, { value: "b", label: "Bbb" }]} />);
    const label = screen.getByText("Aaa").closest("label")!;
    const target = label.querySelector<HTMLElement>("[class*='border-']") ?? label;
    const holder = classes(target).includes(`border-${table[tone].accent}`) ? target : label;
    expect(classes(holder)).toContain(`border-${table[tone].accent}`);
    expect(classes(holder)).toContain(`bg-${table[tone].accent}/15`);
    expect(textClasses(holder)).toEqual([`text-${table[tone].text}`]);
  });

  it.each(tones)("StatusDot %s", (tone) => {
    const { container } = render(<StatusDot tone={tone} />);
    expect(classes(container.firstElementChild!)).toContain(`bg-${table[tone].accent}`);
  });

  it("special cases hold: lights text is info-text, idle text is ink-muted, others are *-text", () => {
    expect(table.lights.text).toBe("info-text");
    expect(table.idle.text).toBe("ink-muted");
    for (const t of tones.filter((x) => x !== "lights" && x !== "idle")) expect(table[t].text.endsWith("-text")).toBe(true);
  });
});
