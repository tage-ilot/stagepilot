import { describe, expect, it } from "vitest";
import { maskEmail } from "../src/codex/account-service.js";

describe("CodexAccountService", () => {
  describe("maskEmail", () => {
    it("masks email correctly", () => {
      expect(maskEmail("user@example.com")).toBe("u***@example.com");
    });

    it("returns null for undefined", () => {
      expect(maskEmail(undefined)).toBeNull();
    });

    it("handles email without domain", () => {
      expect(maskEmail("test")).toBe("[MASKED]");
    });
  });
});

