/**
 * The file the person picked for a song that ProPresenter has in more than one file, kept in THIS browser only (never sent anywhere but
 * with the next preview, never saved in StagePilot's settings). The key is the song's MultiTracks number (`content_id`), so the same
 * song keeps its file from one service to the next; a song with no number is not remembered (its pick lasts while the section is open).
 * The value is the file as the preview's `choices` writes it: `Songs / Firm Foundation.pro`.
 */
export const SONG_FILE_KEY_PREFIX = "stagepilot.presenter-song-file.";

const keyOf = (contentId: number) => `${SONG_FILE_KEY_PREFIX}${contentId}`;

/** The remembered file for a song, or `null`. */
export function rememberedFile(contentId: number | null): string | null {
  if (contentId === null) return null;
  try {
    const value = window.localStorage.getItem(keyOf(contentId));
    return value && value.trim() ? value : null;
  } catch {
    return null;
  }
}

export function rememberFile(contentId: number | null, file: string): void {
  if (contentId === null) return;
  try { window.localStorage.setItem(keyOf(contentId), file); } catch { /* the pick lasts while the section is on screen */ }
}

/** A remembered file that is no longer one of the song's files is dropped without a word: the card asks again. */
export function forgetFile(contentId: number | null): void {
  if (contentId === null) return;
  try { window.localStorage.removeItem(keyOf(contentId)); } catch { /* nothing was kept */ }
}
