import { mkdir, open, rename } from "node:fs/promises";
import path from "node:path";

export interface JournalEntry {
  timestamp: string;
  setlistPosition: number;
  targetId?: string;
  operation: string;
  outcome: "verified" | "skipped" | "failed" | "uncertain";
  message: string;
  eventId?: string;
}

export class OperationJournal {
  readonly entries: JournalEntry[] = [];

  constructor(readonly filePath: string) {}

  async record(entry: JournalEntry): Promise<void> {
    this.entries.push(entry);
    const directory = path.dirname(this.filePath);
    await mkdir(directory, { recursive: true });
    const temporary = `${this.filePath}.${process.pid}.tmp`;
    const handle = await open(temporary, "w", 0o600);
    try {
      await handle.writeFile(`${JSON.stringify({ entries: this.entries }, null, 2)}\n`);
      await handle.sync();
    } finally {
      await handle.close();
    }
    await rename(temporary, this.filePath);
  }
}

