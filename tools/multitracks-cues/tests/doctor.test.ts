import { describe, expect, it } from "vitest";
import { runDoctor } from "../src/doctor.js";

describe("doctor", () => {
  it("returns an array of checks", async () => {
    const checks = await runDoctor();
    expect(Array.isArray(checks)).toBe(true);
    expect(checks.length).toBeGreaterThan(0);
  });

  it("includes Node.js check", async () => {
    const checks = await runDoctor();
    const nodeCheck = checks.find((c) => c.name === "Node.js");
    expect(nodeCheck).toBeDefined();
    expect(["ok", "error"]).toContain(nodeCheck!.status);
  });

  it("includes Codex home check", async () => {
    const checks = await runDoctor();
    const homeCheck = checks.find((c) => c.name === "Codex home");
    expect(homeCheck).toBeDefined();
  });

  it("includes ChatGPT check", async () => {
    const checks = await runDoctor();
    const chatgptCheck = checks.find((c) => c.name === "ChatGPT");
    expect(chatgptCheck).toBeDefined();
  });

  it("includes AI data sharing check", async () => {
    const checks = await runDoctor();
    const privacyCheck = checks.find((c) => c.name === "AI data sharing");
    expect(privacyCheck).toBeDefined();
  });
});

