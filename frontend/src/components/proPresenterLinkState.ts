import type { LinkState, LinkStatus } from "../propresenterLink";
import type { ProPresenterStatusResponse } from "../types";
import type { LiveLinkLine, PillPart } from "./playbackLiveLinkState";
import type { Tone } from "./ui/types";

/** The two connection statuses of the ProPresenter Live Link block, named in ONE place (pills, status lines, hover text, aria labels, live region). */
export const PROPRESENTER_API_NAME = "ProPresenter API";
export const PRESENTER_LINK_NAME = "Presenter Link";

/**
 * The words and tones of the ProPresenter Live Link block (DESIGN.md: a dot AND words, never colour alone). Two statuses are kept apart on
 * purpose: ProPresenter itself (the connection the timer and the Looks use) and the optional Presenter Link (the way StagePilot reaches
 * the song files and the screen of the ProPresenter computer). The Link is optional: a computer that has none is "Not installed", a calm
 * grey, never a warning, and it never changes the header's Ready / Check system verdict.
 */

/** The ProPresenter line: what the existing connection says, in the same shape as the Playback API line. */
export function proPresenterLine(status: Pick<ProPresenterStatusResponse, "enabled" | "host" | "connection_status"> | null): LiveLinkLine {
  if (!status) return { tone: "idle", text: "Checking ProPresenter…" };
  const at = status.host ? ` at ${status.host}` : "";
  if (!status.enabled) return { tone: "idle", text: "Not connected. Save the connection to use ProPresenter" };
  switch (status.connection_status) {
    case "connected": return { tone: "success", text: `Connected to ProPresenter${at}` };
    case "ready": return { tone: "info", text: `Ready, ProPresenter${at}` };
    case "connecting": return { tone: "info", text: `Connecting to ProPresenter${at}…` };
    case "error": return { tone: "warning", text: `Cannot reach ProPresenter${at}` };
    default: return { tone: "idle", text: `Not connected to ProPresenter${at}` };
  }
}

/** The one word the pill beside the title carries for ProPresenter itself. */
export function proPresenterPill(status: Pick<ProPresenterStatusResponse, "enabled" | "connection_status"> | null): PillPart {
  if (!status) return { word: "Checking", tone: "idle" };
  if (!status.enabled) return { word: "Not connected", tone: "idle" };
  switch (status.connection_status) {
    case "connected": return { word: "Connected", tone: "success" };
    case "ready": return { word: "Ready", tone: "info" };
    case "connecting": return { word: "Connecting", tone: "info" };
    case "error": return { word: "Not reachable", tone: "warning" };
    default: return { word: "Not connected", tone: "idle" };
  }
}

const LINK_TONE: Record<LinkState, Tone> = {
  ready: "success",
  outdated: "info",
  needs_command: "warning",
  not_answering: "warning",
  waiting: "info",
  not_installed: "idle",
  checking: "idle",
};

/** Why a song cannot be imported, in the few words the line carries. */
export function importBlockedWords(blocker: string | undefined | null): string {
  if (blocker && /accessibility/i.test(blocker)) return "macOS Accessibility not allowed";
  if (blocker && /\bmac\b/i.test(blocker)) return "needs a Mac";
  return "import not available";
}

/** The Link can add songs ProPresenter already has, but not load a missing one from ProPresenter's Search. */
export const importBlocked = (status: LinkStatus | null): boolean => status?.state === "ready" && !status.can_import;

/** The Presenter Link line: `Ready, on this computer`, `Ready, version 0.1.0`, `Not installed`, `Update available, version 0.1.0`... */
export function linkLine(status: LinkStatus | null, hasComputer: boolean): LiveLinkLine {
  if (!hasComputer) return { tone: "idle", text: "Not installed" };
  if (!status) return { tone: "idle", text: "Checking…" };
  const local = status.mode === "local";
  const where = local ? ", on this computer" : "";
  const withVersion = !local && status.version ? `, version ${status.version}` : "";
  if (status.mode === null) return { tone: LINK_TONE.not_installed, text: "Not installed" };
  switch (status.state) {
    case "ready":
      if (!status.can_import) {
        return { tone: "info", text: `Can add songs, cannot import missing songs (${importBlockedWords(status.import_blocker)})${where}` };
      }
      return { tone: LINK_TONE.ready, text: local ? "Ready, on this computer" : `Ready${withVersion}` };
    case "outdated": return { tone: LINK_TONE.outdated, text: `Update available${withVersion}` };
    case "needs_command": return { tone: LINK_TONE.needs_command, text: "Needs the command again" };
    case "not_answering": return { tone: LINK_TONE.not_answering, text: `Not answering${where}` };
    case "checking": return { tone: LINK_TONE.checking, text: "Checking…" };
    case "waiting": return { tone: LINK_TONE.waiting, text: "Waiting for macOS" };
    default: return { tone: LINK_TONE.not_installed, text: `Not installed${where}` };
  }
}

/** The pill word for the Presenter Link: one or two words. */
export function linkPill(status: LinkStatus | null, hasComputer: boolean): PillPart {
  if (!hasComputer) return { word: "Not installed", tone: "idle" };
  if (!status) return { word: "Checking", tone: "idle" };
  if (status.mode === null) return { word: "Not installed", tone: "idle" };
  switch (status.state) {
    case "ready": return status.can_import ? { word: "Connected", tone: "success" } : { word: "Limited", tone: "info" };
    case "outdated": return { word: "Update available", tone: "info" };
    case "needs_command": return { word: "Needs setup", tone: "warning" };
    case "not_answering": return { word: "Not answering", tone: "warning" };
    case "checking": return { word: "Checking", tone: "idle" };
    case "waiting": return { word: "Waiting for macOS", tone: "info" };
    default: return { word: "Not installed", tone: "idle" };
  }
}

/**
 * A sentence under the lines when there is something to do or know: the import blocker with its one-sentence fix, a computer that
 * answers from memory, the reason this computer's own ProPresenter folder is not usable, and the macOS prompt that is waiting for an answer.
 */
export function linkNote(status: LinkStatus | null): string | null {
  if (!status || status.mode === null) return null;
  if (importBlocked(status)) {
    const fix = status.import_blocker ?? "";
    return `StagePilot can add songs ProPresenter already has, but cannot load a missing song from ProPresenter's search. ${fix}`.trim();
  }
  if (status.stale || status.state === "waiting" || (status.mode === "local" && status.state === "not_answering")) return status.message;
  return null;
}

/** The text of the live region on the block's lines. */
export const linkLive = (api: LiveLinkLine, link: LiveLinkLine): string => `${PROPRESENTER_API_NAME}: ${api.text}. ${PRESENTER_LINK_NAME}: ${link.text}.`;

/** One status as the block writes it, in the pill (`ProPresenter API: Connected`) and in the line under the title (`ProPresenter API: Connected to ProPresenter at 192.0.2.10.`). */
export const apiWords = (rest: string): string => `${PROPRESENTER_API_NAME}: ${rest}`;
export const linkWords = (rest: string): string => `${PRESENTER_LINK_NAME}: ${rest}`;
