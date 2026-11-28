import type { PlaybackConnection, PlaybackRemote, PlaybackStatusResponse } from "../types";
import type { HelperState, LibraryStatus } from "../types";
import { playbackWords } from "./playbackStatus";
import type { Tone } from "./ui/types";

/**
 * The words and tones of the Playback Live Link block (DESIGN.md: a dot AND words, never colour alone). Two statuses are kept apart on
 * purpose: the Playback API (what StagePilot controls Playback through) and the Library Link (the helper on the Playback computer that
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
  const text = status.reason ? playbackWords(status.reason) : fallback;
  if (status.scan?.state === "scanning") return { tone: "info", text };
  if (status.active_source === "playback_api") return { tone: "success", text };
  if (status.active_source === "midi") return { tone: "midi", text };
  return { tone: "idle", text };
}

export const REMOTE_SWITCHING = "The Library Link is switching on Playback's Remote Connections. This takes a few seconds.";
export const REMOTE_NEEDS_PERMISSION = "The Library Link can switch on Playback's Remote Connections by itself once this Mac allows it. At the Playback computer open System Settings > Privacy & Security > Accessibility and turn on \"Playback Library\". Until then turn on Playback > menu > Playback Remote > Allow Remote Connections by hand.";
export const REMOTE_USER_BUSY = "Playback is being used; the Library Link will switch on Remote Connections when it is quiet.";
export const REMOTE_NOT_RUNNING = "Playback is not open on its computer.";
const REMOTE_BY_HAND = "Turn on Allow Remote Connections in Playback by hand.";

/** What the Library Link says about Playback's Remote Connections, in the operator's words; null when it adds nothing (`on`, or `off` with no permission to switch). */
export function remoteWords(remote: PlaybackRemote): string | null {
  switch (remote.state) {
    case "switching":
    case "off": return remote.auto ? REMOTE_SWITCHING : null;
    case "needs_permission": return REMOTE_NEEDS_PERMISSION;
    case "user_busy": return REMOTE_USER_BUSY;
    case "playback_not_running": return REMOTE_NOT_RUNNING;
    case "failed": {
      const said = remote.message?.trim();
      return said ? `${/[.!?]$/.test(said) ? said : `${said}.`} ${REMOTE_BY_HAND}` : REMOTE_BY_HAND;
    }
    default: return null;
  }
}

/**
 * The Playback API line, with the Library Link's explanation in place of "turn on Remote Connections" when the Library Link is working and says
 * what it is doing about it. Only the sentence for a Playback that refuses or does not answer is replaced (the backend's `last_error` says "remote
 * connections may be off"); a Playback that is connected, being searched for, or unreachable for another reason keeps its own words. The dot's
 * tone and every verdict stay as they were.
 */
export function withRemoteWords(line: LiveLinkLine, playback: Pick<PlaybackStatusResponse, "last_error" | "sources" | "scan"> | null | undefined, library: LibraryStatus | null | undefined): LiveLinkLine {
  const remote = library?.active ? library.helper.playback_remote : undefined;
  if (!remote || !playback || playback.sources.playback_api.connected || playback.scan?.state === "scanning") return line;
  if (!/remote connections may be off/i.test(playback.last_error ?? "")) return line;
  const words = remoteWords(remote);
  return words ? { ...line, text: words } : line;
}

/** Whether the Playback API itself is connected (MIDI standing in does not count). */
export const apiGood = (status: Pick<PlaybackConnection, "sources"> | null | undefined): boolean => status?.sources.playback_api.connected ?? false;

const HELPER_TONE: Record<HelperState, Tone> = {
  ready: "success",
  outdated: "info",
  needs_key: "warning",
  not_installed: "idle",
  unreachable: "warning",
};

/** The Library Link line: `Ready, version 0.3.0`, `Not installed`, `Update available`, `Needs the command again`, `Not answering`. */
export function scriptLine(status: LibraryStatus | null, hasComputer: boolean): LiveLinkLine {
  if (!status) return hasComputer ? { tone: "idle", text: "Checking…" } : { tone: "idle", text: "Not installed" };
  const { state, version } = status.helper;
  const withVersion = version ? `, version ${version}` : "";
  switch (state) {
    case "ready": return { tone: HELPER_TONE.ready, text: `Ready${withVersion}` };
    case "outdated": return { tone: HELPER_TONE.outdated, text: `Update available${withVersion}` };
    case "needs_key": return { tone: HELPER_TONE.needs_key, text: "Needs the command again" };
    case "unreachable": return { tone: HELPER_TONE.unreachable, text: "Not answering" };
    default: return { tone: HELPER_TONE.not_installed, text: "Not installed" };
  }
}

/** The library is used in these two states only; everything the library feeds waits for one of them. */
export const scriptActive = (status: LibraryStatus | null | undefined): boolean => Boolean(status?.active);

export interface PillPart {
  /** One or two words after the label: "Connected", "Not installed". */
  word: string;
  tone: Tone;
}

/**
 * The two statuses in the pill beside the title: "Playback API: Connected - Library Link: Connected". One short word each (the long
 * sentence rides in the strip under the fields and in the tooltip). The script is "Connected" when it answers with the key, even if a
 * newer one is available (that is said as "Update available").
 */
export function pillParts(api: { good: boolean; line: LiveLinkLine }, status: LibraryStatus | null, hasComputer: boolean): { api: PillPart; script: PillPart } {
  const apiPart: PillPart = api.good
    ? { word: "Connected", tone: "success" }
    : api.line.tone === "info" ? { word: "Searching", tone: "info" } : api.line.tone === "midi" ? { word: "MIDI only", tone: "midi" } : { word: "Not connected", tone: "idle" };
  const state = status?.helper.state;
  const script: PillPart = !status
    ? { word: hasComputer ? "Checking" : "Not installed", tone: "idle" }
    : state === "ready" ? { word: "Connected", tone: HELPER_TONE.ready }
      : state === "outdated" ? { word: "Update available", tone: HELPER_TONE.outdated }
        : state === "needs_key" ? { word: "Needs setup", tone: HELPER_TONE.needs_key }
          : state === "unreachable" ? { word: "Not answering", tone: HELPER_TONE.unreachable }
            : { word: "Not installed", tone: HELPER_TONE.not_installed };
  return { api: apiPart, script };
}

/** What to say when the library has a data problem while the script itself answers (macOS has not allowed it yet, Playback's data is unreadable). */
export function scriptNote(status: LibraryStatus | null): string | null {
  if (!status?.active) return null;
  return status.state === "waiting" || status.state === "problem" ? status.message : null;
}
