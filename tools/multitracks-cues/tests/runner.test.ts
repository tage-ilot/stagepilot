import { describe, expect, it } from "vitest";
import { codexVersionLabel } from "../src/agent/runner.js";

describe("runner", () => {
  it("returns openai-oauth as version label", () => {
    expect(codexVersionLabel()).toBe("openai-oauth");
  });
});

