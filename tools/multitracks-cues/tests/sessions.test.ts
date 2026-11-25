import os from "node:os";
import path from "node:path";
import { mkdtemp, readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";
import { SessionStore } from "../src/agent/sessions.js";
import { AGENT_PROMPT_VERSION } from "../src/agent/instructions.js";

describe("SessionStore", () => {
  it("returns empty list when file does not exist", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-sessions-"));
    const store = new SessionStore(path.join(dir, "sessions.json"));
    expect(await store.list()).toEqual([]);
  });

  it("persists and retrieves sessions", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-sessions-"));
    const filePath = path.join(dir, "sessions.json");
    const store = new SessionStore(filePath);
    await store.remember("thread-1", "Sunday prep");
    const sessions = await store.list();
    expect(sessions).toHaveLength(1);
    expect(sessions[0]!.threadId).toBe("thread-1");
    expect(sessions[0]!.label).toBe("Sunday prep");
    expect(sessions[0]!.promptVersion).toBe(AGENT_PROMPT_VERSION);
  });

  it("prepends most recent session", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-sessions-"));
    const store = new SessionStore(path.join(dir, "sessions.json"));
    await store.remember("thread-1");
    await store.remember("thread-2");
    const sessions = await store.list();
    expect(sessions[0]!.threadId).toBe("thread-2");
    expect(sessions[1]!.threadId).toBe("thread-1");
  });

  it("deduplicates by thread ID", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-sessions-"));
    const store = new SessionStore(path.join(dir, "sessions.json"));
    await store.remember("thread-1", "first");
    await store.remember("thread-1", "updated");
    const sessions = await store.list();
    expect(sessions).toHaveLength(1);
    expect(sessions[0]!.label).toBe("updated");
  });

  it("caps at 50 sessions", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-sessions-"));
    const store = new SessionStore(path.join(dir, "sessions.json"));
    for (let i = 0; i < 55; i++) await store.remember(`thread-${i}`);
    const sessions = await store.list();
    expect(sessions).toHaveLength(50);
    expect(sessions[0]!.threadId).toBe("thread-54");
    expect(sessions[49]!.threadId).toBe("thread-5");
  });

  it("removes a session by thread ID", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-sessions-"));
    const store = new SessionStore(path.join(dir, "sessions.json"));
    await store.remember("thread-1");
    await store.remember("thread-2");
    await store.remove("thread-1");
    const sessions = await store.list();
    expect(sessions).toHaveLength(1);
    expect(sessions[0]!.threadId).toBe("thread-2");
  });

  it("writes atomically via temp file and rename", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-sessions-"));
    const filePath = path.join(dir, "sessions.json");
    const store = new SessionStore(filePath);
    await store.remember("thread-1");
    const raw = await readFile(filePath, "utf8");
    const parsed = JSON.parse(raw);
    expect(Array.isArray(parsed)).toBe(true);
    expect(parsed[0].threadId).toBe("thread-1");
  });

  it("records prompt version for each session", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-sessions-"));
    const store = new SessionStore(path.join(dir, "sessions.json"));
    await store.remember("thread-1");
    const sessions = await store.list();
    expect(sessions[0]!.promptVersion).toBe(AGENT_PROMPT_VERSION);
  });
});

