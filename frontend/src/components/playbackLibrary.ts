import type { LengthSource, LibraryPlan, LibraryPlanSong, LibrarySection, LibrarySong, ServicePlan } from "../types";
import { serviceStartsMs, zonedWallTimeToMs } from "./dashboard/scanReminder";

/**
 * The optional Playback library, read-only: what the helper on the Playback computer says about each plan song, and the plain
 * words for where it disagrees with the plan. Nothing here is saved or sent anywhere; it only reads a `LibraryPlan`.
 *
 * The length is where StagePilot's countdown ends: the START OF THE SONG'S LAST MEASURE (`scan_length_s`); the END OF THE SONG
 * (`length_s`) for a song with no beat grid or whose last measure starts more than 6 s before the end. The backend resolves the
 * rule into `countdown_length_s` (and says in `length_source` which one it is); this window only displays that one (owner, 2026-11-16: "countdowns ending at the start of the last measure ...
 * more accurate to where the song goes quiet"; "if it has no beat grid the end of the song can be used instead").
 */

/** Playback's length may differ from the plan's by this much before it is worth saying (information only). */
export const LENGTH_TOLERANCE_SECONDS = 15;

export const formatLength = (seconds: number): string => {
  const whole = Math.max(0, Math.round(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
};

export interface LibraryNotice {
  id: string;
  kind: "missing" | "title" | "order" | "length" | "rental" | "date";
  /** An actionable notice goes in the header pill; the rest is information (the chips and the Playback API panel). */
  actionable: boolean;
  text: string;
}

/** Added to the chip's tooltip when the length is the end of the song, and why (nothing for the start of the last measure). */
const LENGTH_SOURCE_WORDS: Record<LengthSource, string | null> = {
  last_measure: null,
  song_end_no_grid: "Playback has no bar lines for this song, so this is where the song ends.",
  song_end_long_last_measure: "This song's last measure is longer than 6 seconds, so this is where the song ends.",
};

/** The hover text of a countdown length: which point of the song the countdown ends at, and why when it is the end of the song. */
export function lengthSourceTitle(song: LibrarySong): string | null {
  if (song.countdown_length_s === null) return null;
  const why = song.length_source ? LENGTH_SOURCE_WORDS[song.length_source] : null;
  return why ?? "StagePilot's countdown for this song ends at the start of its last measure.";
}

export const sectionWord = (n: number) => `${n} ${n === 1 ? "section" : "sections"}`;

/** The face of a song: key (and "was X" when it differs), BPM and the countdown length m:ss; only what the helper gave. */
/** "65 BPM", or "72.4 BPM" when the tempo is not a whole number. */
export const bpmText = (bpm: number): string => `${Number.isInteger(bpm) ? bpm : Math.round(bpm * 10) / 10} BPM`;

/** "Ab (was C) · 4:57": only the key and the countdown length, the two facts the Playback config cards show. */
export function songKeyLength(song: LibrarySong): string[] {
  return songFaceFacts(song).filter((fact) => !/ BPM$/.test(fact));
}

export function songFaceFacts(song: LibrarySong): string[] {
  const facts: string[] = [];
  if (song.key) facts.push(song.key_original && song.key_original !== song.key ? `${song.key} (was ${song.key_original})` : song.key);
  if (song.bpm !== null) facts.push(bpmText(song.bpm));
  if (song.countdown_length_s !== null) facts.push(formatLength(song.countdown_length_s));
  return facts;
}

/** "Ab (was C) · 65 BPM · 4:57 · 7 sections · Default": only what the helper gave; nothing for a song with no beat grid length. */
export function songFacts(song: LibrarySong): string[] {
  const facts = songFaceFacts(song);
  if (song.section_count !== null) facts.push(sectionWord(song.section_count));
  if (song.arrangement) facts.push(song.arrangement);
  return facts;
}

/** Playback's length for a plan song when it differs from the plan's by more than 15 s, else null. */
export function lengthDifference(song: LibraryPlanSong): { playback: string; plan: string } | null {
  const playback = song.library?.countdown_length_s ?? null;
  if (playback === null || song.plan_seconds === null) return null;
  if (Math.abs(playback - song.plan_seconds) <= LENGTH_TOLERANCE_SECONDS) return null;
  return { playback: formatLength(playback), plan: formatLength(song.plan_seconds) };
}

export interface ChipLibrary {
  /** The chip's extra words. */
  text: string;
  /** All of it, for the hover and the screen reader. */
  title: string;
}

export function chipLibrary(song: LibraryPlanSong | undefined): ChipLibrary | null {
  if (!song?.library) return null;
  const facts = songFacts(song.library);
  const different = lengthDifference(song);
  if (different) {
    // The length is the one fact that can disagree: say where, next to it.
    const at = facts.indexOf(different.playback);
    if (at >= 0) facts[at] = `${different.playback} (plan ${different.plan})`;
  }
  if (!facts.length) return null;
  const title = [`Playback: ${facts.join(", ")}`];
  if (different) title.push(`Playback counts this song down to ${different.playback}; the plan says ${different.plan}.`);
  const why = song.library.countdown_length_s !== null && song.library.length_source ? LENGTH_SOURCE_WORDS[song.library.length_source] : null;
  if (why) title.push(why);
  return { text: facts.join(" · "), title: title.join(". ") };
}

/** The moment a rental must outlast: the last service start of the plan's day, else the end of that day, in the saved time zone. */
export function rentalDeadline(plan: Pick<ServicePlan, "date" | "service_times"> | null, timeZone: string): { at: number; known: boolean } | null {
  if (!plan) return null;
  const starts = serviceStartsMs(plan, timeZone);
  if (starts.length) return { at: starts[starts.length - 1]!, known: true };
  const day = /^(\d{4})-(\d{2})-(\d{2})/.exec(plan.date ?? "");
  if (!day) return null;
  const end = zonedWallTimeToMs(timeZone, Number(day[1]), Number(day[2]), Number(day[3]), 23, 59);
  return end === null ? null : { at: end, known: false };
}

/**
 * Every notice about a plan, in plan order. Quiet when everything agrees (an empty list). The pill gets only the actionable ones
 * (`libraryPillLine`); the length difference is information on the chip and in the Playback API panel.
 */
export function libraryNotices(
  library: LibraryPlan | null | undefined,
  plan: Pick<ServicePlan, "date" | "service_times"> | null,
  timeZone: string,
): LibraryNotice[] {
  if (!library?.enabled) return [];
  if (library.state === "no_setlist") {
    return library.message ? [{ id: "date", kind: "date", actionable: true, text: library.message }] : [];
  }
  if (library.state !== "ready") return [];
  const matched = library.songs.filter((song) => song.library);
  // Order is judged among the songs that are in both lists, so one missing song does not make every later song look moved.
  const planRank = new Map(matched.map((song, rank) => [song, rank]));
  const playbackRank = new Map([...matched].sort((a, b) => a.library!.position - b.library!.position).map((song, rank) => [song, rank]));
  const deadline = rentalDeadline(plan, timeZone);
  const ordered: LibraryNotice[] = [];
  for (const song of library.songs) {
    if (song.status === "missing") {
      ordered.push({ id: `missing:${song.key}:${song.plan_position}`, kind: "missing", actionable: true, text: `${song.title} is not in Playback's setlist.` });
    }
    if (song.status === "uncertain" && song.candidate) {
      ordered.push({
        id: `title:${song.key}:${song.plan_position}`, kind: "title", actionable: true,
        text: `${song.title} may be “${song.candidate.title}” in Playback's setlist. Check that they are the same song.`,
      });
    }
    if (song.library && planRank.get(song) !== playbackRank.get(song)) {
      ordered.push({
        id: `order:${song.key}:${song.plan_position}`, kind: "order", actionable: true,
        text: `${song.title} is song ${song.plan_position} in the plan but song ${song.library.position} in Playback.`,
      });
    }
    const different = lengthDifference(song);
    if (different) {
      ordered.push({
        id: `length:${song.key}:${song.plan_position}`, kind: "length", actionable: false,
        text: `${song.title}: Playback counts this song down to ${different.playback}; the plan says ${different.plan}.`,
      });
    }
    const expires = song.library?.rental_expires_utc ? Date.parse(song.library.rental_expires_utc) : NaN;
    if (deadline && Number.isFinite(expires) && expires < deadline.at) {
      ordered.push({
        id: `rental:${song.key}:${song.plan_position}`, kind: "rental", actionable: true,
        text: `${song.title} is a rental in Playback that ends before ${deadline.known ? "the service starts" : `the end of ${library.plan_date ?? "the service day"}`}.`,
      });
    }
  }
  return ordered;
}

/** The pill's one line: the actionable notices together, or null when there is nothing to say. */
export function libraryPillLine(notices: LibraryNotice[]): { label: string; detail: string } | null {
  const actionable = notices.filter((notice) => notice.actionable);
  if (!actionable.length) return null;
  const detail = actionable.map((notice) => notice.text).join(" ");
  const short = actionable.map((notice) => notice.text.replace(/\.$/, "")).join("; ");
  const label = actionable.length === 1 || short.length <= 90 ? `Playback library: ${short}` : `Playback library: ${actionable.length} things to check`;
  return { label, detail };
}

/** The names an arrangement bar says aloud before "and N more". */
export const BAR_NAMES_SPOKEN = 12;

/** "Arrangement: Count Off, Intro, Verse 1 and 18 more (30 sections)": at most 12 names, in playing order. */
export function arrangementLabel(sections: LibrarySection[]): string {
  const names = sections.slice(0, BAR_NAMES_SPOKEN).map((section) => section.name).join(", ");
  const more = sections.length - BAR_NAMES_SPOKEN;
  return `Arrangement: ${names}${more > 0 ? ` and ${more} more` : ""} (${sections.length} ${sections.length === 1 ? "section" : "sections"})`;
}
