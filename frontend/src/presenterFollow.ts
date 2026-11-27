// Following Playback with the song's lyrics in ProPresenter: the shapes of `GET /api/v1/propresenter-follow/status` and the words of the
// screen. The backend (docs/propresenter.md, "Following Playback") decides everything; this file only says it in the operator's words.
import type { StatusWords } from "./components/addSongsStatus";
import { PRESENTER_LINK_NAME } from "./components/proPresenterLinkState";

/** What the follower is doing (`state` of the status). */
export type FollowState = "off" | "idle" | "ready" | "following" | "held" | "paused" | "not_followed";

/** The verdict code of one plan song (`songs[].code`). */
export type FollowCode =
  | "cues" | "cues_aligned" | "bars" | "cues_do_not_match" | "bars_cues_unusable" | "arrangement_changed" | "not_in_playlist" | "link_not_ready" | "switch_off"
  | "not_in_playback" | "sections_changed" | "no_sections" | "library_not_ready";

export interface FollowSong {
  /** The plan song's key (the Playback library plan's key, or the Planning Center id while Playback has not given the songs). */
  key: string;
  title: string;
  following: "cues" | "bars" | null;
  code: FollowCode;
  /** The words of the chip, as the backend says them (docs/propresenter.md lists them). */
  text: string;
  /** A second sentence that says what to do, when there is something to do (`No cues: ...`), naming the Lyrics Bus template that fits the file. */
  advice: string | null;
  /** The usual lines of text on a slide of the song's file (null until the file has been read, or when no slide has text). */
  lines_per_slide: number | null;
  /** The Playback Lyrics Bus template that fits the file (`ProPresenter 2 Line Template`), when the file's lines per slide is 1, 2 or 4. */
  template: string | null;
}

export interface FollowStatus {
  enabled: boolean;
  offset_ms: number;
  state: FollowState;
  message: string;
  song: { key: string; title: string | null } | null;
  section: { index: number; name: string } | null;
  /** ProPresenter's number of the slide StagePilot last showed (0 is its first slide, the Count Off). */
  slide: number | null;
  hold_reason: string | null;
  can_try_again: boolean;
  songs: FollowSong[];
}

/** The range the backend accepts for the timing offset. */
export const OFFSET_MIN_MS = -2000;
export const OFFSET_MAX_MS = 2000;

/** The label of the master switch. */
export const FOLLOW_LABEL = "Follow Playback in ProPresenter";
export const FOLLOW_OFF = "Off";
/** Why the switch is locked (the same words the Presenter Link section uses, and Playback's Library Link). */
export const LOCKED_LINK = `${PRESENTER_LINK_NAME} not ready`;
export const LOCKED_LIBRARY = "Playback's Library Link not ready";

/** The one sentence under a "No cues" chip: where in Playback the lyric cues are made. */
export const NO_CUES_FIX = "In Playback: Edit, the \u201C...\u201D on the song, MIDI OUT, Lyrics Bus, ProPresenter 1 Line Template, Update.";
export const NO_CUES_WORD = "No cues";

/** Why the switch cannot be used now, or null. The Presenter Link comes first (nothing is read without it), then Playback's Library Link. */
export function lockedReason(linkReady: boolean, libraryReady: boolean): string | null {
  if (!linkReady) return LOCKED_LINK;
  if (!libraryReady) return LOCKED_LIBRARY;
  return null;
}

export type LineTone = "success" | "idle" | "warning" | "danger" | "info";
export interface FollowLine { text: string; mark: string; tone: LineTone; state: FollowState | "waiting" | "error" }

/** The first sentence of a line, without the reason in brackets: what the folded header of the section has room for. */
export const shortLine = (text: string): string => text.split(/ \(|\. /)[0]!.replace(/\.$/, "");

/** The one status line. A section's own name (a blank first group is Playback's Count Off); the slide counted the way ProPresenter's window counts (from 1). */
export function followLine(status: FollowStatus | null, enabled: boolean, error: string | null): FollowLine {
  if (!enabled) return { text: FOLLOW_OFF, mark: "\u2212", tone: "idle", state: "off" };
  if (!status) return error
    ? { text: error, mark: "!", tone: "warning", state: "error" }
    : { text: "Checking\u2026", mark: "\u2026", tone: "idle", state: "waiting" };
  const song = status.song?.title?.trim() ?? "";
  switch (status.state) {
    case "off":
      // The switch was just turned on and the follower has not answered yet.
      return { text: "Turning on\u2026", mark: "\u2026", tone: "idle", state: "waiting" };
    case "following": {
      const section = status.section ? (status.section.name.trim() || "Count Off") : song;
      const slide = status.slide !== null ? `, slide ${status.slide + 1}` : "";
      return { text: `Following: ${section || "Playback"}${slide}`, mark: "\u2713", tone: "success", state: "following" };
    }
    case "ready":
      return { text: song ? `Ready: ${song} starts when Playback plays it` : "Ready: waiting for Playback to play the song", mark: "\u2212", tone: "idle", state: "ready" };
    case "held":
      return { text: status.hold_reason || status.message, mark: "!", tone: "warning", state: "held" };
    case "paused":
      return { text: status.message, mark: "!", tone: "danger", state: "paused" };
    case "not_followed":
      return { text: `Not following: ${status.message}`, mark: "!", tone: "warning", state: "not_followed" };
    case "idle":
      return { text: status.message || "Nothing to follow", mark: "\u2212", tone: "idle", state: "idle" };
    default:
      return { text: status.message || FOLLOW_OFF, mark: "\u2212", tone: "idle", state: "idle" };
  }
}

/** The mark and tone of a song's chip, by what the verdict means. The words are the backend's (`text`); the mark carries the state next to them, never colour alone. */
const CHIP: Record<FollowCode, Omit<StatusWords, "word">> = {
  cues: { mark: "\u2713", tone: "success" },
  // Followed by cues, but the file has slides Playback does not number: the first slide is worth a look.
  cues_aligned: { mark: "!", tone: "warning" },
  bars: { mark: "~", tone: "info" },
  cues_do_not_match: { mark: "!", tone: "warning" },
  bars_cues_unusable: { mark: "~", tone: "info" },
  arrangement_changed: { mark: "!", tone: "danger" },
  not_in_playlist: { mark: "!", tone: "warning" },
  link_not_ready: { mark: "\u2212", tone: "idle" },
  library_not_ready: { mark: "\u2212", tone: "idle" },
  switch_off: { mark: "\u2212", tone: "idle" },
  not_in_playback: { mark: "!", tone: "warning" },
  sections_changed: { mark: "!", tone: "warning" },
  no_sections: { mark: "?", tone: "idle" },
};

export function songChipWords(song: Pick<FollowSong, "code" | "text">): StatusWords {
  const look = CHIP[song.code] ?? { mark: "?", tone: "idle" as const };
  return { word: song.text, ...look };
}

/** A song followed by bars because Playback has no cues for it: the chip says No cues and the fix is one sentence. */
export const hasNoCues = (song: Pick<FollowSong, "code" | "advice">): boolean => song.code === "bars" && Boolean(song.advice);

/** `cues go to slide 30, file has 54 cues` -> the quiet phrase under the chip: how the song is followed anyway. */
export function songPhrase(song: Pick<FollowSong, "code">): string | null {
  if (song.code === "cues_do_not_match") return "Followed by bars";
  return null;
}
