import { describe, expect, it } from "vitest";

describe("source file names", () => {
  it("never collide case-insensitively under frontend/src (breaks macOS/Windows builds)", () => {
    const files = Object.keys(import.meta.glob("/src/**/*.{ts,tsx,css}"));
    expect(files.length).toBeGreaterThan(50);
    const seen = new Map<string, string>();
    const clashes: string[] = [];
    for (const f of files) {
      const key = f.toLowerCase();
      const prev = seen.get(key);
      if (prev !== undefined && prev !== f) clashes.push(`${prev} <-> ${f}`);
      seen.set(key, f);
    }
    expect(clashes).toEqual([]);
  });
});
