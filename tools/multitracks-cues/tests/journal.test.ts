import os from "node:os";
import path from "node:path";
import { mkdtemp, readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";
import { OperationJournal, type JournalEntry } from "../src/reporting/journal.js";

describe("OperationJournal", () => {
  it("records entries to a JSON file", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-journal-"));
    const journal = new OperationJournal(path.join(dir, "journal.json"));
    const entry: JournalEntry = {
      timestamp: new Date().toISOString(), setlistPosition: 2, operation: "CREATE_LIBRARY_EVENT",
      outcome: "verified", message: "Created successfully", eventId: "evt-1",
    };
    await journal.record(entry);
    const raw = await readFile(journal.filePath, "utf8");
    const data = JSON.parse(raw);
    expect(data.entries).toHaveLength(1);
    expect(data.entries[0].setlistPosition).toBe(2);
    expect(data.entries[0].eventId).toBe("evt-1");
  });

  it("appends multiple entries", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-journal-"));
    const journal = new OperationJournal(path.join(dir, "journal.json"));
    await journal.record({ timestamp: new Date().toISOString(), setlistPosition: 1, operation: "CREATE", outcome: "verified", message: "ok" });
    await journal.record({ timestamp: new Date().toISOString(), setlistPosition: 2, operation: "SKIP", outcome: "skipped", message: "skip" });
    const raw = await readFile(journal.filePath, "utf8");
    const data = JSON.parse(raw);
    expect(data.entries).toHaveLength(2);
  });

  it("creates parent directories", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-journal-"));
    const nested = path.join(dir, "sub", "dir", "journal.json");
    const journal = new OperationJournal(nested);
    await journal.record({ timestamp: new Date().toISOString(), setlistPosition: 1, operation: "CREATE", outcome: "verified", message: "ok" });
    const raw = await readFile(nested, "utf8");
    expect(JSON.parse(raw).entries).toHaveLength(1);
  });

  it("writes atomically via temp file and rename", async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), "stagepilot-journal-"));
    const filePath = path.join(dir, "journal.json");
    const journal = new OperationJournal(filePath);
    await journal.record({ timestamp: new Date().toISOString(), setlistPosition: 1, operation: "CREATE", outcome: "verified", message: "ok" });
    const raw = await readFile(filePath, "utf8");
    const data = JSON.parse(raw);
    expect(data.entries[0].outcome).toBe("verified");
  });
});

