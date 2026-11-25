import { mkdir } from "node:fs/promises";
import path from "node:path";
import { applicationDataDirectory } from "../config/store.js";

export function codexHome(): string {
  return process.env.STAGEPILOT_CODEX_HOME ?? path.join(applicationDataDirectory(), "codex");
}

export async function ensureCodexHome(): Promise<string> {
  const home = codexHome();
  await mkdir(home, { recursive: true, mode: 0o700 });
  return home;
}

