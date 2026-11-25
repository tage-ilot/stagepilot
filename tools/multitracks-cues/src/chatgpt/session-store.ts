import { readFile, writeFile, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import os from "node:os";

const SESSION_DIR = path.join(os.homedir(), ".stagepilot", "chatgpt-mcp");
const SESSION_FILE = path.join(SESSION_DIR, "session.json");

export interface StoredSession {
  conversationId: string | null;
  conversationUrl: string | null;
  lastUsed: string;
}

async function ensureSessionDir(): Promise<void> {
  if (!existsSync(SESSION_DIR)) {
    await mkdir(SESSION_DIR, { recursive: true });
  }
}

export async function loadSession(): Promise<StoredSession> {
  try {
    await ensureSessionDir();
    if (existsSync(SESSION_FILE)) {
      const data = await readFile(SESSION_FILE, "utf-8");
      const session = JSON.parse(data) as StoredSession;
      console.error(`[session-store] Loaded session: ${JSON.stringify(session)}`);
      return session;
    }
  } catch (error) {
    console.error(`[session-store] Error loading session: ${error}`);
    // ignore errors
  }
  return { conversationId: null, conversationUrl: null, lastUsed: "" };
}

export async function saveSession(session: StoredSession): Promise<void> {
  await ensureSessionDir();
  await writeFile(SESSION_FILE, JSON.stringify(session, null, 2));
}

export async function getConversationId(): Promise<string | null> {
  const session = await loadSession();
  return session.conversationId;
}

export async function setConversationId(id: string, url: string): Promise<void> {
  console.error(`[session-store] Saving conversation: ${id}`);
  const session = await loadSession();
  session.conversationId = id;
  session.conversationUrl = url;
  session.lastUsed = new Date().toISOString();
  await saveSession(session);
  console.error(`[session-store] Saved to: ${SESSION_FILE}`);
}

export async function clearConversation(): Promise<void> {
  await saveSession({ conversationId: null, conversationUrl: null, lastUsed: "" });
}

