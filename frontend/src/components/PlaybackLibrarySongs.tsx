import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { LibraryPlanSong, ServicePlan } from "../types";
import { useKeyChangeClock } from "../hooks/useKeyChangeClock";
import { anyKeyWas, chipLibrary, libraryNotices, songKeyLength } from "./playbackLibrary";
import { FactChips } from "./sections/FactChips";
import { cardLibrary, SongCardBody, songCover } from "./SongCard";

/** What a chip says for a song Playback cannot be paired with (no cover, no bar). */
const unpaired = (song: LibraryPlanSong) =>
  song.status === "missing" ? "not in Playback's setlist"
    : song.status === "uncertain" ? `may be “${song.candidate?.title ?? "another song"}” in Playback, not sure`
      : "appears more than once, so StagePilot cannot tell which Playback song it is";

/**
 * One plan song as a chip: a 28 px cover (only when Playback has a picture), the title, key, BPM and countdown length, and under both a
 * thin bar of the arrangement. A song Playback is missing or cannot tell apart keeps its words, as a chip of the same shape.
 */
function SongChip({ song, now }: { song: LibraryPlanSong; now: number }) {
  const library = song.library;
  const card = cardLibrary(song, now);
  const facts = library ? songKeyLength(library, now) : [];
  // A song with facts shows them as chips; one without says why in words (not paired) or that Playback gave no details.
  const words = library ? (facts.length ? null : "no details") : unpaired(song);
  return (
    <li
      className="flex min-h-[44px] min-w-0 flex-col justify-between gap-0.5 rounded-ds-lg border border-edge-medium bg-surface-panel px-2.5 py-1.5 text-sm"
      data-song={song.key}
      data-status={song.status}
      title={library ? chipLibrary(song, now)?.title : undefined}
    >
      <SongCardBody cover={songCover(song)} facts={facts.length ? <FactChips facts={facts} /> : undefined} sections={card?.sections ?? []} title={song.title}>{words}</SongCardBody>
    </li>
  );
}

/**
 * What the Playback library (the Library Link) knows about the plan's songs, in the Playback API panel: one chip per plan song (a 28 px
 * cover, key, BPM, countdown length and a thin arrangement bar) and the notices (a song missing from Playback, a different order, a rental
 * that ends, another date). StagePilot only READS it: nothing here changes Playback, the plan, the saved scan, a timer or a light.
 *
 * It draws NOTHING unless the Library Link is ready (or outdated, which still works): a computer without the script looks exactly like
 * a build without the library. The notices that need action also go in the header pill; the length difference is information only.
 */
export function PlaybackLibrarySongs({ library, plan, timeZone }: { library: PlaybackLibraryController; plan: ServicePlan | null; timeZone: string }) {
  // The backend answers `ready` only while the Library Link is ready or outdated, so this one gate is the whole rule.
  const notices = libraryNotices(library.plan, plan, timeZone);
  // "(was X)" on a key lasts a day: while one shows, the cards look again every minute so it goes away by itself.
  const now = useKeyChangeClock((at) => anyKeyWas(library.plan, at));
  const songs = library.plan?.enabled && library.plan.state === "ready" ? library.plan.songs : [];
  if (!songs.length && !notices.length) return null;
  return (
    <section aria-label="Songs in Playback" className="space-y-1.5" data-testid="playback-library">
      {songs.length > 0 && (
        <ul aria-label="Songs in Playback" className="grid gap-1.5 p-0.5 [grid-template-columns:repeat(auto-fill,minmax(240px,1fr))]" data-testid="library-songs">
          {songs.map((song) => <SongChip key={`${song.key}-${song.plan_position}`} now={now} song={song} />)}
        </ul>
      )}
      {notices.length > 0 && (
        <ul aria-label="Playback library notices" className="space-y-0.5 text-xs" data-testid="library-notices">
          {notices.map((notice) => <li className={notice.actionable ? "text-warning-text" : "text-ink-muted"} data-kind={notice.kind} key={notice.id}>{notice.text}</li>)}
        </ul>
      )}
    </section>
  );
}
