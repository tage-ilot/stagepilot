import { captureLabelText, type CaptureLabel } from "./lightingLooks";
import { Button } from "./ui/Button";

/** One song of the loaded service plan, with what is saved for it under the method on screen. */
export interface SongChip {
  key: string;
  title: string;
  order: number;
  looks: number;
  handmade: number;
  hasLoudness: boolean;
  /** What the saved rows say about capturing; absent on callers that predate capture status. */
  label?: CaptureLabel;
}

const isCaptured = (chip: SongChip) => chip.looks + chip.handmade > 0 || chip.hasLoudness;

const countText = (chip: SongChip, noun: "looks" | "cues") => {
  if (chip.label === "not_captured" && chip.hasLoudness) return "loudness only";
  if (chip.label) return captureLabelText[chip.label];
  if (chip.looks > 0) return `${chip.looks} ${chip.looks === 1 ? "look" : "looks"}`;
  if (chip.handmade > 0) return `${chip.handmade} ${chip.handmade === 1 ? "cue" : "cues"}`;
  return noun === "looks" ? "loudness only" : "loudness only";
};

/**
 * The songs of the service as small chips above the timeline. A captured song is a coloured box (title, count, a note glyph when
 * it has loudness, and an x that asks to erase it); a plan song with nothing saved is grey and dashed. A click selects the song in
 * the timeline like the Song menu does. The visible chip is 28 px tall; the buttons inside are 44 px tall so a finger lands.
 */
export function SongsStrip({ chips, noun, selectedKey, liveKey, onSelect, onClear, onClearAll }: {
  chips: SongChip[];
  noun: "looks" | "cues";
  selectedKey: string;
  liveKey: string;
  onSelect: (key: string) => void;
  onClear: (key: string) => void;
  onClearAll: () => void;
}) {
  if (!chips.length) return null;
  const captured = chips.filter((chip) => (chip.label ? chip.label === "fully_captured" || chip.label === "manually_cued_and_fully_captured" : isCaptured(chip))).length;
  return (
    <section aria-label="Songs in this service" className="mt-5" data-testid="songs-strip">
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <h3 className="text-type-body-sm font-semibold text-ink">{`Songs in this service - ${captured} of ${chips.length} ${chips.some((chip) => chip.label) ? "fully " : ""}captured`}</h3>
        <Button disabled={!chips.some((chip) => chip.looks + chip.handmade > 0)} onClick={onClearAll} variant="secondary">Clear all songs...</Button>
      </div>
      <ul aria-label="Songs in this service" className="mt-2 flex max-h-[11.5rem] flex-wrap content-start gap-x-2 gap-y-4 overflow-y-auto overscroll-contain py-2">
        {chips.map((chip) => {
          const has = isCaptured(chip);
          const manualOnly = chip.label === "manually_cued";
          const incomplete = chip.label === "incomplete";
          const counts = `${chip.looks} ${chip.looks === 1 ? "look" : "looks"}, ${chip.handmade} hand-made`;
          const selected = chip.key === selectedKey;
          const live = chip.key === liveKey;
          const name = [
            chip.title,
            has ? countText(chip, noun) : "not captured",
            has && chip.hasLoudness ? "has loudness" : null,
            live ? "live now" : null,
          ].filter(Boolean).join(", ");
          return (
            <li
              className={`relative flex h-7 min-w-0 max-w-[28rem] items-center rounded-ds-lg border text-type-body-sm ${manualOnly ? "border-cue-look-manual bg-cue-look-manual/20 text-ink" : incomplete ? "border-dashed border-cue-look-captured bg-cue-look-captured/10 text-ink" : has ? "border-cue-look-captured bg-cue-look-captured/20 text-ink" : "border-dashed border-edge-medium bg-surface-900/40 text-ink-muted"} ${selected ? "outline outline-2 outline-offset-1 outline-ink" : ""}`}
              data-song={chip.key}
              data-state={has ? "captured" : "empty"}
              data-label={chip.label}
              title={chip.label ? `${captureLabelText[chip.label]} (${counts})` : undefined}
              key={chip.key}
            >
              <button
                aria-label={name}
                aria-pressed={selected}
                className="relative flex min-h-[44px] min-w-0 touch-manipulation items-center gap-1.5 rounded-ds-lg py-0 pl-2.5 pr-2 text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink"
                onClick={() => onSelect(chip.key)}
                style={{ marginTop: -8, marginBottom: -8 }}
                type="button"
              >
                {live && <span aria-hidden="true" className="size-2 shrink-0 rounded-ds-full bg-success" data-testid="song-chip-live" />}
                <span aria-hidden="true" className="max-w-[16rem] shrink-0 truncate font-semibold" title={chip.title}>{chip.title}</span>
                <span aria-hidden="true" className="min-w-0 truncate tabular-nums text-ink-body">{`· ${has || chip.label ? countText(chip, noun) : "not captured"}`}</span>
                {has && chip.hasLoudness && <span aria-hidden="true" className="shrink-0" data-testid="song-chip-loudness" title="Has loudness">♪</span>}
              </button>
              {chip.looks + chip.handmade > 0 && (
                <button
                  aria-label={`Clear cues for ${chip.title}...`}
                  className="relative flex min-h-[44px] min-w-[36px] shrink-0 touch-manipulation items-center justify-center rounded-ds-lg text-ink-body can-hover:text-danger-text focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink"
                  onClick={() => onClear(chip.key)}
                  style={{ marginTop: -8, marginBottom: -8 }}
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
