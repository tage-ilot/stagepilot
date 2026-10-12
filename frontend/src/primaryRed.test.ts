import { describe, expect, it } from "vitest";

declare const process: { cwd(): string };
type Entry = { name: string; isDirectory(): boolean };
const { readdirSync, readFileSync } = (await import("node:fs" as string)) as {
  readdirSync(dir: string, opts: { withFileTypes: true }): Entry[];
  readFileSync(file: string, enc: "utf8"): string;
};
const { join } = (await import("node:path" as string)) as { join(...parts: string[]): string };

function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e: Entry) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
}

describe("primary red guard", () => {
  const config = readFileSync(join(process.cwd(), "tailwind.config.js"), "utf8");
  const color = (name: string) => new RegExp(`"${name}": "(#[0-9a-f]{6})"`, "i").exec(config)?.[1];

  it("theme primary is #c7231d with white on-primary and dark on-light", () => {
    expect(color("primary")).toBe("#c7231d");
    expect(color("on-primary")).toBe("#ffffff");
    expect(color("on-light")).toBe("#020617");
    expect(color("primary-active")).toBe("#b3201a");
    expect(color("primary-text")).toBe("#e3918e");
  });

  it("no retired coral value or name remains in src (this file excluded)", () => {
    const banned = new RegExp(["ff" + "6238", "ff" + "805e", "ff" + "9b7e", "255[ ,]+98[ ,]+56", "cor" + "al"].join("|"), "i");
    const offenders = walk(join(process.cwd(), "src"))
      .filter((f) => !f.endsWith("primaryRed.test.ts"))
      .filter((f) => banned.test(readFileSync(f, "utf8")));
    expect(offenders).toEqual([]);
  });
});
