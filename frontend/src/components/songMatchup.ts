import type { LibraryPlan, LibraryPlanSong, LibrarySong, MatchConfidence, PairChoice } from "../types";
import { formatLength, lengthDifference } from "./playbackLibrary";
import { arrangeSongs, CLOSE_SECONDS, LIKELY_DIFFERENT_SECONDS, secondsApart } from "./songPairing";

/**
 * The song matchup (DESIGN.md "Song arrangement"): one row per plan song and per Playback song, in the order a person reads
 * them, each with the plain words for where the two sides disagree. Two sources feed it and both end in the same rows:
 *  - `classicRows`: the saved scan alone (Playback songs are "Playback song N", paired by position after the skips), exactly what
 *    the window did before the Library Link existed;
 *  - `libraryRows`: the Playback library's setlist, paired with the plan by title (the backend says how sure each pair is), with
 *    the saved scan joined on afterwards so the skips and "which song will really start this timer" stay true.
 * Nothing here reorders a song, saves anything or asks anyone.
 */

export type RowStamp = "matched" | "check-key" | "check-length" | "check-title" | "check-order" | "only-plan" | "only-playback" | "skipped";

/** The word on a row's stamp. */
export const STAMP_WORDS: Record<RowStamp, string> = {
  matched: "Matched",
  "check-key": "Check key",
  "check-length": "Check length",
  "check-title": "Check title",
  "check-order": "Check order",
  "only-plan": "Only in plan",
  "only-playback": "Only in Playback",
  skipped: "Skipped",
};

export interface PlanCell {
  /** Place among the plan's songs, from 1. */
  number: number;
  id: string;
  title: string;
  seconds: number | null;
  /** The key the plan names for the song, when it names one (library only). */
  key: string | null;
  skipped: boolean;
}

export interface PlaybackCell {
  /** Place in Playback's setlist, from 1. */
  number: number;
  /** The Playback title; null in the scan-only view, which has no titles ("Playback song N"). */
  title: string | null;
  /** The library's song (key, BPM, sections, cover); null in the scan-only view and for an uncertain pair's cover. */
  song: LibrarySong | null;
  /** Show the song's cover (only a song the backend paired with a plan song has one to serve). */
  cover: boolean;
  /** The length shown, in seconds: the scan's, else the library's countdown length. */
  seconds: number | null;
  /** The saved scan's id for this song, the one a skip is stored under; null when the scan cannot be joined to it. */
  skipId: number | null;
  skipped: boolean;
}

/** What a person can do about the pairing of one plan song, on its row (Song order): pick the Playback song, or undo a pick. */
export interface RowPairing {
  /** The plan song's key (`LibraryPlanSong.key`), which the backend pairs by. */
  planKey: string;
  /** The Playback songs a person may pick, the alike one first. Empty: Playback has no other song to pair. */
  choices: PairChoice[];
  /** A person paired this row ("Paired by you"): the action is Undo, not Pick. */
  byHand: boolean;
}

export interface MatchRow {
  id: string;
  kind: "pair" | "plan-only" | "playback-only";
  stamp: RowStamp;
  confidence: MatchConfidence | null;
  plan: PlanCell | null;
  playback: PlaybackCell | null;
  /** The plain sentences under the row: every difference, in the order they matter. */
  notes: string[];
  /** Only a library row of a song that is not paired, only uncertain, or paired by hand. */
  pairing?: RowPairing;
  /** The whole row as one sentence for a screen reader. */
  sentence: string;
}

/** A plan song as the Playback status lists it (`plan_songs`). */
export interface PlanSongInfo { id: string; title: string; duration_seconds: number | null }

export interface MatchupInput {
  plan: PlanSongInfo[];
  order: readonly number[];
  lengths: readonly (number | null)[];
  planSkips: ReadonlySet<string>;
  playbackSkips: ReadonlySet<number>;
}

const known = (seconds: number | null | undefined): seconds is number => typeof seconds === "number" && Number.isFinite(seconds) && seconds >= 0;

/** m:ss, or "Unknown". */
export function clock(seconds: number | null | undefined): string {
  return known(seconds) ? formatLength(seconds) : "Unknown";
}

const STEPS: Record<string, number> = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 };

/** A key as (pitch class, minor): "Ab", "G#", "F♯m", "Bb minor". Null when it is not a key at all. */
function keyOf(text: string): { pitch: number; minor: boolean } | null {
  const match = /^([A-Ga-g])\s*([#♯b♭]?)\s*(m|min|minor)?$/.exec(text.trim());
  if (!match) return null;
  const accidental = match[2] === "#" || match[2] === "♯" ? 1 : match[2] === "b" || match[2] === "♭" ? -1 : 0;
  return { pitch: (STEPS[match[1]!.toLowerCase()]! + accidental + 12) % 12, minor: Boolean(match[3]) };
}

/** Whether two keys are the same note: "A#" and "Bb" are, "A" and "Am" are not. Text that is not a key compares as text. */
export function sameKey(a: string, b: string): boolean {
  const x = keyOf(a);
  const y = keyOf(b);
  if (x && y) return x.pitch === y.pitch && x.minor === y.minor;
  return a.trim().toLowerCase() === b.trim().toLowerCase();
}

/** "Plan says key A, Playback plays Ab." when both are known and they are not the same note, else null. */
export function keyDifference(planKey: string | null | undefined, playbackKey: string | null | undefined): string | null {
  if (!planKey || !playbackKey || sameKey(planKey, playbackKey)) return null;
  return `Plan says key ${planKey}, Playback plays ${playbackKey}.`;
}

/** The existing wording for a length that differs by more than 15 s, or null. */
export function lengthWords(song: LibraryPlanSong): string | null {
  const different = lengthDifference(song);
  return different ? `Playback counts this song down to ${different.playback}; the plan says ${different.plan}.` : null;
}

/** The word on a row's stamp: `Matched`, but `Close match` for a close pair and `Paired by you` for one a person made. */
export function matchedWords(stamp: RowStamp, confidence: MatchConfidence | null): string {
  if (stamp !== "matched") return STAMP_WORDS[stamp];
  return confidence === "close" ? "Close match" : confidence === "by_hand" ? "Paired by you" : STAMP_WORDS[stamp];
}

const ordinalSong = (number: number, title: string | null, side: "plan" | "Playback") =>
  title ? `${side === "plan" ? "plan song" : "Playback song"} ${number}, ${title}` : `${side === "plan" ? "plan song" : "Playback song"} ${number}`;

/** The row as one sentence, built from what it shows. */
function sentenceOf(row: Omit<MatchRow, "sentence">): string {
  const plan = row.plan ? ordinalSong(row.plan.number, row.plan.title, "plan") : null;
  const playback = row.playback ? ordinalSong(row.playback.number, row.playback.title, "Playback") : null;
  const stamp = matchedWords(row.stamp, row.confidence);
  const lead = plan && playback ? `${plan[0]!.toUpperCase()}${plan.slice(1)} goes with ${playback}.`
    : plan ? `${plan[0]!.toUpperCase()}${plan.slice(1)} has no Playback song.`
      : `${playback![0]!.toUpperCase()}${playback!.slice(1)} has no plan song.`;
  return [lead, `${stamp}.`, ...row.notes].join(" ");
}

function finish(row: Omit<MatchRow, "sentence">): MatchRow {
  return { ...row, sentence: sentenceOf(row) };
}

// ---- the scan alone ---------------------------------------------------------------------------

/** The scan-only matchup: the Nth song left on each side pairs, after the skips; Playback songs have no titles. */
export function classicRows({ plan, order, lengths, planSkips, playbackSkips }: MatchupInput): MatchRow[] {
  const lines = arrangeSongs(plan.map((song) => song.id), order.length, planSkips, playbackSkips, order);
  const planCell = (index: number): PlanCell => {
    const song = plan[index]!;
    return { number: index + 1, id: song.id, title: song.title, seconds: song.duration_seconds, key: null, skipped: planSkips.has(song.id) };
  };
  const playbackCell = (index: number): PlaybackCell => ({
    number: index + 1, title: null, song: null, cover: false, seconds: lengths[index] ?? null, skipId: order[index]!, skipped: playbackSkips.has(order[index]!),
  });
  return lines.map((line): MatchRow => {
    if (line.kind === "pair") {
      const apart = secondsApart(plan[line.plan]?.duration_seconds, lengths[line.playback]);
      const far = apart !== null && apart > LIKELY_DIFFERENT_SECONDS;
      const note = apart === null ? null : apart === 0 ? "Same length" : apart <= CLOSE_SECONDS ? `${apart} s apart` : far ? `${apart} s apart. Probably a different song.` : `${apart} s apart`;
      return finish({
        id: `pair-${line.plan}-${line.playback}`, kind: "pair", stamp: far ? "check-length" : "matched", confidence: null,
        plan: planCell(line.plan), playback: playbackCell(line.playback), notes: note ? [note] : [],
      });
    }
    if (line.kind === "plan-skip") return finish({ id: `plan-skip-${line.plan}`, kind: "plan-only", stamp: "skipped", confidence: null, plan: planCell(line.plan), playback: null, notes: [] });
    if (line.kind === "plan-alone") return finish({ id: `plan-alone-${line.plan}`, kind: "plan-only", stamp: "only-plan", confidence: null, plan: planCell(line.plan), playback: null, notes: ["No Playback song left to pair with."] });
    if (line.kind === "playback-skip") return finish({ id: `playback-skip-${line.playback}`, kind: "playback-only", stamp: "skipped", confidence: null, plan: null, playback: playbackCell(line.playback), notes: [] });
    return finish({ id: `playback-alone-${line.playback}`, kind: "playback-only", stamp: "only-playback", confidence: null, plan: null, playback: playbackCell(line.playback), notes: ["No service song left to pair with."] });
  });
}

// ---- the library --------------------------------------------------------------------------------

/**
 * Whether the library's answer is about the plan the window holds: the same number of songs with the same titles in the same
 * places. (The two are read at different moments; for a second after a plan changes they can disagree, and then the library is
 * simply not used.)
 */
export function libraryMatchesPlan(library: LibraryPlan | null | undefined, plan: readonly PlanSongInfo[]): library is LibraryPlan {
  if (!library?.enabled || library.state !== "ready" || !library.setlist) return false;
  return library.songs.length > 0 && library.songs.length === plan.length
    && library.songs.every((song, index) => song.plan_position === index + 1 && song.title === plan[index]!.title);
}

/** Every song of the chosen setlist, in Playback's order: the paired ones, the uncertain candidates and the ones only Playback has. */
export function setlistSongs(library: LibraryPlan): LibrarySong[] {
  const all = [
    ...library.songs.flatMap((song) => (song.library ? [song.library] : song.candidate ? [song.candidate] : [])),
    ...library.playback_only,
  ];
  return all.sort((a, b) => a.position - b.position);
}

export type ScanJoin = "id" | "position";

/**
 * How the saved scan's songs are tied to the library's. By Playback's own song numbers when they are the same set; by place when
 * the scan shares no number with the library but has as many songs (the scan's numbers could not be checked); otherwise not at all,
 * which is what a scan of another setlist looks like. Returns the library song of each scan place, or null.
 */
export function joinScan(order: readonly number[], songs: readonly LibrarySong[]): { by: ScanJoin; of: (LibrarySong | undefined)[] } | null {
  if (!order.length || order.length !== songs.length || new Set(order).size !== order.length) return null;
  const byId = new Map(songs.flatMap((song) => (song.setlist_song_id === null ? [] : [[song.setlist_song_id, song] as const])));
  const shared = order.filter((id) => byId.has(id)).length;
  if (shared === order.length && byId.size === songs.length) return { by: "id", of: order.map((id) => byId.get(id)) };
  if (shared === 0) return { by: "position", of: songs.slice() };
  return null;
}

/** The indices (into `values`) of one longest run of rising values: the pairs that are in the same order on both sides. */
export function longestRising(values: readonly number[]): Set<number> {
  const best: number[] = values.map(() => 1);
  const before: number[] = values.map(() => -1);
  let end = -1;
  values.forEach((value, i) => {
    for (let j = 0; j < i; j += 1) {
      if (values[j]! < value && best[j]! + 1 > best[i]!) { best[i] = best[j]! + 1; before[i] = j; }
    }
    if (end < 0 || best[i]! > best[end]!) end = i;
  });
  const kept = new Set<number>();
  for (let at = end; at >= 0; at = before[at]!) kept.add(at);
  return kept;
}

const PLAN_ONLY_WORDS: Record<string, string> = {
  missing: "Playback's setlist has no song with this title.",
  duplicate_in_plan: "This title is in the plan more than once, so StagePilot cannot tell which Playback song goes with it.",
  duplicate_in_playback: "Playback's setlist has this title more than once, so StagePilot cannot tell which one goes with it.",
};

export interface LibraryView {
  rows: MatchRow[];
  /** The scan is joined to the library, so the skips are live and "which song starts which timer" is known. */
  live: boolean;
  join: ScanJoin | null;
}

/**
 * The library matchup. `scan` is the saved scan when it is usable (there and not out of date), else null. With a scan that cannot
 * be joined to the setlist this returns null: the caller shows the scan-only view instead of guessing.
 */
export function libraryRows(input: MatchupInput, library: LibraryPlan, scan: boolean): LibraryView | null {
  const { plan, order, planSkips, playbackSkips } = input;
  const songs = setlistSongs(library);
  const join = scan ? joinScan(order, songs) : null;
  if (scan && !join) return null;

  // The scan place of each library song, and what the saved skips + positions will really do.
  const scanPlace = new Map<LibrarySong, number>();
  join?.of.forEach((song, place) => { if (song) scanPlace.set(song, place); });
  const startsPlan = new Map<number, LibrarySong | undefined>();
  const startedBy = new Map<LibrarySong, number>();
  if (join) {
    for (const line of arrangeSongs(plan.map((song) => song.id), order.length, planSkips, playbackSkips, order)) {
      if (line.kind !== "pair") continue;
      const song = join.of[line.playback];
      startsPlan.set(line.plan, song);
      if (song) startedBy.set(song, line.plan);
    }
  }

  const partnerOf = (entry: LibraryPlanSong): LibrarySong | null => entry.library ?? entry.candidate;
  const paired = library.songs.flatMap((entry, index) => (partnerOf(entry) ? [index] : []));
  const rising = longestRising(paired.map((index) => partnerOf(library.songs[index]!)!.position));
  const moved = new Set(paired.filter((_, at) => !rising.has(at)));

  const planCell = (index: number, entry: LibraryPlanSong): PlanCell => ({
    number: index + 1, id: plan[index]!.id, title: plan[index]!.title, seconds: plan[index]!.duration_seconds, key: entry.plan_key, skipped: planSkips.has(plan[index]!.id),
  });
  const playbackCell = (song: LibrarySong, cover: boolean): PlaybackCell => {
    const place = scanPlace.get(song);
    const skipId = place === undefined ? null : order[place]!;
    return {
      number: song.position, title: song.title, song, cover, seconds: song.countdown_length_s, skipId, skipped: skipId !== null && playbackSkips.has(skipId),
    };
  };
  const runtimeNote = (index: number, partner: LibrarySong | null): string | null => {
    if (!join || planSkips.has(plan[index]!.id)) return null;
    const starts = startsPlan.get(index);
    if (partner && starts === partner) return null;
    if (!starts) return partner ? "With the skips as they are, no Playback song will start this plan song's timer." : null;
    return partner
      ? `With the skips as they are, Playback song ${starts.position}, ${starts.title}, will start this plan song's timer, not ${partner.title}.`
      : `With the skips as they are, Playback song ${starts.position}, ${starts.title}, will start this plan song's timer.`;
  };

  const rows: MatchRow[] = [];
  const emitted = new Set<LibrarySong>();
  const flushPlaybackOnly = (before: number) => {
    for (const song of library.playback_only) {
      if (emitted.has(song) || song.position >= before) continue;
      emitted.add(song);
      const cell = playbackCell(song, false);
      const by = startedBy.get(song);
      const notes = ["Playback has this song; the plan does not."];
      if (by !== undefined && !cell.skipped) notes.push(`Playback song ${song.position} will start the timer of plan song ${by + 1}, ${plan[by]!.title}, unless you skip it.`);
      rows.push(finish({ id: `playback-only-${song.position}`, kind: "playback-only", stamp: cell.skipped ? "skipped" : "only-playback", confidence: null, plan: null, playback: cell, notes }));
    }
  };

  library.songs.forEach((entry, index) => {
    const partner = partnerOf(entry);
    if (partner) flushPlaybackOnly(partner.position);
    if (!partner) {
      const cell = planCell(index, entry);
      const notes = [PLAN_ONLY_WORDS[entry.status] ?? PLAN_ONLY_WORDS.missing!];
      const extra = runtimeNote(index, null);
      if (extra) notes.push(extra);
      rows.push(finish({ id: `plan-only-${index}`, kind: "plan-only", stamp: cell.skipped ? "skipped" : "only-plan", confidence: null, plan: cell, playback: null, notes, pairing: { planKey: entry.key, choices: entry.pair_choices ?? [], byHand: false } }));
      return;
    }
    emitted.add(partner);
    const confidence = entry.confidence;
    const uncertain = entry.status === "uncertain";
    const plan_ = planCell(index, entry);
    const playback = playbackCell(partner, !uncertain && Boolean(partner.artwork));
    const notes: string[] = [];
    let stamp: RowStamp = "matched";
    const flag = (next: RowStamp) => { if (stamp === "matched") stamp = next; };
    if (uncertain) {
      notes.push(`The titles are alike but not the same: the plan says “${plan_.title}”, Playback has “${partner.title}”. Check that they are the same song.`);
      flag("check-title");
    } else if (confidence === "close") {
      notes.push(`Close match: the plan says “${plan_.title}”, Playback has “${partner.title}”.`);
    } else if (confidence === "by_hand") {
      notes.push(`Paired by you: the plan says “${plan_.title}”, Playback has “${partner.title}”.`);
    }
    const order_ = runtimeNote(index, partner);
    if (order_) { notes.push(order_); flag("check-order"); }
    else if (!join && moved.has(index)) {
      notes.push(`This is song ${index + 1} in the plan but song ${partner.position} in Playback.`);
      flag("check-order");
    }
    if (!uncertain) {
      const key = keyDifference(entry.plan_key, partner.key);
      if (key) { notes.push(key); flag("check-key"); }
      const length = lengthWords(entry);
      if (length) { notes.push(length); flag("check-length"); }
    }
    if (plan_.skipped || playback.skipped) stamp = "skipped";
    const pairing: RowPairing | undefined = uncertain ? { planKey: entry.key, choices: entry.pair_choices ?? [], byHand: false }
      : confidence === "by_hand" ? { planKey: entry.key, choices: [], byHand: true } : undefined;
    rows.push(finish({ id: `pair-${index}-${partner.position}`, kind: "pair", stamp, confidence, plan: plan_, playback, notes, pairing }));
  });
  flushPlaybackOnly(Infinity);
  return { rows, live: join !== null, join: join?.by ?? null };
}

/** A number of rows in each state, for the summary. */
export function countStamps(rows: readonly MatchRow[]): Record<RowStamp, number> {
  const counts = Object.fromEntries(Object.keys(STAMP_WORDS).map((stamp) => [stamp, 0])) as Record<RowStamp, number>;
  for (const row of rows) counts[row.stamp] += 1;
  return counts;
}
