import { describe, expect, it } from "vitest";

import { libraryStatus } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
import type { HelperState } from "../types";
import { apiGood, apiLine, pillParts, scriptActive, scriptLine, scriptNote } from "./playbackLiveLinkState";

describe("the Playback API line: the backend's own sentence, with a tone", () => {
  it("connected, MIDI, searching and not connected", () => {
    expect(apiLine(playbackStatus({ reason: "Connected to Playback at 10.30.0.141." }), "x")).toEqual({ tone: "success", text: "Connected to Playback at 10.30.0.141." });
    expect(apiLine(playbackStatus({ active_source: "midi", reason: "Connected through MIDI." }), "x")).toEqual({ tone: "midi", text: "Connected through MIDI." });
    expect(apiLine(playbackStatus({ active_source: "none", reason: "Looking for Playback…", scan: { state: "scanning", candidates: [], reason: null, current: 1, total: 2 } }), "x").tone).toBe("info");
    // The backend's sentence still names the old button; the page says the button's name.
    expect(apiLine(playbackStatus({ active_source: "none", reason: "Disconnected. Choose Scan Network to find Playback." }), "x")).toEqual({ tone: "idle", text: "Not connected. Choose Search to find Playback." });
    expect(apiLine(playbackStatus({ active_source: "none", reason: "Disconnected. Choose Scan to find Playback." }), "x").text).toBe("Not connected. Choose Scan to find Playback.");
    expect(apiLine(playbackStatus({ active_source: "none", reason: "" }), "Checking Playback…").text).toBe("Checking Playback…");
  });

  it("only the API itself counts as the API being good (MIDI standing in does not)", () => {
    expect(apiGood(playbackStatus())).toBe(true);
    expect(apiGood(playbackStatus({ active_source: "midi", sources: { playback_api: { connected: false, reason: "" }, midi: { connected: true, reason: "" } } }))).toBe(false);
    expect(apiGood(undefined)).toBe(false);
  });
});

describe("the Library Link line: words for every state, and never a warning for a computer that has none", () => {
  const line = (state: Parameters<typeof libraryStatus>[0], over = {}) => scriptLine(libraryStatus(state, over), true);

  it("says each state in the words Hunter asked for", () => {
    expect(line("ready")).toEqual({ tone: "success", text: "Ready, version 0.3.0" });
    expect(line("not_installed")).toEqual({ tone: "idle", text: "Not installed" });
    expect(line("outdated").text).toBe("Update available, version 0.2.0");
    expect(line("needs_key")).toEqual({ tone: "warning", text: "Needs the command again" });
    expect(line("unreachable")).toEqual({ tone: "warning", text: "Not answering" });
  });

  it("not installed is grey, never red", () => {
    expect(line("not_installed").tone).toBe("idle");
  });

  it("before the first answer it says it is checking, and with no computer chosen it says not installed", () => {
    expect(scriptLine(null, true).text).toBe("Checking…");
    expect(scriptLine(null, false).text).toBe("Not installed");
  });

  it("the script is active in ready and outdated only", () => {
    expect(scriptActive(libraryStatus("ready"))).toBe(true);
    expect(scriptActive(libraryStatus("outdated"))).toBe(true);
    for (const state of ["not_installed", "needs_key", "unreachable"] as const) expect(scriptActive(libraryStatus(state))).toBe(false);
    expect(scriptActive(null)).toBe(false);
  });
});

describe("the pill: one short word for each of the two statuses", () => {
  const line = (tone: "success" | "idle" | "info" | "midi") => ({ tone, text: "x" });
  const library = (state: HelperState) => libraryStatus(state);

  it("says Connected for both when the API is connected and the script answers", () => {
    const parts = pillParts({ good: true, line: line("success") }, library("ready"), true);
    expect(parts.api).toEqual({ word: "Connected", tone: "success" });
    expect(parts.script).toEqual({ word: "Connected", tone: "success" });
  });

  it("names the API's other states: searching, MIDI only, not connected", () => {
    expect(pillParts({ good: false, line: line("info") }, null, false).api).toEqual({ word: "Searching", tone: "info" });
    expect(pillParts({ good: false, line: line("midi") }, null, false).api.word).toBe("MIDI only");
    expect(pillParts({ good: false, line: line("idle") }, null, false).api).toEqual({ word: "Not connected", tone: "idle" });
  });

  it("names each state of the Library Link, and needs-setup is amber, never the calm teal of a healthy state", () => {
    const word = (state: HelperState) => pillParts({ good: true, line: line("success") }, library(state), true).script;
    expect(word("outdated").word).toBe("Update available");
    expect(word("needs_key")).toEqual({ word: "Needs setup", tone: "warning" });
    expect(word("unreachable")).toEqual({ word: "Not answering", tone: "warning" });
    expect(word("not_installed")).toEqual({ word: "Not installed", tone: "idle" });
  });

  it("says Not installed when there is no computer yet and Checking while the first answer is awaited", () => {
    expect(pillParts({ good: false, line: line("idle") }, null, false).script.word).toBe("Not installed");
    expect(pillParts({ good: true, line: line("success") }, null, true).script.word).toBe("Checking");
  });
});

describe("the note under the script line", () => {
  it("is the backend's words when the script answers but Playback's data cannot be read yet, and nothing otherwise", () => {
    expect(scriptNote(libraryStatus("ready", { state: "waiting", message: "Click Allow on the Playback computer." }))).toBe("Click Allow on the Playback computer.");
    expect(scriptNote(libraryStatus("ready", { state: "problem", message: "macOS has not allowed it." }))).toBe("macOS has not allowed it.");
    expect(scriptNote(libraryStatus("ready"))).toBeNull();
    expect(scriptNote(libraryStatus("not_installed"))).toBeNull();
    expect(scriptNote(libraryStatus("unreachable"))).toBeNull();
    expect(scriptNote(null)).toBeNull();
  });
});
