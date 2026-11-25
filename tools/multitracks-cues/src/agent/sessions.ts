import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import { applicationDataDirectory } from "../config/store.js";
import { AGENT_PROMPT_VERSION } from "./instructions.js";
import type { AgentService } from "../ai/agent-service.js";

export interface SessionMetadata {
  threadId: string;
  label?: string;
  lastAccessedAt: string;
  promptVersion: string;
  history?: ReturnType<AgentService["getHistory"]>;
}

export class SessionStore {
  constructor(readonly filePath = path.join(applicationDataDirectory(), "agent-sessions.json")) {}
  async list(): Promise<SessionMetadata[]> {
    try { return JSON.parse(await readFile(this.filePath, "utf8")) as SessionMetadata[]; }
    catch { return []; }
  }
  async get(threadId: string): Promise<SessionMetadata | undefined> {
    return (await this.list()).find((s) => s.threadId === threadId);
  }
  async remember(threadId: string, label?: string, history?: SessionMetadata["history"]): Promise<void> {
    const sessions = (await this.list()).filter((item) => item.threadId !== threadId);
    sessions.unshift({ threadId, label, lastAccessedAt: new Date().toISOString(), promptVersion: AGENT_PROMPT_VERSION, history });
    await this.#write(sessions.slice(0, 50));
  }
  async remove(threadId: string): Promise<void> { await this.#write((await this.list()).filter((item) => item.threadId !== threadId)); }
  async #write(value: SessionMetadata[]): Promise<void> {
    await mkdir(path.dirname(this.filePath), { recursive: true, mode: 0o700 });
    const temporary = `${this.filePath}.${process.pid}.tmp`;
    await writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
    await rename(temporary, this.filePath);
  }
}

