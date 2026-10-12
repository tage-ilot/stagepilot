import { describe, expect, it } from "vitest";
import badgeSource from "./components/ui/Badge.tsx?raw";
import buttonSource from "./components/ui/Button.tsx?raw";

declare const process: { cwd(): string };
const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
const { join } = (await import("node:path" as string)) as { join(...parts: string[]): string };

describe("secondary accent teal guard", () => {
  const config = readFileSync(join(process.cwd(), "tailwind.config.js"), "utf8");
  const color = (name: string) => new RegExp(`"${name}": "(#[0-9a-f]{6})"`, "i").exec(config)?.[1];

  it("generated tokens carry the accent set", () => {
    expect(color("info")).toBe("#1dc1c7");
    expect(color("info-text")).toBe("#77dadd");
    expect(color("info-hover")).toBe("#34c7cd");
    expect(color("info-active")).toBe("#1aaeb3");
    expect(color("on-info")).toBe("#020617");
    expect(color("live-reload")).toBe("#1dc1c7");
    expect(color("live-reload-text")).toBe("#77dadd");
  });

  it("Lighting uses its own pink-red identity and text token, not the info teal", () => {
    expect(color("integration-lighting")).toBe("#d53367");
    expect(color("integration-lighting-text")).toBe("#e687a5");
    for (const source of [badgeSource, buttonSource]) {
      const lighting = source.split("\n").find((line) => /^\s*lighting:/.test(line))!;
      expect(lighting).toContain("text-integration-lighting-text");
      expect(lighting).not.toContain("text-info-text");
    }
  });

  it("reload hover and pressed use dark on-info text with no white or shadow", () => {
    const reload = buttonSource.split("\n").find((line) => /^\s*reload:/.test(line))!;
    expect(reload).toContain("can-hover:text-on-info");
    expect(reload).toContain("active:text-on-info");
    expect(reload).toContain("can-hover:[text-shadow:none]");
    expect(reload).not.toMatch(/text-white|text-on-primary|text-on-danger/);
  });
});
