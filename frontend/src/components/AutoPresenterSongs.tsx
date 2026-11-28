import type { FollowLock, FollowSong } from "../presenterFollow";
import { hasFix, songChipWords, songPhrase, templateFix } from "../presenterFollow";
import type { AddSongsRow } from "./addSongsSongs";
import { NO_PLAN_SONGS, unpairedSentence } from "./addSongsWording";
import { CoverSlot } from "./CoverSlot";
import { librarySongCover } from "./playbackLibrary";
import { StatusChip } from "./StatusChips";
import { cx } from "./ui/cx";

/** The chip a row shows while it waits for the first answer of the follower. */
const CHECKING = { word: "Checking\u2026", mark: "\u2026", tone: "idle" } as const;

/**
 * One row: the song's cover and title, ONE readiness chip in the backend's words (a mark AND words; a song with no cues has just
 * `No cues: following by bars`), the quiet phrase, and the one-sentence fix. While the section is locked the row is only its cover and
 * its title, greyed: the one reason box above the list says what is missing.
 */
function SongRow({ row, song, locked }: { row: AddSongsRow; song: FollowSong | undefined; locked: boolean }) {
  const phrase = !locked && song ? songPhrase(song) : null;
  const fix = !locked && song && hasFix(song);
  return (
    <li className={cx("grid min-w-0 gap-x-3 gap-y-1.5 rounded-ds-lg border border-edge-medium bg-surface-panel p-2.5 text-sm", locked ? "opacity-60" : "sm:grid-cols-[minmax(0,14rem)_minmax(0,1fr)]")} data-code={locked ? undefined : song?.code} data-testid="auto-presenter-song">
      <span className="flex min-w-0 items-center gap-2">
        <CoverSlot src={librarySongCover(row.song)} />
        <span className="block min-w-0 truncate font-semibold text-ink" title={row.title}>{row.title}</span>
      </span>
      {!locked && (
        <span className="flex min-w-0 flex-col items-start gap-1">
          <span className="flex min-w-0 max-w-full flex-wrap items-center gap-1.5">
            {song ? <StatusChip {...songChipWords(song)} testId="follow-song-chip" title={song.advice ? `${song.text}. ${song.advice}` : song.text} wrap /> : <StatusChip {...CHECKING} testId="follow-song-chip" />}
            {phrase && <span className="text-sm text-ink" data-testid="follow-song-phrase">{phrase}</span>}
          </span>
          {song && fix && <span className="block text-sm text-ink" data-testid="follow-song-fix">{templateFix(song)}</span>}
        </span>
      )}
    </li>
  );
}

/**
 * The Auto-Presenter's list of the plan's songs: one row per song with its cover and title and whether it is followed, in the words of
 * docs/propresenter.md. A song with no Playback cues says ONE thing, `No cues: following by bars`, and the one sentence that fixes it in
 * Playback, naming the Lyrics Bus template the song file needs. While the section is locked the rows are titles only (no chip), greyed.
 */
export function AutoPresenterSongs({ rows, follow, lock, headingId, unpaired = [] }: { rows: readonly AddSongsRow[]; follow: ReadonlyMap<string, FollowSong>; lock: FollowLock | null; headingId: string; unpaired?: readonly string[] }) {
  return (
    <section aria-labelledby={headingId} className="space-y-2" data-testid="auto-presenter-songs">
      <h4 className="sr-only" id={headingId}>Songs</h4>
      {rows.length === 0 ? <p className="text-sm text-ink-muted" data-testid="auto-presenter-empty">{NO_PLAN_SONGS}</p> : (
        <ul aria-labelledby={headingId} className="grid min-w-0 gap-2">
          {rows.map((row) => <SongRow key={row.id} locked={lock !== null} row={row} song={follow.get(row.id)} />)}
        </ul>
      )}
      {lock === null && unpaired.length > 0 && <p className="text-sm text-ink-muted" data-testid="auto-presenter-unpaired">{unpairedSentence(unpaired)}</p>}
    </section>
  );
}
