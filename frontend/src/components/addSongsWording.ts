import type {
  AddSongsAction,
  AddSongsOutcome,
  AddSongsPlan,
  AddSongsReport,
  AddSongsSong,
  AddSongsSongStatus,
  AddSongsStep,
  AddSongsStepState,
  ProPresenterPlaylist,
} from "../propresenterLink";
import type { LibrarySong } from "../types";
import type { Tone } from "./ui/types";

/** The words of the "Add to ProPresenter" window, in one place (church-operator wording: short, plain, no jargon). */

export const ADD_TO_PROPRESENTER = "Add to ProPresenter";
export const NOTHING_CHANGES = "Nothing in ProPresenter changes until you press Add songs.";
export const KEY_NOTE = "ProPresenter does not show the key for these songs; it is written into the copy and shown here.";
export const OTHER_ITEMS_STAY = "Other items stay as they are.";
export const OPEN_LIBRARY_ONCE = "Open the Playback library in ProPresenter once, then press Try again.";
export const PLAYING_WORDS = "A song is playing. Wait until it has ended, then try again.";
export const LINK_NOT_READY_WORDS = "The ProPresenter Link is not ready. Check it in the Presentation panel.";
export const AT_THE_END = "At the end";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** The place picker's option for a header. */
export const afterHeaderLabel = (header: string) => `After \u201C${header}\u201D`;

/** `Folder / Name`, or the name alone. */
export const playlistLabel = (playlist: Pick<ProPresenterPlaylist, "name" | "folder">) =>
  playlist.folder ? `${playlist.folder} / ${playlist.name}` : playlist.name;

/** The names of Playback's sections in playing order, as many as fit one line: `Verse 1, Chorus, Verse 2, Chorus, \u2026 (14 sections)`. */
export function orderWords(song: Pick<AddSongsSong, "sections">, library: Pick<LibrarySong, "sections"> | null | undefined, spoken = 4): string {
  const names = (library?.sections ?? []).map((section) => section.name.trim() || "Count Off");
  if (!names.length) return plural(song.sections, "section");
  if (names.length <= spoken) return names.join(", ");
  return `${names.slice(0, spoken).join(", ")}, \u2026 (${names.length} sections)`;
}

export interface SongStamp {
  word: string;
  /** A glyph that carries the state next to the word (never colour alone). */
  mark: string;
  tone: "success" | "info" | "warning";
}

/** The stamp on a preview row: one word and a mark. */
export const ACTION_STAMP: Record<AddSongsAction, SongStamp> = {
  reuse: { word: "Already there", mark: "\u2713", tone: "success" },
  copy: { word: "New copy", mark: "+", tone: "info" },
  import_then_copy: { word: "Load first", mark: "\u2193", tone: "info" },
  needs_choice: { word: "Needs a choice", mark: "?", tone: "warning" },
  blocked: { word: "Blocked", mark: "!", tone: "warning" },
};

/** A song the plan cannot do: it must be skipped for the others to go ahead. */
export const isStuck = (song: Pick<AddSongsSong, "action">): boolean => song.action === "blocked" || song.action === "needs_choice";

const quoted = (name: string) => `\u201C${name}\u201D`;

/** What the preview says will happen to one song, as one plain sentence. */
export function songSentence(song: AddSongsSong, library: Pick<LibrarySong, "sections"> | null | undefined): string {
  const file = song.file ?? song.title;
  const inList = song.already_in_playlist ? " It is already in this playlist, so it is not added again." : "";
  switch (song.action) {
    case "reuse":
      return `Already in ProPresenter as ${quoted(file)}.${inList}`;
    case "copy":
      return `New copy ${quoted(file)} with Playback's order (${orderWords(song, library)}).${inList}`;
    case "import_then_copy":
      return `Will be loaded from ProPresenter search first${song.import_as ? ` (as ${quoted(song.import_as)})` : ""}, then a new copy ${quoted(file)} with Playback's order (${orderWords(song, library)}).`;
    case "needs_choice":
      return `Needs a choice: ${song.reason ?? "ProPresenter has more than one file that could be this song. StagePilot does not guess."}${song.choices.length ? ` Files: ${song.choices.map(quoted).join(", ")}.` : ""}`;
    default:
      return `Blocked: ${song.reason ?? "StagePilot cannot make this song."}`;
  }
}

/** How well the song's sections matched the song's groups, and every sentence a match is not exact about. */
export function mappingLines(song: Pick<AddSongsSong, "mapping">): string[] {
  const total = song.mapping.length;
  if (!total) return [];
  const exact = song.mapping.filter((m) => m.how === "full" || m.how === "only").length;
  const same = song.mapping.filter((m) => m.how === "identical").length;
  const unclear = total - exact - same;
  const lines: string[] = [];
  if (!same && !unclear) lines.push(`All ${plural(total, "section")} matched exactly to the song's own groups.`);
  else {
    const parts = [`${exact} of ${plural(total, "section")} matched exactly`];
    if (same) parts.push(`${same} to groups that say the same thing`);
    if (unclear) parts.push(`${unclear} not matched`);
    lines.push(`${parts.join(", ")}.`);
  }
  for (const mapping of song.mapping) if (mapping.line) lines.push(mapping.line);
  return lines;
}

/** `Adds 4 songs after Worship in Sunday Morning Service. Other items stay as they are.` */
export function playlistSentence(plan: Pick<AddSongsPlan, "playlist" | "to_add">, header: string | null): string {
  const { playlist, to_add: count } = plan;
  if (count === 0) return `Adds nothing to ${playlist.name}. ${OTHER_ITEMS_STAY}`;
  const where = header ? `after ${header}` : "at the end";
  return `Adds ${plural(count, "song")} ${where} in ${playlist.name}. ${OTHER_ITEMS_STAY}`;
}

/** The neighbours the new songs land between, when the plan names them. */
export function neighbours(playlist: Pick<AddSongsPlan["playlist"], "before" | "after">): string | null {
  const { before, after } = playlist;
  if (before && after) return `They go between ${quoted(before)} and ${quoted(after)}.`;
  if (before) return `They go after ${quoted(before)}, the last item.`;
  if (after) return `They go before ${quoted(after)}, the first item.`;
  return null;
}

// ---- the job ----------------------------------------------------------------------------------------------------------

export const STEP_WORDS: Record<AddSongsStep, string> = {
  check: "Checking ProPresenter",
  backup: "Saving a copy of the playlist",
  library: "The Playback library",
  import: "Loading from ProPresenter search",
  copy: "Making the copy",
  load: "Waiting for ProPresenter to load it",
  playlist: "Writing the playlist",
  read_back: "Checking the playlist",
};

export const STEP_STATE: Record<AddSongsStepState, { word: string; mark: string }> = {
  running: { word: "Working", mark: "\u2026" },
  done: { word: "Done", mark: "\u2713" },
  failed: { word: "Failed", mark: "\u2715" },
  note: { word: "Note", mark: "i" },
};

export const SONG_STATUS: Record<AddSongsSongStatus, { word: string; mark: string; tone: Tone }> = {
  added: { word: "Added", mark: "\u2713", tone: "success" },
  already_there: { word: "Was already there", mark: "\u2713", tone: "success" },
  skipped: { word: "Skipped", mark: "\u2212", tone: "idle" },
  failed: { word: "Failed", mark: "\u2715", tone: "danger" },
  not_tried: { word: "Not tried", mark: "\u2212", tone: "idle" },
  not_loaded: { word: "Not loaded yet", mark: "!", tone: "warning" },
};

export interface OutcomeWords {
  heading: string;
  tone: Tone;
  mark: string;
}

/** The headline of the result. The sentence under it is the backend's own (`report.message`). */
export const OUTCOME: Record<AddSongsOutcome, OutcomeWords> = {
  done: { heading: "Added", tone: "success", mark: "\u2713" },
  done_with_differences: { heading: "Added, but ProPresenter shows something different", tone: "warning", mark: "!" },
  needs_library_open: { heading: "ProPresenter has not loaded the new songs yet", tone: "warning", mark: "!" },
  stopped: { heading: "Stopped", tone: "warning", mark: "!" },
  song_playing: { heading: "Stopped: a song is playing", tone: "warning", mark: "!" },
  playlist_refused: { heading: "ProPresenter did not accept the change", tone: "danger", mark: "\u2715" },
  playlist_failed: { heading: "The playlist could not be changed", tone: "danger", mark: "\u2715" },
  backup_failed: { heading: "Nothing was changed: the playlist could not be saved first", tone: "danger", mark: "\u2715" },
  not_reachable: { heading: "ProPresenter did not answer", tone: "danger", mark: "\u2715" },
  nothing_to_do: { heading: "Nothing to add", tone: "info", mark: "i" },
};

/** The outcomes after which a fresh preview is the sensible next step (files made before are reused, so it is safe to repeat). */
export const canTryAgain = (outcome: AddSongsOutcome): boolean => !(outcome === "done" || outcome === "done_with_differences" || outcome === "nothing_to_do");

/** The file name of a backup, for a person to look for (the folder is beside the settings file). */
export const backupName = (backup: string | null): string | null => (backup ? backup.split(/[\\/]/).pop() ?? backup : null);

/** One sentence for a screen reader when the job's newest event changes. */
export const liveText = (title: string | null, text: string): string => (title ? `${title}: ${text}` : text);

/** What the report says in one line for the counts: `3 added, 1 skipped`. */
export function resultCounts(report: Pick<AddSongsReport, "songs">): string {
  const count = (status: AddSongsSongStatus) => report.songs.filter((song) => song.status === status).length;
  return [
    count("added") ? `${count("added")} added` : null,
    count("already_there") ? `${count("already_there")} already there` : null,
    count("skipped") ? `${count("skipped")} skipped` : null,
    count("failed") ? `${count("failed")} failed` : null,
    count("not_loaded") ? `${count("not_loaded")} not loaded yet` : null,
    count("not_tried") ? `${count("not_tried")} not tried` : null,
  ].filter(Boolean).join(", ");
}
