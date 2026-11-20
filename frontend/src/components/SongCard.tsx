import type { ComponentPropsWithoutRef, ReactNode } from "react";

import type { LibraryPlanSong, LibrarySection } from "../types";
import { ArrangementBar } from "./ArrangementBar";
import { bpmText, chipLibrary, librarySongCover, type ChipLibrary } from "./playbackLibrary";

/** The picture of a paired plan song. */
export const songCover = (song: LibraryPlanSong): string | null => librarySongCover(song.library);

/** What a song card draws from the Playback library for one plan song: the cover, the arrangement, and the long words (all the facts) for the hover. */
export interface CardLibrary extends ChipLibrary {
  cover: string | null;
  /** "65 BPM", or null when Playback has no tempo for the song. */
  bpm: string | null;
  sections: LibrarySection[];
}

export function cardLibrary(song: LibraryPlanSong | undefined, now: number): CardLibrary | null {
  const chip = chipLibrary(song, now);
  if (!song?.library || !chip) return null;
  return { ...chip, cover: songCover(song), bpm: song.library.bpm !== null ? bpmText(song.library.bpm) : null, sections: song.library.sections };
}

/**
 * The inside of a song card, shared by the Playback config (Songs in Playback) and the Lighting tab (Songs in this service) so the
 * two lists read as the same thing: a small cover when Playback has a picture, the title, one line of facts, and under both a thin
 * bar of the arranged song. The caller draws the border and any buttons around it.
 */
export function SongCardBody({ title, cover, sections, lead, facts, chips, below, children }: {
  title: string;
  cover: string | null;
  sections: LibrarySection[];
  /** Drawn before the title (the live dot). */
  lead?: ReactNode;
  /** The song's facts as square chips (`FactChips`), under the title: key, length, BPM, status. Used INSTEAD of a line of words. */
  facts?: ReactNode;
  /** Small chips under the line of facts (a key, where the song comes from, why it cannot be used). Left out: nothing is drawn. */
  chips?: ReactNode;
  /** More plain text UNDER the bar, never cut short (the Presenter Link's arrangement): title, facts, the bar, then this. Left out: nothing is drawn. */
  below?: ReactNode;
  /** Words under the title when a card has no facts to chip (`not in Playback's setlist`). */
  children?: ReactNode;
}) {
  return (
    <>
      <span className="flex min-w-0 items-center gap-2">
        {cover && (
          <img
            alt=""
            className="h-7 w-7 shrink-0 rounded-ds-md bg-surface-900 object-cover"
            decoding="async"
            height={28}
            loading="lazy"
            onError={(event) => { event.currentTarget.style.visibility = "hidden"; }}
            src={cover}
            width={28}
          />
        )}
        <span className="block min-w-0 leading-tight">
          <span className="flex min-w-0 items-center gap-1.5">
            {lead}
            <span className="block truncate font-semibold text-ink" title={title}>{title}</span>
          </span>
          {facts && <span className="mt-0.5 block text-xs" data-part="facts-row">{facts}</span>}
          {children && <span className="block line-clamp-2 text-xs tabular-nums text-ink-muted">{children}</span>}
          {chips && <span className="mt-1 flex flex-wrap items-center gap-1" data-part="chips">{chips}</span>}
        </span>
      </span>
      <ArrangementBar sections={sections} />
      {below}
    </>
  );
}

// ---- the card around the body: one shell for every song-card list -----------------------------------------------------------------

/** The grid of song cards: as many 240 px columns as fit, growing to show every card (nothing scrolls inside it). */
export const SONG_GRID = "grid content-start gap-1.5 p-0.5 [grid-template-columns:repeat(auto-fill,minmax(240px,1fr))]";
/** The same grid with room between the outlines, for a list where every card can be selected at once (check mode). */
export const SONG_GRID_CHECK = "grid content-start gap-2.5 p-1 [grid-template-columns:repeat(auto-fill,minmax(240px,1fr))]";

/** The fills a card can have (written out in full so Tailwind can see them). */
export const SONG_CARD_LOOK = {
  /** A song with hand-made cues only (Lighting). */
  manual: "border-cue-look-manual bg-cue-look-manual/20 text-ink",
  /** A capture that did not finish (Lighting). */
  incomplete: "border-dashed border-cue-look-captured bg-cue-look-captured/10 text-ink",
  /** A captured song (Lighting). */
  captured: "border-cue-look-captured bg-cue-look-captured/20 text-ink",
  /** Nothing there, or nothing that can be used: grey and dashed. */
  empty: "border-dashed border-edge-medium bg-surface-900/40 text-ink-muted",
  /** A plain song (a list that picks songs). */
  plain: "border-edge-medium bg-surface-panel text-ink",
} as const;

/** The outline round a selected card (Lighting's selected song; a ticked card in a list that picks songs). */
const SELECTED = "outline outline-2 outline-offset-1 outline-ink";
const PRESSABLE = "flex min-h-[44px] min-w-0 flex-1 touch-manipulation flex-col justify-between gap-0.5 rounded-ds-lg px-2.5 py-1.5 text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink";

type DataAttributes = { [key: `data-${string}`]: string | undefined };

export interface SongCardShellProps {
  /** The card's fill (`SONG_CARD_LOOK`). */
  look: string;
  /** `press`: one button that picks this song (`aria-pressed`, Lighting's strip). `check`: a checkbox that is ticked on and off
   * (`role="checkbox"`, `aria-checked`, Space and Enter toggle, a visible check mark), for a list that picks several songs. `static`:
   * a card with nothing to press (a song that cannot be used). */
  mode?: "press" | "check" | "static";
  /** Pressed (press) or ticked (check): the outline, and in check mode the check mark. */
  selected?: boolean;
  disabled?: boolean;
  /** The button's name. */
  label?: string;
  /** Ids of what a screen reader reads after the name. */
  describedBy?: string;
  onActivate?: () => void;
  /** The body (`SongCardBody`). */
  children: ReactNode;
  /** Buttons beside the card's own (Lighting's x). */
  after?: ReactNode;
  /** The list item's own attributes (`data-*`, `title`). */
  item?: Omit<ComponentPropsWithoutRef<"li">, "className" | "children"> & DataAttributes;
}

/** A visible check mark for a check-mode card: a box with a tick, never colour alone. */
function CheckMark({ checked }: { checked: boolean }) {
  return (
    <span aria-hidden="true" className={`pointer-events-none absolute right-2 top-2 inline-flex size-4 items-center justify-center rounded-ds-sm border text-[0.6875rem] font-bold leading-none ${checked ? "border-ink bg-ink text-surface-950" : "border-ink-muted bg-surface-950 text-transparent"}`} data-part="check">
      {"\u2713"}
    </span>
  );
}

/**
 * ONE song card, around a `SongCardBody`: the border and fill, the outline when selected, a 44 px target. Used by the Lighting tab's
 * strip (press mode) and the Presenter Link's song list (check mode), so the two read as the same thing.
 */
export function SongCardShell({ look, mode = "press", selected = false, disabled = false, label, describedBy, onActivate, children, after, item }: SongCardShellProps) {
  const check = mode === "check";
  return (
    <li className={`relative flex min-w-0 items-stretch rounded-ds-lg border text-sm ${look} ${selected ? SELECTED : ""}`} {...item}>
      {mode === "static" ? <div className={PRESSABLE.replace(" touch-manipulation", "")} data-part="card">{children}</div> : (
        <button
          aria-checked={check ? selected : undefined}
          aria-describedby={describedBy}
          aria-label={label}
          aria-pressed={check ? undefined : selected}
          className={check ? `${PRESSABLE} pr-8 disabled:cursor-not-allowed` : PRESSABLE}
          disabled={disabled || undefined}
          onClick={onActivate}
          role={check ? "checkbox" : undefined}
          type="button"
        >
          {children}
        </button>
      )}
      {check && <CheckMark checked={selected} />}
      {after}
    </li>
  );
}
