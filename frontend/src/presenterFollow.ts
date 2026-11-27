// Following Playback with the song's lyrics in ProPresenter: the shapes of `GET /api/v1/propresenter-follow/status` and the words of the
// screen. The backend (docs/propresenter.md, "Following Playback") decides everything; this file only says it in the operator's words.
import type { StatusWords } from "./components/addSongsStatus";

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

/** The name of the feature, the label of its master switch and the title of its section. */
export const FOLLOW_LABEL = "Auto-Presenter";
export const FOLLOW_OFF = "Off";
/** Where this browser remembers whether the Auto-Presenter section is open. */
export const AUTO_PRESENTER_OPEN_KEY = "stagepilot.presentation.auto-presenter-open";

/** The ONE chip of a song followed by bars for want of cues (the backend's long words stay in its hover text). */
export const NO_CUES_WORD = "No cues: following by bars";
/** The mark of a line with nothing to follow now (off, ready, idle, needs a link): a hollow circle, never a lone dash that reads like a typo. */
export const MARK_IDLE = "\u25CB";

/** Playback's Library Link: answering with current data, answering with an old copy, or not connected at all. */
export type LibraryLinkState = "ready" | "stale" | "missing";
export const libraryLinkState = (status: { active: boolean; stale: boolean } | null | undefined): LibraryLinkState => (!status?.active ? "missing" : status.stale ? "stale" : "ready");

/**
 * Why the Auto-Presenter cannot be used now: which link it needs (`need`, said after "Needs" or "Waiting for") and where to set it up
 * (`where`). The reason is said ONCE; the song rows show only their titles while it holds.
 */
export interface FollowLock { need: string; where: string }

const LINK_WHERE = "Set it up in Configuration \u2192 Presentation (Presenter Link setup).";
const LIBRARY_WHERE = "Set it up in Configuration \u2192 Playback (Library Link setup).";

/**
 * Whether the Auto-Presenter is locked, and why in ONE plain reason: it needs the Presenter Link AND Playback's Library Link, the
 * latter with current data. Nothing is read without the Presenter Link, so its chip comes first when both are missing.
 */
export function followLock(linkReady: boolean, library: LibraryLinkState): FollowLock | null {
  if (!linkReady && library !== "ready") return { need: "the Presenter Link and the Playback Library Link", where: "Set up the Presenter Link in Configuration \u2192 Presentation and the Library Link in Configuration \u2192 Playback." };
  if (!linkReady) return { need: "the Presenter Link", where: LINK_WHERE };
  if (library === "stale") return { need: "current data from the Playback Library Link", where: "It has not answered lately: check it in Configuration \u2192 Playback (Library Link setup)." };
  if (library === "missing") return { need: "the Playback Library Link", where: LIBRARY_WHERE };
  return null;
}

/** The locked section's one line, the same in its header and inside it: `Needs the Playback Library Link`, or `Waiting for ...` when the switch was already saved as on. */
export const lockWords = (lock: FollowLock, enabled: boolean): string => `${enabled ? "Waiting for" : "Needs"} ${lock.need}`;

export type LineTone = "success" | "idle" | "warning" | "danger" | "info";
export interface FollowLine { text: string; mark: string; tone: LineTone; state: FollowState | "waiting" | "error" }

/** The first sentence of a line, without the reason in brackets: what the folded header of the section has room for. */
export const shortLine = (text: string): string => text.split(/ \(|\. /)[0]!.replace(/\.$/, "");

/** The one status line. A section's own name (a blank first group is Playback's Count Off); the slide counted the way ProPresenter's window counts (from 1). */
export function followLine(status: FollowStatus | null, enabled: boolean, error: string | null): FollowLine {
  if (!enabled) return { text: FOLLOW_OFF, mark: MARK_IDLE, tone: "idle", state: "off" };
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
      return { text: song ? `Ready: ${song} starts when Playback plays it` : "Ready: waiting for Playback to play the song", mark: MARK_IDLE, tone: "idle", state: "ready" };
    case "held":
      return { text: status.hold_reason || status.message, mark: "!", tone: "warning", state: "held" };
    case "paused":
      return { text: status.message, mark: "!", tone: "danger", state: "paused" };
    case "not_followed":
      return { text: `Not following: ${status.message}`, mark: "!", tone: "warning", state: "not_followed" };
    case "idle":
      return { text: status.message || "Nothing to follow", mark: MARK_IDLE, tone: "idle", state: "idle" };
    default:
      return { text: status.message || FOLLOW_OFF, mark: MARK_IDLE, tone: "idle", state: "idle" };
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

export function songChipWords(song: Pick<FollowSong, "code" | "text"> & Partial<Pick<FollowSong, "advice">>): StatusWords {
  // A song followed by bars for want of cues: ONE warning chip (the backend's `Following by bars (no Playback cues)` is its hover text).
  if (hasNoCues(song)) return { word: NO_CUES_WORD, mark: "!", tone: "warning" };
  const look = CHIP[song.code] ?? { mark: "?", tone: "idle" as const };
  return { word: song.text, ...look };
}

/** A song followed by bars because Playback has no cues for it: its ONE chip says `No cues: following by bars` and the fix is one sentence under it. */
export const hasNoCues = (song: Pick<FollowSong, "code"> & Partial<Pick<FollowSong, "advice">>): boolean => song.code === "bars" && Boolean(song.advice);

/**
 * The one sentence that says where in Playback the lyric cues are made, naming the Lyrics Bus template the song file needs (the status's
 * `template`: 1, 2 or 4 lines per slide; a file with another number says to pick the one that matches).
 */
export function templateFix(song: Pick<FollowSong, "template">): string {
  return `In Playback: Edit, the \u201C...\u201D on the song, MIDI OUT, Lyrics Bus, ${song.template ?? "a ProPresenter Line Template that matches the file"}, Update.`;
}

/** A song followed by bars because its cues are missing or wrong has a fix: pick the template that fits the file. */
export const hasFix = (song: Pick<FollowSong, "code" | "advice">): boolean => (song.code === "bars" || song.code === "cues_do_not_match") && Boolean(song.advice);

/** `cues go to slide 30, file has 54 cues` -> the quiet phrase under the chip: how the song is followed anyway. */
export function songPhrase(song: Pick<FollowSong, "code">): string | null {
  if (song.code === "cues_do_not_match") return "Followed by bars";
  return null;
}
