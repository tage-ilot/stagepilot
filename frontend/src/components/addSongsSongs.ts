import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { LibraryPlan, LibrarySong } from "../types";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";

/** One song of the paired Playback setlist, as the window lists it. */
export interface AddSongsRow {
  /** `setlist_song_id` as text: what the preview names a song by (`ref_id`). Empty when Playback gave the song no number. */
  ref: string;
  title: string;
  /** The key Playback plays it in; null when none is known. */
  keyName: string | null;
  arrangement: string | null;
  song: LibrarySong;
  /** Playback gave the song no number, so StagePilot cannot name it to ProPresenter's side. */
  unnamed: boolean;
}

/** The setlist songs the loaded plan is paired with (an uncertain or missing pairing has no Playback song to add). In plan order. */
export function addSongsRows(plan: LibraryPlan | null | undefined): AddSongsRow[] {
  if (!plan?.enabled || plan.state !== "ready" || !plan.setlist) return [];
  return plan.songs.flatMap((planSong) => {
    const song = planSong.library;
    if (!song) return [];
    return [{
      ref: song.setlist_song_id === null ? "" : String(song.setlist_song_id),
      title: song.title,
      keyName: song.key,
      arrangement: song.arrangement,
      song,
      unnamed: song.setlist_song_id === null,
    }];
  });
}

/**
 * The one rule for whether **Add to ProPresenter** is drawn: the Presenter Link is ready (this computer, or the helper), the Playback
 * Library Link answers with data that is not an old copy, and the loaded plan is paired with a Playback setlist that has songs.
 * Anything else draws nothing at all (no warning: the feature is optional).
 */
export function addToProPresenterAvailable(link: Pick<ProPresenterLinkController, "status"> | undefined, library: Pick<PlaybackLibraryController, "status" | "plan"> | undefined): boolean {
  if (!link || !library) return false;
  const linkReady = link.status?.state === "ready" && link.status.mode !== null;
  const libraryReady = Boolean(library.status?.active) && !library.status?.stale;
  return linkReady && libraryReady && addSongsRows(library.plan).length > 0;
}
