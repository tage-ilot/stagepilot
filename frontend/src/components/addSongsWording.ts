import type {
  AddSongsGuess,
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
import { PRESENTER_LINK_NAME } from "./proPresenterLinkState";

/**
 * The words of the Presenter Link section, in one place (church-operator wording: short, plain, no jargon). The section says as little as
 * it can: only what a person must act on. What the button does is explained in docs/propresenter.md, not on the page.
 */

export const IMPORT_SONGS = "Import songs to ProPresenter";
/** The button's words while it makes the preview (and, when nothing stops it, starts the import). */
export const IMPORTING = "Importing\u2026";
/** The last option of the Section box: after everything in the playlist. */
export const END_OF_PLAYLIST = "End of Playlist";
/** The small chip on a card that is a Planning Center name (Playback has not given the songs). */
export const PLANNING_CENTER = "Planning Center";
/** The count for the cards, read out politely and never drawn (a screen-reader line): `3 of 4 selected`. */
export const selectedWords = (selected: number, of: number): string => `${selected} of ${of} selected`;
/** The engine's own line for a header name the playlist has twice: StagePilot does not guess which one. */
export const twoHeadersLine = (header: string): string => `More than one header in this playlist is named \u201C${header.trim()}\u201D. Pick another place.`;
/** The chip on a Planning Center card ProPresenter does not have, and its one short phrase; the full sentence is the chip's hover text. */
export const NOT_IN_PROPRESENTER = "Not in ProPresenter";
export const NOT_IN_PROPRESENTER_WHY = "Playback data is needed to import it.";
export const NOT_IN_PROPRESENTER_PHRASE = "Needs Playback data";
export const SEVERAL_IN_PROPRESENTER = "Pick one";
export const SEVERAL_IN_PROPRESENTER_WHY = "More than one song in ProPresenter has this title.";
export const SKIP_AND_IMPORT = "Skip those and import the rest";
/** A Playback song with no setlist number (nothing to name it by): the chip on its card, its phrase, and the hover text. */
export const NO_NUMBER = "Can't import";
export const NO_NUMBER_PHRASE = "No Playback number";
export const NO_NUMBER_WHY = "Playback gave this song no number, so it cannot be imported.";
export const NO_PLAN_SONGS = "The loaded plan has no songs yet.";

/** The Auto-Presenter's one sentence for plan songs with no Playback song behind them. */
export const unpairedSentence = (titles: readonly string[]): string => `Not followed: ${titles.join(", ")}. ${titles.length === 1 ? "It is" : "They are"} not in Playback's setlist, or not paired with a Playback song yet. Pair a song by hand in the Song order section of Configuration \u2192 Playback.`;
export const CHECK_IN_PROPRESENTER = "Check in ProPresenter:";
export const OPEN_LIBRARY_ONCE = "Open the Playback library in ProPresenter, then Try again";
/** The one line at the top when a song is playing (the whole job waits). */
export const PLAYING_WORDS = "Song playing: wait";
/** On a preview that no longer matches what is ticked. */
export const STALE_WORDS = "Out of date: press Import again";
export const LIVE_ON_SCREEN_WORDS = "Live on screen: clear it, then try again";
export const LINK_NOT_READY_WORDS = `${PRESENTER_LINK_NAME} not ready`;

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

// ---- the Presenter Link's header: whether the songs can be imported ---------------------------------------------------------
/** The header's words while a song plays (the whole import waits). */
export const NOT_WHILE_PLAYING = "Not while a song plays";
export const NO_SONGS_IN_PLAN = "No songs in the plan";
export const NO_SONGS_CAN_BE_IMPORTED = "No songs can be imported";
/** The header's one word for a problem (the full words are inside the section, which opens). */
export const PROBLEM_WORDS = "Problem";

export interface PresenterSummaryNumbers {
  /** The cards on the list. */
  songs: number;
  /** The cards that can be imported: the ones that are not the grey dashed `Can't import` kind. */
  ready: number;
  /** A song is playing in Playback. */
  playing?: boolean;
  /** The preview is being made, or the import is running. */
  importing?: boolean;
  /** How the job that just ended was headed (`Done: 4 added`); `null` while none has ended. */
  result?: string | null;
  /** A problem is shown inside the section. */
  problem?: boolean;
  /** The one short word a preview that stopped says (`2 can't be imported yet`); `null` when it did not stop. */
  reason?: string | null;
}

/**
 * The one short line beside the Presenter Link's heading, folded or not: a status, never a sentence. Work in progress, a result or a
 * problem comes first (the section is open for them too); then the songs: `4 songs ready to import`, `3 of 4 songs can be imported`,
 * `No songs can be imported`, `Not while a song plays`, `No songs in the plan`. The playlist is not mentioned: its own pill says so inside.
 */
export function presenterSummary({ songs, ready, playing = false, importing = false, result = null, problem = false, reason = null }: PresenterSummaryNumbers): string {
  if (problem) return PROBLEM_WORDS;
  if (result) return result;
  if (importing) return IMPORTING;
  if (songs <= 0) return NO_SONGS_IN_PLAN;
  if (playing) return NOT_WHILE_PLAYING;
  if (reason) return reason;
  if (ready <= 0) return NO_SONGS_CAN_BE_IMPORTED;
  if (ready < songs) return `${ready} of ${songs} songs can be imported`;
  return `${plural(songs, "song")} ready to import`;
}

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

/**
 * What stops the one button after the preview, short: a blocked place, songs that cannot be done, nothing to add. `null` when the preview
 * is clean and the songs can go ahead. `full` adds what the person can do about it. The preview's own chips say which songs and why.
 */
export function stuckReason(plan: Pick<AddSongsPlan, "songs" | "stuck" | "to_add" | "can_apply"> & { playlist: Pick<AddSongsPlan["playlist"], "blocked"> }, full: boolean): string | null {
  if (plan.playlist.blocked) return "Playlist is blocked";
  if (plan.stuck > 0) {
    const lead = `${plan.stuck} can't be imported yet`;
    if (!full) return lead;
    return plan.to_add > 0 ? `${lead}: skip them, or fix them and try again` : `${lead}: fix them, or select other songs`;
  }
  if (!plan.can_apply) return "Nothing to import: all in playlist";
  return null;
}

/** A song the plan cannot do: it must be skipped for the others to go ahead. */
export const isStuck = (song: Pick<AddSongsSong, "action">): boolean => song.action === "blocked" || song.action === "needs_choice";

const quoted = (name: string) => `\u201C${name}\u201D`;

/** The small button that brings the file buttons back on a card that used a file without asking. */
export const CHANGE_FILE = "Change";
/** `Songs / Firm Foundation (Live).pro` is `Firm Foundation (Live)`: the file's name as a person reads it. */
export const fileWords = (file: string): string => {
  const at = file.indexOf(" / ");
  return (at >= 0 ? file.slice(at + 3) : file).replace(/\.pro$/i, "");
};
/** The files of a song as the buttons say them: the file name alone, and `Songs / Name` only for a name that two libraries share. */
export function fileLabels(files: readonly string[]): string[] {
  const names = files.map(fileWords);
  return files.map((file, index) => (names.filter((name) => name === names[index]).length > 1 ? file.replace(/\.pro$/i, "") : names[index]!));
}
/** The quiet phrase of a song that used a file without being asked (a pick or the name rule): `Using “Firm Foundation (Live)”`. */
export const usingPhrase = (file: string): string => `Using ${quoted(fileWords(file))}`;

/** What the preview says will happen to one song, as one plain sentence. */
export function songSentence(song: AddSongsSong, library: Pick<LibrarySong, "sections"> | null | undefined): string {
  const file = song.file ?? song.title;
  const inList = song.already_in_playlist ? " It is already in this playlist, so it is not added again." : "";
  switch (song.action) {
    case "reuse":
      return `Already in ProPresenter as ${quoted(file)}.${inList}`;
    case "as_is":
      return `Added as it is in ProPresenter as ${quoted(file)}: no copy, no new arrangement, no key change.${inList}`;
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

/** `1st`, `2nd`, `3rd`, `4th`, `11th`, `21st`. */
export function ordinal(n: number): string {
  const rest = n % 100;
  const suffix = rest >= 11 && rest <= 13 ? "th" : ({ 1: "st", 2: "nd", 3: "rd" } as Record<number, string>)[n % 10] ?? "th";
  return `${n}${suffix}`;
}

/** One guess as a short plain phrase: `Interlude (3rd use) uses the 2nd \u201CInterlude\u201D group`, and which rule picked it when it is not the plain one. */
export function guessLine(guess: AddSongsGuess): string {
  const group = guess.group_name.trim() ? `${quoted(guess.group_name)} group` : "blank-named group";
  const base = `${guess.section} (${ordinal(guess.position)} use) uses the ${ordinal(guess.group_index)} ${group}`;
  switch (guess.rule) {
    case "last_reused":
      return `${base} (Playback uses it once more than the song has groups, so the last group is used again)`;
    case "repeat_in_turn":
      return `${base} (Playback uses it more often than the song has groups, so the groups are used in turn)`;
    default:
      return base;
  }
}

/** After an import: the short note under a song whose sections were guessed. `null` when nothing was guessed. */
export function guessNote(guesses: readonly AddSongsGuess[] | null | undefined): { heading: string; lines: string[] } | null {
  if (!guesses?.length) return null;
  return { heading: CHECK_IN_PROPRESENTER, lines: guesses.map(guessLine) };
}

/** How well the song's sections matched the song's groups, and every sentence a match is not exact about. */
export function mappingLines(song: Pick<AddSongsSong, "mapping">): string[] {
  const total = song.mapping.length;
  if (!total) return [];
  const exact = song.mapping.filter((m) => m.how === "full" || m.how === "only").length;
  const same = song.mapping.filter((m) => m.how === "identical").length;
  const guessed = song.mapping.filter((m) => m.how === "guessed").length;
  const unclear = total - exact - same - guessed;
  const lines: string[] = [];
  if (!same && !guessed && !unclear) lines.push(`All ${plural(total, "section")} matched exactly to the song's own groups.`);
  else {
    const parts = [`${exact} of ${plural(total, "section")} matched exactly`];
    if (same) parts.push(`${same} to groups that say the same thing`);
    if (guessed) parts.push(`${guessed} guessed (check ${guessed === 1 ? "it" : "them"} in ProPresenter)`);
    if (unclear) parts.push(`${unclear} not matched`);
    lines.push(`${parts.join(", ")}.`);
  }
  for (const mapping of song.mapping) if (mapping.line) lines.push(mapping.line);
  return lines;
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

/** One short word per step of the job. */
export const STEP_WORDS: Record<AddSongsStep, string> = {
  check: "ProPresenter",
  backup: "Backup",
  library: "Library",
  import: "Importing",
  copy: "Copy",
  load: "Loading",
  playlist: "Playlist",
  read_back: "Check",
};

export const STEP_STATE: Record<AddSongsStepState, { word: string; mark: string }> = {
  running: { word: "Working", mark: "\u2026" },
  done: { word: "Done", mark: "\u2713" },
  failed: { word: "Failed", mark: "\u2715" },
  note: { word: "Note", mark: "i" },
};

export const SONG_STATUS: Record<AddSongsSongStatus, { word: string; mark: string; tone: Tone }> = {
  added: { word: "Added", mark: "\u2713", tone: "success" },
  already_there: { word: "In playlist", mark: "\u2713", tone: "success" },
  skipped: { word: "Skipped", mark: "\u2212", tone: "idle" },
  failed: { word: "Failed", mark: "\u2715", tone: "danger" },
  not_tried: { word: "Not tried", mark: "\u2212", tone: "idle" },
  not_loaded: { word: "Not loaded", mark: "!", tone: "warning" },
};

export interface OutcomeWords {
  heading: string;
  tone: Tone;
  mark: string;
}

/** The headline of the result, short; `resultHeading` adds the counts (`Done: 3 added`). */
export const OUTCOME: Record<AddSongsOutcome, OutcomeWords> = {
  done: { heading: "Done", tone: "success", mark: "\u2713" },
  done_with_differences: { heading: "Differs", tone: "warning", mark: "!" },
  needs_library_open: { heading: "Not loaded yet", tone: "warning", mark: "!" },
  stopped: { heading: "Stopped", tone: "warning", mark: "!" },
  song_playing: { heading: PLAYING_WORDS, tone: "warning", mark: "!" },
  live_on_screen: { heading: LIVE_ON_SCREEN_WORDS, tone: "warning", mark: "!" },
  playlist_refused: { heading: "ProPresenter refused it", tone: "danger", mark: "\u2715" },
  playlist_failed: { heading: "Playlist not changed", tone: "danger", mark: "\u2715" },
  backup_failed: { heading: "Stopped: no backup", tone: "danger", mark: "\u2715" },
  not_reachable: { heading: "ProPresenter not answering", tone: "danger", mark: "\u2715" },
  nothing_to_do: { heading: "Nothing to import", tone: "info", mark: "i" },
};

/** The outcomes after which a fresh preview is the sensible next step (files made before are reused, so it is safe to repeat). */
export const canTryAgain = (outcome: AddSongsOutcome): boolean => !(outcome === "done" || outcome === "done_with_differences" || outcome === "nothing_to_do");

/** The file name of a backup, for a person to look for (the folder is beside the settings file). */
export const backupName = (backup: string | null): string | null => (backup ? backup.split(/[\\/]/).pop() ?? backup : null);

/** One sentence for a screen reader when the job's newest event changes. */
export const liveText = (title: string | null, text: string): string => (title ? `${title}: ${text}` : text);

/** The report in one line of counts: `3 added \u00B7 1 skipped`. */
export function resultCounts(report: Pick<AddSongsReport, "songs">): string {
  const count = (status: AddSongsSongStatus) => report.songs.filter((song) => song.status === status).length;
  return [
    count("added") ? `${count("added")} added` : null,
    count("already_there") ? `${count("already_there")} in playlist` : null,
    count("skipped") ? `${count("skipped")} skipped` : null,
    count("failed") ? `${count("failed")} failed` : null,
    count("not_loaded") ? `${count("not_loaded")} not loaded` : null,
    count("not_tried") ? `${count("not_tried")} not tried` : null,
  ].filter(Boolean).join(" \u00B7 ");
}

/** A sentence for a screen reader after a job: the guesses to check, per song. Empty when nothing was guessed. */
export function guessAnnouncement(report: Pick<AddSongsReport, "songs"> | null): string {
  const notes = (report?.songs ?? []).flatMap((song) => {
    const note = guessNote(song.guesses);
    return note ? [`${song.title}: ${note.lines.join("; ")}.`] : [];
  });
  return notes.length ? `${CHECK_IN_PROPRESENTER} ${notes.join(" ")}` : "";
}
