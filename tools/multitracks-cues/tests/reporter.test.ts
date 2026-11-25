import { mkdtemp, readFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { Reporter } from "../src/reporting/reporter.js";
import { redactString } from "../src/security/redact.js";

describe("ordinal cue reports", () => {
  it("separates setlist position, ordinal, velocity, and resolved position", async () => {
    const directory = await mkdtemp(path.join(os.tmpdir(), "stagepilot-report-"));
    const files = await new Reporter(directory).write({
      startedAt: "2026-07-30T00:00:00.000Z",
      finishedAt: "2026-07-30T00:00:01.000Z",
      command: "prepare",
      serverOrigin: "https://mcp.multitracks.com",
      setlist: { id: "synthetic", name: "Synthetic", items: [] },
      configuration: { cueProfile: "setlist-ordinal-test", channel: 1, note: 112, busId: "aux" },
      plan: [{
        setlistPosition: 7,
        songOrdinal: 3,
        velocity: 3,
        songTitle: "Synthetic Song",
        targetType: "library",
        targetId: "library",
        libraryId: "library",
        bankId: "default",
        busId: "aux",
        resolvedPosition: { measure: 1, beat: 2, tick: 0 },
        operations: ["CREATE_LIBRARY_EVENT"],
        reason: "test",
        verificationStrategy: "read-back",
        existingBanks: [],
        selectedBusEvents: [],
      }],
      finalStatus: "success",
    });
    expect(await readFile(files.text, "utf8")).toMatch(/Position 7; ordinal 3; velocity 3/);
    expect(await readFile(files.csv, "utf8")).toMatch(/setlist_position.*song_ordinal.*velocity.*resolved_position/);
    expect(JSON.parse(await readFile(files.json, "utf8")).plan[0].resolvedPosition).toEqual({ measure: 1, beat: 2, tick: 0 });
  });
});

describe("agent redaction", () => {
  it("redacts bearer tokens, URL queries, device codes, emails, and user homes", () => {
    const output = redactString("Bearer abc.def https://auth.test/login?code=secret ABCD-1234 human@example.com C:\\Users\\human\\codex");
    expect(output).not.toContain("abc.def");
    expect(output).not.toContain("code=secret");
    expect(output).not.toContain("ABCD-1234");
    expect(output).not.toContain("human@example.com");
    expect(output).not.toContain("Users\\human");
  });
});

