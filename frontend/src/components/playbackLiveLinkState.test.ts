import { describe, expect, it } from "vitest";

import { helperOf, libraryStatus } from "../test/libraryFixtures";
import { playbackStatus } from "../test/playbackFixtures";
import type { HelperState, PlaybackRemote } from "../types";
import { REMOTE_NEEDS_PERMISSION, REMOTE_NOT_RUNNING, REMOTE_SWITCHING, REMOTE_USER_BUSY, apiGood, apiLine, pillParts, remoteWords, scriptActive, scriptLine, scriptNote, withRemoteWords } from "./playbackLiveLinkState";

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


describe("Library Link switching on Playback's Remote Connections: the words", () => {
  const remote = (over: Partial<PlaybackRemote> = {}): PlaybackRemote => ({ listening: false, auto: true, state: "switching", message: null, last_attempt_utc: null, ...over });
  const refused = (over = {}) => playbackStatus({ connected: false, active_source: "none", last_error: "Playback refused the connection; remote connections may be off", reason: "Disconnected. Looking for Playback… If it stays offline, turn on Remote Connections.", sources: { playback_api: { connected: false, reason: "" }, midi: { connected: false, reason: "" } }, ...over });
  const link = (state: HelperState, playback_remote?: PlaybackRemote) => libraryStatus(state, { helper: helperOf(state, { playback_remote }) });
  const today = apiLine(refused(), "x");

  it("says each state in the words Hunter chose", () => {
    expect(remoteWords(remote({ state: "switching" }))).toBe("The Library Link is switching on Playback's Remote Connections. This takes a few seconds.");
    expect(remoteWords(remote({ state: "off", auto: true }))).toBe(REMOTE_SWITCHING);
    expect(remoteWords(remote({ state: "needs_permission" }))).toBe(REMOTE_NEEDS_PERMISSION);
    expect(REMOTE_NEEDS_PERMISSION).toBe("The Library Link can switch on Playback's Remote Connections by itself once this Mac allows it. At the Playback computer open System Settings > Privacy & Security > Accessibility and turn on \"Playback Library\". Until then turn on Playback > menu > Playback Remote > Allow Remote Connections by hand.");
    expect(remoteWords(remote({ state: "user_busy" }))).toBe("Playback is being used; the Library Link will switch on Remote Connections when it is quiet.");
    expect(remoteWords(remote({ state: "playback_not_running" }))).toBe("Playback is not open on its computer.");
    expect(remoteWords(remote({ state: "failed", message: "macOS did not let it click." }))).toBe("macOS did not let it click. Turn on Allow Remote Connections in Playback by hand.");
    expect(remoteWords(remote({ state: "failed", message: "It did not work" }))).toBe("It did not work. Turn on Allow Remote Connections in Playback by hand.");
    expect(remoteWords(remote({ state: "failed", message: null }))).toBe("Turn on Allow Remote Connections in Playback by hand.");
  });

  it("says nothing when it is on, or off with no permission to switch it on", () => {
    expect(remoteWords(remote({ state: "on", listening: true }))).toBeNull();
    expect(remoteWords(remote({ state: "off", auto: false }))).toBeNull();
    expect(remoteWords(remote({ state: "switching", auto: false }))).toBeNull();
  });

  it("replaces the refused sentence, keeping the dot's tone", () => {
    const got = withRemoteWords(today, refused(), link("ready", remote()));
    expect(got).toEqual({ tone: "idle", text: REMOTE_SWITCHING });
    expect(withRemoteWords(apiLine(refused({ last_error: "Playback unavailable; remote connections may be off" }), "x"), refused({ last_error: "Playback unavailable; remote connections may be off" }), link("ready", remote({ state: "user_busy" }))).text).toBe(REMOTE_USER_BUSY);
    expect(withRemoteWords(today, refused(), link("outdated", remote({ state: "playback_not_running" }))).text).toBe(REMOTE_NOT_RUNNING);
  });

  it("leaves today's words alone: an older helper, on, a Library Link that is not working, a connected Playback, a search, another failure", () => {
    expect(withRemoteWords(today, refused(), link("ready"))).toBe(today);
    expect(withRemoteWords(today, refused(), link("ready", remote({ state: "on", listening: true })))).toBe(today);
    expect(withRemoteWords(today, refused(), link("ready", remote({ state: "off", auto: false })))).toBe(today);
    for (const state of ["not_installed", "needs_key", "unreachable"] as const) expect(withRemoteWords(today, refused(), link(state, remote()))).toBe(today);
    expect(withRemoteWords(today, refused(), null)).toBe(today);
    expect(withRemoteWords(today, null, link("ready", remote()))).toBe(today);
    const connected = playbackStatus();
    expect(withRemoteWords(apiLine(connected, "x"), connected, link("ready", remote()))).toEqual(apiLine(connected, "x"));
    const searching = refused({ scan: { state: "scanning", candidates: [], reason: null, current: 1, total: 2 } });
    expect(withRemoteWords(apiLine(searching, "x"), searching, link("ready", remote())).text).toBe(apiLine(searching, "x").text);
    const network = refused({ last_error: "Cannot reach Playback at 10.30.0.141: network unreachable" });
    expect(withRemoteWords(apiLine(network, "x"), network, link("ready", remote())).text).toBe(apiLine(network, "x").text);
    const idle = refused({ last_error: null });
    expect(withRemoteWords(apiLine(idle, "x"), idle, link("ready", remote())).text).toBe(apiLine(idle, "x").text);
  });

  it("never changes the pills' words", () => {
    const status = refused();
    const status2 = link("ready", remote({ state: "needs_permission" }));
    const before = pillParts({ good: apiGood(status), line: apiLine(status, "x") }, status2, true);
    const after = pillParts({ good: apiGood(status), line: withRemoteWords(apiLine(status, "x"), status, status2) }, status2, true);
    expect(after).toEqual(before);
  });
});
