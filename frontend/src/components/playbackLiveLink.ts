import type { PlaybackConnection } from "../types";
import type { HelperState, LibraryStatus } from "../types";
import type { Tone } from "./ui/types";

/**
 * The words and tones of the Playback Live Link block (DESIGN.md: a dot AND words, never colour alone). Two statuses are kept apart on
 * purpose: the Playback API (what StagePilot controls Playback through) and the network script (the helper on the Playback computer that
 * lets StagePilot read each song's key, BPM and arrangement). The script is optional: a computer that has none is "Not installed", a calm
 * grey, never a warning.
 */

export interface LiveLinkLine {
  /** The dot's tone. */
  tone: Tone;
  /** The words after the label, e.g. "Connected to Playback at 10.30.0.141". */
  text: string;
}

/** The Playback API line: the backend's own sentence (`reason`), with a tone for how it stands. */
export function apiLine(status: Pick<PlaybackConnection, "reason" | "active_source" | "sources"> & { scan?: { state: string } }, fallback: string): LiveLinkLine {
  if (!status) return { tone: "idle", text: fallback };
  const text = status.reason || fallback;
  if (status.scan?.state === "scanning") return { tone: "info", text };
  if (status.active_source === "playback_api") return { tone: "success", text };
  if (status.active_source === "midi") return { tone: "midi", text };
  return { tone: "idle", text };
}

/** Whether the Playback API itself is connected (MIDI standing in does not count). */
export const apiGood = (status: Pick<PlaybackConnection, "sources"> | null | undefined): boolean => status?.sources.playback_api.connected ?? false;

const HELPER_TONE: Record<HelperState, Tone> = {
  ready: "success",
  outdated: "info",
  needs_key: "info",
  not_installed: "idle",
  forgotten: "idle",
  unreachable: "warning",
};

/** The network script line: `Ready, version 0.3.0`, `Not installed`, `Update available`, `Needs the command again`, `Not answering`, `Forgotten`. */
export function scriptLine(status: LibraryStatus | null, hasComputer: boolean): LiveLinkLine {
  if (!status) return hasComputer ? { tone: "idle", text: "Checking…" } : { tone: "idle", text: "Not installed" };
  const { state, version } = status.helper;
  const withVersion = version ? `, version ${version}` : "";
  switch (state) {
    case "ready": return { tone: HELPER_TONE.ready, text: `Ready${withVersion}` };
    case "outdated": return { tone: HELPER_TONE.outdated, text: `Update available${withVersion}` };
    case "needs_key": return { tone: HELPER_TONE.needs_key, text: "Needs the command again" };
    case "unreachable": return { tone: HELPER_TONE.unreachable, text: "Not answering" };
    case "forgotten": return { tone: HELPER_TONE.forgotten, text: "Forgotten" };
    default: return { tone: HELPER_TONE.not_installed, text: "Not installed" };
  }
}

/** The library is used in these two states only; everything the library feeds waits for one of them. */
export const scriptActive = (status: LibraryStatus | null | undefined): boolean => Boolean(status?.active);

export interface LiveLinkPillState {
  text: "Connected" | "API only" | "Script only" | "Not connected";
  tone: Tone;
}

/** The pill beside the title: both good = Connected, the API alone = API only, the script alone = Script only, neither = Not connected. */
export function pillState(api: boolean, script: boolean): LiveLinkPillState {
  if (api && script) return { text: "Connected", tone: "success" };
  if (api) return { text: "API only", tone: "success" };
  if (script) return { text: "Script only", tone: "info" };
  return { text: "Not connected", tone: "idle" };
}

/** What to say when the library has a data problem while the script itself answers (macOS has not allowed it yet, Playback's data is unreadable). */
export function scriptNote(status: LibraryStatus | null): string | null {
  if (!status?.active) return null;
  return status.state === "waiting" || status.state === "problem" ? status.message : null;
}
