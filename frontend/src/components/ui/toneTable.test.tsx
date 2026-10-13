import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

// @ts-expect-error node builtin without types
import { readFileSync } from "node:fs";
declare const process: { cwd: () => string };

import { Badge } from "./Badge";
import { Button } from "./Button";
import { InlineAlert, type InlineAlertTone } from "./InlineAlert";
import { StatusDot } from "./StatusDot";
import type { Tone } from "./types";

/** The D3 tone table (p2_common). Accent = dot/border colour name, text = light text token. */
const table: Record<Tone, { accent: string; text: string }> = {
  services: { accent: "integration-services-soft", text: "integration-services-text" },
  presentation: { accent: "integration-presentation-soft", text: "integration-presentation-text" },
  playback: { accent: "integration-playback-soft", text: "integration-playback-text" },
  lighting: { accent: "integration-lighting-soft", text: "integration-lighting-text" },
  backend: { accent: "integration-backend-soft", text: "integration-backend-text" },
  success: { accent: "success-soft", text: "success-text" },
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
    expect(c).toContain(`border-${table[tone].accent}/${tone === "success" ? 40 : 30}`);
    expect(c).toContain(`bg-${table[tone].accent}/10`);
    expect(textClasses(screen.getByRole("button"))).toEqual([`text-${table[tone].text}`]);
  });

  it.each(tones)("Badge tinted %s", (tone) => {
    const { container } = render(<Badge tone={tone}>x</Badge>);
    const el = container.firstElementChild!;
    expect(classes(el)).toContain(`border-${table[tone].accent}/${tone === "success" ? 40 : 30}`);
    expect(classes(el)).toContain(`bg-${table[tone].accent}/10`);
    expect(textClasses(el)).toEqual([`text-${table[tone].text}`]);
  });

  it.each(["warning", "danger", "success", "info"] as InlineAlertTone[])("InlineAlert %s", (tone) => {
    const { container } = render(<InlineAlert tone={tone}>x</InlineAlert>);
    const el = container.firstElementChild!;
    // The deep green reads weakly, so its outline is a touch stronger (40%) than the other tones (30%).
    expect(classes(el)).toContain(`border-${table[tone].accent}/${tone === "success" ? 40 : 30}`);
    expect(classes(el)).toContain(`bg-${table[tone].accent}/10`);
    expect(textClasses(el)).toEqual([`text-${table[tone].text}`]);
  });


  it.each(tones)("StatusDot %s", (tone) => {
    const { container } = render(<StatusDot tone={tone} />);
    expect(classes(container.firstElementChild!)).toContain(`bg-${table[tone].accent}`);
  });

  it("special cases hold: lights text is integration-lighting-text, idle text is ink-muted, others are *-text", () => {
    expect(table.lighting.text).toBe("integration-lighting-text");
    expect(table.idle.text).toBe("ink-muted");
    for (const t of tones.filter((x) => x !== "lighting" && x !== "idle")) expect(table[t].text.endsWith("-text")).toBe(true);
  });
});

describe("warning and error are the same rose (owner 2026-10-13)", () => {
  it("warning and danger share one accent and one text token", () => {
    expect(table.warning.accent).not.toBe(table.danger.accent); // different class names...
    // ...but the same colours: the tokens file maps both to the same hex.
    const css = readFileSync(`${process.cwd()}/src/design-tokens.css`, "utf8");
    const hex = (name: string) => css.match(new RegExp(`--ds-${name}: (#[0-9a-f]{6});`))?.[1];
    expect(hex("warning")).toBe(hex("danger"));
    expect(hex("warning-text")).toBe(hex("danger-text"));
  });

  it("the Previous control stays orange so it is not mistaken for the rose Stop controls", () => {
    const css = readFileSync(`${process.cwd()}/src/design-tokens.css`, "utf8");
    const hex = (name: string) => css.match(new RegExp(`--ds-${name}: (#[0-9a-f]{6});`))?.[1];
    expect(hex("live-back")).toBe("#e34e1f");
    expect(hex("live-back")).not.toBe(hex("warning"));
    expect(hex("live-back")).not.toBe(hex("live-stop"));
  });
});
