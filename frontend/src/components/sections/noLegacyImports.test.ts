import { describe, expect, it } from "vitest";

/**
 * A page is built from the section templates, not from its own copy of them. The shared Live Link pieces and the helper setup box live
 * in `components/sections/` only; no file imports them (or the old per-helper copies) from where they used to be.
 */
const sources = import.meta.glob("/src/**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const files = Object.entries(sources).filter(([file]) => !file.endsWith("noLegacyImports.test.ts"));

const MOVED = ["LiveLinkShell", "LiveLinkPill", "LiveLinkField", "DisclosureButton"];
const importsOf = (text: string) => Array.from(text.matchAll(/import\s+(?:type\s+)?\{([^}]*)\}\s+from\s+["']([^"']+)["']/g)).map((match) => ({
  names: (match[1] ?? "").split(",").map((name) => (name.trim().replace(/^type\s+/, "").split(/\s+as\s+/)[0] ?? "")),
  from: match[2] ?? "",
}));

describe("the section templates are the only home of the Live Link pieces", () => {
  it("sees the source tree", () => {
    expect(files.length).toBeGreaterThan(100);
    expect(Object.keys(sources).some((file) => file.endsWith("components/sections/ConnectionSection.tsx"))).toBe(true);
  });

  it("no file imports LiveLinkShell, LiveLinkPill, LiveLinkField or DisclosureButton from LightingOscParts, and LightingOscParts exports none", () => {
    const offenders = files
      .filter(([, text]) => importsOf(text).some((entry) => /LightingOscParts$/.test(entry.from) && entry.names.some((name) => MOVED.includes(name))))
      .map(([file]) => file);
    expect(offenders).toEqual([]);
    const parts = sources["/src/components/LightingOscParts.tsx"] ?? "";
    expect(parts.length).toBeGreaterThan(1000);
    for (const name of MOVED) expect(parts).not.toMatch(new RegExp(`export\\s+(function|const)\\s+${name}\\b`));
    expect(parts).not.toMatch(/export\s+\{[^}]*\b(LiveLinkShell|LiveLinkPill|LiveLinkField|DisclosureButton)\b/);
  });

  it("the three per-helper install boxes are gone, and nothing imports them", () => {
    for (const name of ["LightkeyInstallBox", "PlaybackInstallBox", "ProPresenterInstallBox"]) {
      expect(Object.keys(sources).filter((file) => file.endsWith(`/${name}.tsx`) || file.endsWith(`/${name}.test.tsx`))).toEqual([]);
      expect(files.filter(([, text]) => importsOf(text).some((entry) => entry.from.endsWith(`/${name}`))).map(([file]) => file)).toEqual([]);
    }
  });

  it("outside sections/, no page defines its own Live Link shell, pill, field or Details button", () => {
    const offenders = files
      .filter(([file]) => !file.includes("/components/sections/"))
      .filter(([, text]) => MOVED.some((name) => new RegExp(`(function|const)\\s+${name}\\b`).test(text)))
      .map(([file]) => file);
    expect(offenders).toEqual([]);
  });
});
