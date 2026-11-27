import type { FollowLock, FollowSong } from "../presenterFollow";
import { hasFix, NO_CUES_WORD, songChipWords, songPhrase, templateFix } from "../presenterFollow";
import type { AddSongsRow } from "./addSongsSongs";
import { NO_PLAN_SONGS } from "./addSongsWording";
import { CoverSlot } from "./CoverSlot";
import { librarySongCover } from "./playbackLibrary";
import { StatusChip } from "./StatusChips";

/** The chip a row shows while it waits for the first answer of the follower. */
const CHECKING = { word: "Checking\u2026", mark: "\u2026", tone: "idle" } as const;

/** One row: the song's cover and title, its readiness chip in the backend's words (a mark AND words), the quiet phrase, and the one-sentence fix. */
function SongRow({ row, song, lock }: { row: AddSongsRow; song: FollowSong | undefined; lock: FollowLock | null }) {
  // While the section is locked nothing claims a song is followed: every row says which link is missing, in the words the backend uses for it.
  const shown: FollowSong | undefined = lock && song ? { ...song, ...lock.chip, following: null, advice: null } : song;
  const phrase = shown ? songPhrase(shown) : null;
  const fix = shown && hasFix(shown);
  return (
    <li className="grid min-w-0 gap-x-3 gap-y-1.5 rounded-ds-lg border border-edge-medium bg-surface-panel p-2.5 text-sm sm:grid-cols-[minmax(0,14rem)_minmax(0,1fr)]" data-code={shown?.code} data-testid="auto-presenter-song">
      <span className="flex min-w-0 items-center gap-2">
        <CoverSlot src={librarySongCover(row.song)} />
        <span className="block min-w-0 truncate font-semibold text-ink" title={row.title}>{row.title}</span>
      </span>
      <span className="flex min-w-0 flex-col items-start gap-1">
        <span className="flex min-w-0 max-w-full flex-wrap items-center gap-1.5">
          {shown ? <StatusChip {...songChipWords(shown)} testId="follow-song-chip" title={shown.advice ? `${shown.text}. ${shown.advice}` : shown.text} wrap /> : <StatusChip {...CHECKING} testId="follow-song-chip" />}
          {phrase && <span className="text-xs text-ink-body" data-testid="follow-song-phrase">{phrase}</span>}
          {shown?.code === "bars" && fix && <StatusChip mark="?" testId="follow-song-no-cues" title={shown.advice ?? undefined} tone="warning" word={NO_CUES_WORD} />}
        </span>
        {shown && fix && <span className="block text-xs text-ink-body" data-testid="follow-song-fix">{templateFix(shown)}</span>}
      </span>
    </li>
  );
}

/**
 * The Auto-Presenter's list of the plan's songs: one row per song with its cover and title and whether it is followed, in the words of
 * docs/propresenter.md. A song with no Playback cues also says **No cues** and the one sentence that fixes it in Playback, naming the
 * Lyrics Bus template the song file needs. While the section is locked every row says so instead.
 */
export function AutoPresenterSongs({ rows, follow, lock, headingId }: { rows: readonly AddSongsRow[]; follow: ReadonlyMap<string, FollowSong>; lock: FollowLock | null; headingId: string }) {
  return (
    <section aria-labelledby={headingId} className="space-y-2" data-testid="auto-presenter-songs">
      <h4 className="sr-only" id={headingId}>Songs</h4>
      {rows.length === 0 ? <p className="text-sm text-ink-muted" data-testid="auto-presenter-empty">{NO_PLAN_SONGS}</p> : (
        <ul aria-labelledby={headingId} className="grid min-w-0 gap-2">
          {rows.map((row) => <SongRow key={row.id} lock={lock} row={row} song={follow.get(row.id)} />)}
        </ul>
      )}
    </section>
  );
}
