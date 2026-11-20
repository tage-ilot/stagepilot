import type { AddSongsGuess, AddSongsPlan, AddSongsReport, AddSongsSong, AddSongsSongResult } from "../propresenterLink";
import { guessLine, mappingLines, ordinal, OUTCOME, songSentence, usingPhrase } from "./addSongsWording";
import type { Tone } from "./ui/types";

/**
 * The status words of the import preview, one set for the preview, the cards and the result: ONE chip (an icon and a word) and at most one
 * short phrase per song. The long sentences the engine sends are kept as the chip's hover text and for a screen reader (`detail`); the page
 * itself shows only the chip and the phrase. The words are listed, one line each, in docs/propresenter.md.
 */
export type StatusKey = "in_playlist" | "ready" | "new_copy" | "will_import" | "cant_import" | "pick_one" | "cant_map" | "no_mtid" | "check" | "skipped";

export interface StatusWords { word: string; /** A glyph that carries the state next to the word (never colour alone). */ mark: string; tone: Tone }

/** Tones are the status tokens: green done, teal for news, red only for what stops a song (here every warning and danger is red). */
export const STATUS: Record<StatusKey, StatusWords> = {
  in_playlist: { word: "In playlist", mark: "\u2713", tone: "success" },
  ready: { word: "Ready", mark: "+", tone: "info" },
  new_copy: { word: "New copy", mark: "+", tone: "info" },
  will_import: { word: "Will import", mark: "\u2193", tone: "info" },
  cant_import: { word: "Can't import", mark: "!", tone: "danger" },
  pick_one: { word: "Pick one", mark: "?", tone: "warning" },
  cant_map: { word: "Can't map", mark: "!", tone: "danger" },
  no_mtid: { word: "No mtID", mark: "!", tone: "danger" },
  check: { word: "Check", mark: "?", tone: "info" },
  skipped: { word: "Skipped", mark: "\u2212", tone: "idle" },
};

export interface SongStatus extends StatusWords {
  key: StatusKey;
  /** At most one short phrase (about six words, no sentence): what the chip alone does not say. */
  phrase: string | null;
  /** `Pick one`: the files that could be the song, one per line. */
  files: string[];
  /** The long sentences (what the page used to say): the chip's hover text and the screen-reader text. */
  detail: string;
}

const quoted = (name: string) => `\u201C${name}\u201D`;
const MAX_NAMES = 3;

/** `Interlude (3rd use): 2nd group`: one guess, short. The rule that picked it stays in `guessLine`. */
export function guessPhrase(guess: AddSongsGuess): string {
  const group = guess.group_name.trim() && guess.group_name.trim() !== guess.section.trim() ? `${quoted(guess.group_name.trim())} group` : "group";
  return `${guess.section} (${ordinal(guess.position)} use): ${ordinal(guess.group_index)} ${group}`;
}

/** The guesses of one song as one phrase: the first, and `+N` for more. */
export function guessesPhrase(guesses: readonly AddSongsGuess[]): string | null {
  if (!guesses.length) return null;
  return `${guessPhrase(guesses[0]!)}${guesses.length > 1 ? ` +${guesses.length - 1}` : ""}`;
}

/** The sections no group of the song has: `Missing: Bridge 2` (names only, a few at most). */
function missingPhrase(song: AddSongsSong): string {
  let names = song.mapping.filter((m) => m.how === "missing").map((m) => m.section.trim() || "Count Off");
  if (!names.length) {
    const found = /no group with that name in the song: (.+?)\. Nothing/.exec(song.reason ?? "");
    names = found ? found[1]!.split(", ") : [];
  }
  const unique = [...new Set(names)];
  const shown = unique.slice(0, MAX_NAMES).join(", ");
  return unique.length ? `Missing: ${shown}${unique.length > MAX_NAMES ? ` +${unique.length - MAX_NAMES}` : ""}` : "Missing sections";
}

/** What stops a song that cannot be done, as a status and the shortest cause (the full reason is the chip's hover text). */
function blockedStatus(song: AddSongsSong): { key: StatusKey; phrase: string | null } {
  const reason = song.reason ?? "";
  if (song.mapping.some((m) => m.how === "missing") || /no group with that name/i.test(reason)) return { key: "cant_map", phrase: missingPhrase(song) };
  if (/MultiTracks number|has no number|no number/i.test(reason)) return { key: "no_mtid", phrase: null };
  if (/cannot be imported from here/i.test(reason)) return { key: "cant_import", phrase: /\bMac\b/.test(reason) ? "Needs a Mac" : "Import not possible here" };
  const table: [RegExp, string][] = [
    [/rental/i, "Rental ended"],
    [/no arrangement|no sections/i, "No arrangement"],
    [/could not be read/i, "File unreadable"],
    [/no letters|too many songs/i, "Name not usable"],
    [/Playback's data is needed/i, "Needs Playback data"],
    [/does not list it yet/i, "Open the Songs library"],
  ];
  return { key: "cant_import", phrase: table.find(([pattern]) => pattern.test(reason))?.[1] ?? null };
}

/** `Playback order, “(2)”`: the new file's suffix, or its name when it does not start with the title. */
function copyPhrase(song: AddSongsSong): string {
  const file = (song.file ?? "").trim();
  const rest = file.startsWith(song.title.trim()) ? file.slice(song.title.trim().length).trim() : "";
  return `Playback order, ${quoted(rest || file || song.title)}`;
}

/** The sentences behind a status: the engine's own sentence for the song, how its sections matched, and every guess. */
export function songDetail(song: AddSongsSong): string {
  const sentence = songSentence(song, null);
  const used = song.chosen_by && song.chosen_file ? [song.notes.find((note) => note.startsWith("Used ")) ?? `Used ${song.chosen_file}.`] : [];
  const matched = mappingLines(song);
  // A guess is told once: the engine's mapping line may already say it.
  const guessed = song.guesses.map(guessLine).filter((line) => !matched.some((said) => said.includes(line.split(" (Playback")[0]!)));
  const lines = [...matched, ...guessed].map((line) => (/[.!?)]$/.test(line) ? line : `${line}.`));
  return [sentence, ...used, ...lines].join(" ");
}

/** One song of the preview: its chip, its one phrase, and (for Pick one) its files. */
export function songStatus(song: AddSongsSong, skipped = false): SongStatus {
  const detail = songDetail(song);
  const make = (key: StatusKey, phrase: string | null = null, files: string[] = []): SongStatus => ({ key, ...STATUS[key], phrase, files, detail });
  if (skipped) return make("skipped");
  if (song.action === "needs_choice") return make("pick_one", null, song.choices);
  if (song.action === "blocked") {
    const stuck = blockedStatus(song);
    return make(stuck.key, stuck.phrase);
  }
  if (song.already_in_playlist) return make("in_playlist");
  if (song.guesses.length > 0 || song.mapping.some((m) => m.how === "guessed")) return make("check", guessesPhrase(song.guesses));
  // A song that used a file without being asked (the person's pick, or the name rule) says which, in place of its usual phrase.
  const using = song.chosen_by && song.chosen_file ? usingPhrase(song.chosen_file) : null;
  switch (song.action) {
    case "reuse": return make("ready", using ?? "Copy exists");
    case "as_is": return make("ready", using ?? "Used as is");
    case "import_then_copy": return make("will_import");
    default: return make("new_copy", using ?? copyPhrase(song));
  }
}

/** Whether a song is added by the import (not stuck, not already in the playlist). */
export const isToAdd = (status: StatusKey): boolean => status === "ready" || status === "new_copy" || status === "will_import" || status === "check";

/** The counts under the heading: `1 to add · 2 in playlist · 1 can't import · 1 pick one`. Zero counts are left out, except `to add`, which is always first. */
export const summaryLine = (statuses: readonly StatusKey[]): string => summaryParts(statuses).join(" \u00B7 ");

/** The same counts as separate pieces (`1 to add`, `2 in playlist`), so a narrow screen breaks the line between two of them, never inside one. */
export function summaryParts(statuses: readonly StatusKey[]): string[] {
  const count = (test: (key: StatusKey) => boolean) => statuses.filter(test).length;
  const only = (wanted: StatusKey) => count((key) => key === wanted);
  const parts: [number, string][] = [
    [count(isToAdd), "to add"],
    [only("in_playlist"), "in playlist"],
    [only("cant_import"), "can't import"],
    [only("cant_map"), "can't map"],
    [only("no_mtid"), "no mtID"],
    [only("pick_one"), "pick one"],
  ];
  return parts.filter(([n], index) => index === 0 || n > 0).map(([n, words]) => `${n} ${words}`);
}

/** `Sunday Morning Service › Worship`: where the songs go. */
export const placeHeading = (plan: Pick<AddSongsPlan, "playlist">, header: string | null, endWords: string): string => `${plan.playlist.name} \u203A ${header ?? endWords}`;

// ---- the result ---------------------------------------------------------------------------------------------------------------

/** The word of a song's result chip. A failure says what kind when the engine said (`unmapped`), else `Can't import`. */
export function resultWord(song: Pick<AddSongsSongResult, "status" | "code">): string {
  if (song.status !== "failed") return "";
  return song.code === "unmapped" ? STATUS.cant_map.word : song.code === "wrong_song" ? "Wrong song" : song.code === "live_on_screen" ? "Live on screen" : STATUS.cant_import.word;
}

/** The first song the job stopped at: the first failure. */
export const stoppedAt = (report: Pick<AddSongsReport, "songs">): AddSongsSongResult | null => report.songs.find((song) => song.status === "failed") ?? null;

/** A line of the read-back, short: `Item 7: duration 80 \u2192 0`. Anything it does not know stays as the engine said it, without its full stop. */
export function differenceLine(text: string): string {
  const item = /^Item (\d+) "[^"]*": /.exec(text);
  const where = item ? `Item ${item[1]}` : null;
  const rest = item ? text.slice(item[0].length) : text;
  const duration = /^its duration reads (\S+) now, it was (\S+)\.$/.exec(rest);
  if (where && duration) return `${where}: duration ${duration[2]} \u2192 ${duration[1]}`;
  const kind = /^it is a (.+) now, not a (.+)\.$/.exec(rest);
  if (where && kind) return `${where}: ${kind[2]} \u2192 ${kind[1]}`;
  const name = /^it reads "(.*)" now\.$/.exec(rest);
  if (where && name) return `${where}: now ${quoted(name[1]!)}`;
  const arrangement = /^ProPresenter shows the arrangement "(.*)" instead of "(.*)"\.$/.exec(rest);
  if (where && arrangement) return `${where}: arrangement ${arrangement[2]} \u2192 ${arrangement[1]}`;
  const count = /^The playlist has (\d+) items now; (\d+) were written\.$/.exec(text);
  if (count) return `Items: ${count[2]} written \u2192 ${count[1]} now`;
  return text.replace(/\.$/, "");
}

export interface ResultHeading { text: string; tone: Tone; mark: string }

/** The result's one headline: `Done: 3 added`, `3 added, 1 differs`, `Stopped: Can't import \u201CBattle Belongs\u201D`, or the short word of the outcome. */
export function resultHeading(report: Pick<AddSongsReport, "outcome" | "songs" | "differences">): ResultHeading {
  const words = OUTCOME[report.outcome];
  const added = report.songs.filter((song) => song.status === "added").length;
  const make = (text: string): ResultHeading => ({ text, tone: words.tone, mark: words.mark });
  switch (report.outcome) {
    case "done": return make(added ? `Done: ${added} added` : "Done: nothing added");
    case "done_with_differences": return make(`${added} added, ${report.differences.length} ${report.differences.length === 1 ? "differs" : "differ"}`);
    case "stopped": {
      const at = stoppedAt(report);
      return make(at ? `Stopped: ${resultWord(at)} ${quoted(at.title)}` : words.heading);
    }
    default: return make(words.heading);
  }
}
