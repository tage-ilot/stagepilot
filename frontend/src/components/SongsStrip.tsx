import { captureLabelText, type CaptureLabel } from "./lightingLooks";
import type { CardLibrary } from "./SongCard";
import { SongCardBody } from "./SongCard";

/** One song of the loaded service plan, with what is saved for it under the method on screen. */
export interface SongChip {
  key: string;
  title: string;
  order: number;
  looks: number;
  /** Repeating loops saved with the song (OSC). */
  loops?: number;
  handmade: number;
  hasLoudness: boolean;
  /** What the saved rows say about capturing; absent on callers that predate capture status. */
  label?: CaptureLabel;
  /** What the optional Playback library says about the song (key, BPM, length, arrangement); information only. */
  library?: CardLibrary | null;
}

const isCaptured = (chip: SongChip) => chip.looks + (chip.loops ?? 0) + chip.handmade > 0 || chip.hasLoudness;

const countText = (chip: SongChip, noun: "looks" | "cues") => {
  if (chip.label === "not_captured" && chip.hasLoudness) return "loudness only";
  if (chip.label) return captureLabelText[chip.label];
  if (chip.looks > 0) return `${chip.looks} ${chip.looks === 1 ? "look" : "looks"}`;
  if (chip.handmade > 0) return `${chip.handmade} ${chip.handmade === 1 ? "cue" : "cues"}`;
  return noun === "looks" ? "loudness only" : "loudness only";
};

/**
 * The songs of the service as small cards above the timeline, in the same style as Songs in Playback: a cover (when the Library Link
 * has one), the title, what is captured then the tempo, and a thin arrangement bar. It grows to show every song, never scrolls. No heading and no key or length: the Song lighting timeline below
 * says where a song stands, and the Playback config has the key and length. A captured song is a coloured card
 * (a note glyph when it has loudness, and an x that asks to erase it); a plan song with nothing saved is grey and dashed. A click selects the song in
 * the timeline like the Song menu does. The buttons are at least 44 px tall so a finger lands.
 */
export function SongsStrip({ chips, noun, selectedKey, liveKey, onSelect, onClear }: {
  chips: SongChip[];
  noun: "looks" | "cues";
  selectedKey: string;
  liveKey: string;
  onSelect: (key: string) => void;
  onClear: (key: string) => void;
}) {
  if (!chips.length) return null;
  return (
    <section aria-label="Songs in this service" className="mt-5" data-testid="songs-strip">
      <ul aria-label="Songs in this service" className="grid content-start gap-1.5 p-0.5 [grid-template-columns:repeat(auto-fill,minmax(240px,1fr))]">
        {chips.map((chip) => {
          const has = isCaptured(chip);
          const manualOnly = chip.label === "manually_cued";
          const incomplete = chip.label === "incomplete";
          const counts = `${chip.looks + (chip.loops ?? 0)} captured item${chip.looks + (chip.loops ?? 0) === 1 ? "" : "s"} (${chip.looks} ramps/steps, ${chip.loops ?? 0} loop${chip.loops === 1 ? "" : "s"}), ${chip.handmade} hand-made`;
          const selected = chip.key === selectedKey;
          const live = chip.key === liveKey;
          const captureWords = has || chip.label ? countText(chip, noun) : "not captured";
          const name = [
            chip.title,
            has ? countText(chip, noun) : "not captured",
            has && chip.hasLoudness ? "has loudness" : null,
            chip.library?.title ?? null,
            live ? "live now" : null,
          ].filter(Boolean).join(", ");
          return (
            <li
              className={`relative flex min-w-0 items-stretch rounded-ds-lg border text-sm ${manualOnly ? "border-cue-look-manual bg-cue-look-manual/20 text-ink" : incomplete ? "border-dashed border-cue-look-captured bg-cue-look-captured/10 text-ink" : has ? "border-cue-look-captured bg-cue-look-captured/20 text-ink" : "border-dashed border-edge-medium bg-surface-900/40 text-ink-muted"} ${selected ? "outline outline-2 outline-offset-1 outline-ink" : ""}`}
              data-song={chip.key}
              data-state={has ? "captured" : "empty"}
              data-label={chip.label}
              title={chip.label ? `${captureLabelText[chip.label]} (${counts})` : undefined}
              key={chip.key}
            >
              <button
                aria-label={name}
                aria-pressed={selected}
                className="flex min-h-[44px] min-w-0 flex-1 touch-manipulation flex-col justify-between gap-0.5 rounded-ds-lg px-2.5 py-1.5 text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink"
                onClick={() => onSelect(chip.key)}
                type="button"
              >
                <SongCardBody
                  cover={chip.library?.cover ?? null}
                  lead={live ? <span aria-hidden="true" className="size-2 shrink-0 rounded-ds-full bg-success" data-testid="song-chip-live" /> : undefined}
                  sections={chip.library?.sections ?? []}
                  title={chip.title}
                >
                  <span data-testid="song-chip-capture">{captureWords}</span>
                  {has && chip.hasLoudness && <span className="ml-1" data-testid="song-chip-loudness" title="Has loudness">♪</span>}
                  {chip.library?.bpm && <span data-testid="song-chip-bpm" title={chip.library.title}>{` · ${chip.library.bpm}`}</span>}
                </SongCardBody>
              </button>
              {chip.looks + (chip.loops ?? 0) + chip.handmade > 0 && (
                <button
                  aria-label={`Clear cues for ${chip.title}...`}
                  className="flex min-h-[44px] min-w-[36px] shrink-0 touch-manipulation items-center justify-center rounded-ds-lg text-ink-body can-hover:text-danger-text focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink"
                  onClick={() => onClear(chip.key)}
                  type="button"
                >
                  <span aria-hidden="true">×</span>
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
