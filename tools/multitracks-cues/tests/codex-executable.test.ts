import { describe, expect, it } from "vitest";
import { codexHome } from "../src/codex/executable.js";

describe("codexHome", () => {
  it("returns a string path", () => {
    const home = codexHome();
    expect(typeof home).toBe("string");
    expect(home.length).toBeGreaterThan(0);
  });

  it("uses STAGEPILOT_CODEX_HOME when set", () => {
    const original = process.env.STAGEPILOT_CODEX_HOME;
    process.env.STAGEPILOT_CODEX_HOME = "/tmp/test-codex";
    try {
      expect(codexHome()).toBe("/tmp/test-codex");
    } finally {
      if (original === undefined) delete process.env.STAGEPILOT_CODEX_HOME;
      else process.env.STAGEPILOT_CODEX_HOME = original;
    }
  });
});

