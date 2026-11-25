import { mkdir, readFile, rm, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import { applicationDataDirectory } from "../config/store.js";

export const DISCLOSURE_VERSION = 1;
export const AI_DISCLOSURE = `Agent mode sends your request and the normalized MultiTracks information
needed to answer it to OpenAI through your signed-in ChatGPT/Codex account.

StagePilot will not send MultiTracks OAuth tokens, refresh tokens, client
secrets, authorization codes, cookies, Keychain contents, or raw MCP
responses.

The deterministic StagePilot commands can still be used without ChatGPT.`;

export class PrivacyStore {
  constructor(readonly filePath = path.join(applicationDataDirectory(), "agent-privacy.json")) {}
  async status(): Promise<{ accepted: boolean; version: number | null; acceptedAt: string | null }> {
    try {
      const data = JSON.parse(await readFile(this.filePath, "utf8")) as { accepted?: boolean; version?: number; acceptedAt?: string };
      return { accepted: data.accepted === true && data.version === DISCLOSURE_VERSION, version: data.version ?? null, acceptedAt: data.acceptedAt ?? null };
    } catch { return { accepted: false, version: null, acceptedAt: null }; }
  }
  async accept(): Promise<void> {
    await mkdir(path.dirname(this.filePath), { recursive: true, mode: 0o700 });
    const temporary = `${this.filePath}.${process.pid}.tmp`;
    await writeFile(temporary, `${JSON.stringify({ accepted: true, version: DISCLOSURE_VERSION, acceptedAt: new Date().toISOString() }, null, 2)}\n`, { mode: 0o600 });
    await rename(temporary, this.filePath);
  }
  async reset(): Promise<void> { await rm(this.filePath, { force: true }); }
}

