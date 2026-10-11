import { describe, expect, it } from "vitest";
import { cx } from "./cx";

describe("cx", () => {
  it("joins strings in order and skips falsy values", () => {
    expect(cx("bg-primary", false, null, undefined, 0, "", "text-ink")).toBe("bg-primary text-ink");
  });
  it("keeps authored classes and caller overrides without merging", () => {
    expect(cx("p-2 p-4", "p-6")).toBe("p-2 p-4 p-6");
    expect(cx()).toBe("");
  });
});
