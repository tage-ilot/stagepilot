import { describe, expect, it } from "vitest";

import { helperStatus, localStatus, noComputerStatus, presentation } from "../test/propresenterLinkFixtures";
import { importBlocked, importBlockedWords, linkLine, linkNote, linkPill, proPresenterLine, proPresenterPill } from "./proPresenterLinkState";

const ACCESS = "Allow the Presenter Link in System Settings > Privacy & Security > Accessibility, then try again.";

describe("the ProPresenter line (the existing connection)", () => {
  it.each([
    ["connected", "success", "Connected to ProPresenter at 192.168.1.60"],
    ["ready", "info", "Ready, ProPresenter at 192.168.1.60"],
    ["connecting", "info", "Connecting to ProPresenter at 192.168.1.60\u2026"],
    ["error", "warning", "Cannot reach ProPresenter at 192.168.1.60"],
    ["disconnected", "idle", "Not connected to ProPresenter at 192.168.1.60"],
  ] as const)("%s reads %s", (connection_status, tone, text) => {
    expect(proPresenterLine(presentation({ connection_status }))).toEqual({ tone, text });
  });

  it("says plainly when Presentation is not on, and while it has not been read", () => {
    expect(proPresenterLine(presentation({ enabled: false }))).toEqual({ tone: "idle", text: "Not connected. Save the connection to use ProPresenter" });
    expect(proPresenterLine(null).text).toBe("Checking ProPresenter\u2026");
    expect(proPresenterPill(null)).toEqual({ word: "Checking", tone: "idle" });
    expect(proPresenterPill(presentation({ connection_status: "connected" }))).toEqual({ word: "Connected", tone: "success" });
    expect(proPresenterPill(presentation({ enabled: false })).word).toBe("Not connected");
    expect(proPresenterPill(presentation({ connection_status: "error" }))).toEqual({ word: "Not reachable", tone: "warning" });
  });
});

describe("the Presenter Link line: every status text", () => {
  it.each([
    ["ready, another computer", helperStatus("ready"), "success", "Ready, version 0.1.0"],
    ["ready, this computer", localStatus(), "success", "Ready, on this computer"],
    ["not installed", helperStatus("not_installed"), "idle", "Not installed"],
    ["update available", helperStatus("outdated"), "info", "Update available, version 0.0.9"],
    ["needs the command again", helperStatus("needs_command"), "warning", "Needs the command again"],
    ["not answering", helperStatus("not_answering"), "warning", "Not answering"],
    ["checking", helperStatus("checking"), "idle", "Checking\u2026"],
    ["can add songs, cannot import (another computer)", helperStatus("ready", { can_import: false, import_blocker: ACCESS }), "info", "Can add songs, cannot import missing songs (macOS Accessibility not allowed)"],
    ["can add songs, cannot import (this computer)", localStatus({ can_import: false, import_blocker: "Allow StagePilot in System Settings > Privacy & Security > Accessibility, then try again." }), "info", "Can add songs, cannot import missing songs (macOS Accessibility not allowed), on this computer"],
    ["not a Mac", localStatus({ can_import: false, import_blocker: "Importing a song needs a Mac; this computer is not one." }), "info", "Can add songs, cannot import missing songs (needs a Mac), on this computer"],
    ["this computer's folder is not there", localStatus({ state: "not_answering" }), "warning", "Not answering, on this computer"],
    ["macOS is asking for permission", localStatus({ state: "waiting", can_import: false }), "info", "Waiting for macOS"],
  ])("%s", (_name, status, tone, text) => {
    expect(linkLine(status, true)).toEqual({ tone, text });
  });

  it("is a calm grey 'Not installed' with no computer saved, and 'Checking' before the first answer", () => {
    expect(linkLine(noComputerStatus(), false)).toEqual({ tone: "idle", text: "Not installed" });
    expect(linkLine(null, false)).toEqual({ tone: "idle", text: "Not installed" });
    expect(linkLine(noComputerStatus(), true)).toEqual({ tone: "idle", text: "Not installed" });
    expect(linkLine(null, true)).toEqual({ tone: "idle", text: "Checking\u2026" });
  });

  it("never turns an optional Link into a danger", () => {
    for (const state of ["ready", "not_installed", "outdated", "needs_command", "not_answering", "checking", "waiting"] as const) {
      expect(["danger", "error"]).not.toContain(linkLine(helperStatus(state), true).tone);
    }
  });

  it("the pill carries one or two words", () => {
    expect(linkPill(helperStatus("ready"), true)).toEqual({ word: "Connected", tone: "success" });
    expect(linkPill(localStatus(), true)).toEqual({ word: "Connected", tone: "success" });
    expect(linkPill(helperStatus("ready", { can_import: false }), true)).toEqual({ word: "Limited", tone: "info" });
    expect(linkPill(helperStatus("outdated"), true)).toEqual({ word: "Update available", tone: "info" });
    expect(linkPill(helperStatus("needs_command"), true)).toEqual({ word: "Needs setup", tone: "warning" });
    expect(linkPill(helperStatus("not_answering"), true)).toEqual({ word: "Not answering", tone: "warning" });
    expect(linkPill(helperStatus("not_installed"), true)).toEqual({ word: "Not installed", tone: "idle" });
    expect(linkPill(helperStatus("checking"), true).word).toBe("Checking");
    expect(linkPill(localStatus({ state: "waiting", can_import: false }), true)).toEqual({ word: "Waiting for macOS", tone: "info" });
    expect(linkPill(null, true).word).toBe("Checking");
    expect(linkPill(null, false).word).toBe("Not installed");
    expect(linkPill(noComputerStatus(), true).word).toBe("Not installed");
  });
});

describe("the import blocker, in plain words with the one-sentence fix", () => {
  it("names the cause in a few words", () => {
    expect(importBlockedWords(ACCESS)).toBe("macOS Accessibility not allowed");
    expect(importBlockedWords("Importing a song needs a Mac; this computer is not one.")).toBe("needs a Mac");
    expect(importBlockedWords(undefined)).toBe("import not available");
  });

  it("is a note only while the Link is ready and cannot import, and carries the backend's fix sentence", () => {
    const status = localStatus({ can_import: false, import_blocker: ACCESS });
    expect(importBlocked(status)).toBe(true);
    expect(linkNote(status)).toBe(`StagePilot can add songs ProPresenter already has, but cannot load a missing song from ProPresenter's search. ${ACCESS}`);
    expect(linkNote(localStatus())).toBeNull();
    expect(importBlocked(helperStatus("needs_command"))).toBe(false);
    expect(linkNote(helperStatus("ready"))).toBeNull();
    expect(linkNote(null)).toBeNull();
    expect(linkNote(noComputerStatus())).toBeNull();
  });

  it("says the backend's one sentence while macOS waits for the person to answer a prompt", () => {
    const message = "macOS may be asking for permission on this computer. Answer the prompt, then press Connect again.";
    const status = localStatus({ state: "waiting", can_import: false, libraries: [], message });
    expect(importBlocked(status)).toBe(false);
    expect(linkNote(status)).toBe(message);
  });

  it("says what the backend says when this computer's folder is not there, or the answer is a memory", () => {
    expect(linkNote(localStatus({ state: "not_answering", message: "ProPresenter's folder was not found on this computer." }))).toBe("ProPresenter's folder was not found on this computer.");
    expect(linkNote(helperStatus("ready", { stale: true, message: "The ProPresenter computer did not answer just now." }))).toBe("The ProPresenter computer did not answer just now.");
  });
});
