import { describe, expect, it } from "vitest";
// @ts-expect-error generated JS config has no declarations
import tailwind from "../tailwind.config.js";

declare const process: { cwd(): string };
const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
const { join } = (await import("node:path" as string)) as { join(...parts: string[]): string };
const tokensCss = readFileSync(join(process.cwd(), "src/design-tokens.css"), "utf8");
const colors = (tailwind as { theme: { extend: { colors: Record<string, string> } } }).theme.extend.colors;

describe("connection palette (DESIGN.md Connection colours)", () => {
  it.each([
    ["services", "#6bb03c", "#6bb03c", "#72b446"],
    ["presentation", "#fe7821", "#fe7821", "#fe802e"],
    ["playback", "#56677b", "#56677b", "#8f9aa8"],
    ["lighting", "#d53367", "#d53367", "#e687a5"],
    ["backend", "#c7231d", "#c7231d", "#e28d89"],
  ])("%s has base, -soft and -text tokens", (name, base, soft, text) => {
    expect(colors[`integration-${name}`]).toBe(base);
    expect(colors[`integration-${name}-soft`]).toBe(soft);
    expect(colors[`integration-${name}-text`]).toBe(text);
    expect(colors[`advanced-${name}`]).toBeTruthy();
    expect(tokensCss).toContain(`--ds-integration-${name}-soft: ${soft};`);
  });

  it("no old lights-named colour tokens remain", () => {
    expect(Object.keys(colors).filter((k) => /lights/.test(k))).toEqual([]);
  });

  it("Playback base and soft are the same colour (no separate lifted copy)", () => {
    expect(colors["integration-playback"]).toBe(colors["integration-playback-soft"]);
  });
});
