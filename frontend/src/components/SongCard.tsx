import type { ReactNode } from "react";

import { playbackLibraryArtworkUrl } from "../api";
import type { LibraryPlanSong, LibrarySection } from "../types";
import { ArrangementBar } from "./ArrangementBar";
import { bpmText, chipLibrary, type ChipLibrary } from "./playbackLibrary";

/** The picture of a paired song, fetched through StagePilot (the etag in the address makes a changed picture a new address). */
export const songCover = (song: LibraryPlanSong): string | null => {
  const library = song.library;
  return library?.artwork && library.setlist_song_id !== null ? playbackLibraryArtworkUrl(library.setlist_song_id, library.artwork.etag) : null;
};

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
export function SongCardBody({ title, cover, sections, lead, children }: {
  title: string;
  cover: string | null;
  sections: LibrarySection[];
  /** Drawn before the title (the live dot). */
  lead?: ReactNode;
  /** The line of facts under the title. */
  children: ReactNode;
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
          <span className="block line-clamp-2 text-xs tabular-nums text-ink-muted">{children}</span>
        </span>
      </span>
      <ArrangementBar sections={sections} />
    </>
  );
}
