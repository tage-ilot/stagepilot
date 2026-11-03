import type { LightingCue, OscLightingCue } from "../types";

/** What a timeline marker / cue row is. OSC rows get one of seven types; MIDI rows one per note letter. */
export type OscCueTypeId = "look-captured" | "look-edited" | "look-manual" | "cue-on" | "cue-off" | "fixture" | "other";
export type NoteLetter = "C" | "D" | "E" | "F" | "G" | "A" | "B";
export type MidiCueTypeId = `note-${NoteLetter}`;
export type CueTypeId = OscCueTypeId | MidiCueTypeId;

export interface CueTypeInfo {
  id: CueTypeId;
  /** Spoken and legend name. */
  name: string;
  /** The glyph box text; never colour alone. */
  glyph: string;
  group: "Looks" | "Lightkey cues" | "Single messages" | "Notes";
  /** Legend / list swatch classes (a small bar piece). Written out in full so Tailwind sees them. */
  bar: string;
}

const info = (id: CueTypeId, name: string, glyph: string, group: CueTypeInfo["group"], bar: string): [CueTypeId, CueTypeInfo] => [id, { id, name, glyph, group, bar }];

export const CUE_TYPES: Record<CueTypeId, CueTypeInfo> = Object.fromEntries([
  info("look-captured", "Captured look", "◆", "Looks", "bg-cue-look-captured"),
  info("look-edited", "Edited look", "◆✎", "Looks", "bg-cue-look-edited"),
  info("look-manual", "Manual look", "◇", "Looks", "bg-cue-look-manual"),
  info("cue-on", "Lightkey cue on", "▶", "Lightkey cues", "bg-cue-live"),
  info("cue-off", "Lightkey cue off", "■", "Lightkey cues", "cue-bar-off"),
  info("fixture", "Fixture message", "◐", "Single messages", "bg-cue-fixture"),
  info("other", "Other OSC", "○", "Single messages", "cue-bar-other"),
  info("note-C", "C notes", "C", "Notes", "bg-cue-note-c"),
  info("note-D", "D notes", "D", "Notes", "bg-cue-note-d"),
  info("note-E", "E notes", "E", "Notes", "bg-cue-note-e"),
  info("note-F", "F notes", "F", "Notes", "bg-cue-note-f"),
  info("note-G", "G notes", "G", "Notes", "bg-cue-note-g"),
  info("note-A", "A notes", "A", "Notes", "bg-cue-note-a"),
  info("note-B", "B notes", "B", "Notes", "bg-cue-note-b"),
]) as Record<CueTypeId, CueTypeInfo>;

export const OSC_TYPE_IDS: OscCueTypeId[] = ["look-captured", "look-edited", "look-manual", "cue-on", "cue-off", "fixture", "other"];
export const MIDI_TYPE_IDS: MidiCueTypeId[] = ["note-C", "note-D", "note-E", "note-F", "note-G", "note-A", "note-B"];
export const typeIdsFor = (method: "osc" | "midi"): CueTypeId[] => (method === "osc" ? OSC_TYPE_IDS : MIDI_TYPE_IDS);

const LIVE_CUE = /^\/live\/[^/]+\/cue\/([^/]+(?:\/[^/]+)*?)\/(activate|deactivate)$/;
const FIXTURE_MESSAGE = /^\/fixture\/[^/]+\/overrides\/.+$/;

/** Pure: the type of an OSC row. A row with messages is a look; a single address is a cue, a fixture message or other. */
export const classifyOscCue = (cue: OscLightingCue): OscCueTypeId => {
  if ((cue.messages?.length ?? 0) > 0) {
    if (cue.source === "captured") return cue.edited ? "look-edited" : "look-captured";
    return "look-manual";
  }
  const live = LIVE_CUE.exec(cue.address ?? "");
  if (live) return live[2] === "activate" ? "cue-on" : "cue-off";
  if (FIXTURE_MESSAGE.test(cue.address ?? "")) return "fixture";
  return "other";
};

const NOTE_SPELLING = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"] as const;
const pitchClass = (note: number) => ((Math.trunc(note) % 12) + 12) % 12;

/** `60` -> `C3` (the app's octave numbering: note 60 is C3). Flats are always spelled as the sharp of the same pitch. */
export const midiNoteName = (note: number): string => `${NOTE_SPELLING[pitchClass(note)]}${Math.floor(note / 12) - 2}`;

export interface MidiClass { type: MidiCueTypeId; letter: NoteLetter; sharp: boolean }

/** Pure: colour comes from the pitch class (note % 12), so it is the same in every octave and in numeric display mode. */
export const classifyMidiCue = (cue: Pick<LightingCue, "note">): MidiClass => {
  const spelled: string = NOTE_SPELLING[pitchClass(cue.note)] ?? "C";
  const letter = spelled[0] as NoteLetter;
  return { type: `note-${letter}`, letter, sharp: spelled.length === 2 };
};

export const glyphFor = (type: CueTypeId, sharp = false): string => CUE_TYPES[type].glyph + (sharp ? "♯" : "");

/** "Captured look, Look - 12 fixtures, at 00:14". */
export const typeAccessibleName = (type: CueTypeId, label: string, time: string): string => `${typeSpokenLabel(type, label)}, at ${time}`;

/** "Captured look, Look - 12 fixtures" (the time is added by whoever knows it). */
export const typeSpokenLabel = (type: CueTypeId, label: string): string => `${CUE_TYPES[type].name}, ${label}`;

/** "MIDI note C#3 (C sharp, octave 3), Verse, at 00:04". */
export const midiSpokenLabel = (note: number, label: string): string => {
  const name = midiNoteName(note);
  const spelled = name.replace(/-?\d+$/, "");
  const octave = name.slice(spelled.length);
  return `MIDI note ${name} (${spelled[0]}${spelled.length === 2 ? " sharp" : ""}, octave ${octave}), ${label}`;
};

/** "3 Captured look, 2 Lightkey cue on" in order of count, highest first. */
export const describeTypeCounts = (counts: Map<CueTypeId, number>): string =>
  [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([type, count]) => `${count} ${CUE_TYPES[type].name}`).join(", ");

export const dominantType = (counts: Map<CueTypeId, number>): CueTypeId | null =>
  [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
