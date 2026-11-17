import { playbackLibraryArtworkUrl } from "../api";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { LibraryPlanSong, ServicePlan } from "../types";
import { ArrangementBar } from "./ArrangementBar";
import { chipLibrary, libraryNotices, songFaceFacts } from "./playbackLibrary";

/** What a chip says for a song Playback cannot be paired with (no cover, no bar). */
const unpaired = (song: LibraryPlanSong) =>
  song.status === "missing" ? "not in Playback's setlist" : "appears more than once, so StagePilot cannot tell which Playback song it is";

/**
 * One plan song as a chip: a 28 px cover (only when Playback has a picture), the title, key, BPM and countdown length, and under both a
 * thin bar of the arrangement. A song Playback is missing or cannot tell apart keeps its words, as a chip of the same shape.
 */
function SongChip({ song }: { song: LibraryPlanSong }) {
  const library = song.library;
  const cover = library?.artwork && library.setlist_song_id !== null ? playbackLibraryArtworkUrl(library.setlist_song_id, library.artwork.etag) : null;
  const facts = library ? songFaceFacts(library).join(" · ") || "no details" : unpaired(song);
  return (
    <li
      className="flex min-w-0 flex-col rounded-ds-xl border border-edge-medium bg-surface-panel px-3 py-2 text-sm"
      data-song={song.key}
      data-status={song.status}
      title={library ? chipLibrary(song)?.title : undefined}
    >
      <div className="flex min-w-0 items-center gap-2.5">
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
        <div className="min-w-0 leading-tight">
          <span className="block truncate font-semibold text-ink">{song.title}</span>
          <span className="block truncate text-xs tabular-nums text-ink-muted">{facts}</span>
        </div>
      </div>
      {library && <ArrangementBar sections={library.sections} />}
    </li>
  );
}

/**
 * What the Playback library (the network script) knows about the plan's songs, in the Playback API panel: one chip per plan song (a 28 px
 * cover, key, BPM, countdown length and a thin arrangement bar) and the notices (a song missing from Playback, a different order, a rental
 * that ends, another date). StagePilot only READS it: nothing here changes Playback, the plan, the saved scan, a timer or a light.
 *
 * It draws NOTHING unless the network script is ready (or outdated, which still works): a computer without the script looks exactly like
 * a build without the library. The notices that need action also go in the header pill; the length difference is information only.
 */
export function PlaybackLibrarySongs({ library, plan, timeZone }: { library: PlaybackLibraryController; plan: ServicePlan | null; timeZone: string }) {
  // The backend answers `ready` only while the network script is ready or outdated, so this one gate is the whole rule.
  const notices = libraryNotices(library.plan, plan, timeZone);
  const songs = library.plan?.enabled && library.plan.state === "ready" ? library.plan.songs : [];
  if (!songs.length && !notices.length) return null;
  return (
    <section aria-labelledby="playback-library-heading" className="space-y-2 rounded-ds-xl bg-integration-playback-soft/5 p-4" data-testid="playback-library">
      <h3 className="font-bold text-ink" id="playback-library-heading">Songs in Playback</h3>
      {songs.length > 0 && (
        <ul aria-label="Songs in Playback" className="grid gap-[10px] [grid-template-columns:repeat(auto-fill,minmax(270px,1fr))]" data-testid="library-songs">
          {songs.map((song) => <SongChip key={`${song.key}-${song.plan_position}`} song={song} />)}
        </ul>
      )}
      {notices.length > 0 && (
        <ul aria-label="Playback library notices" className="space-y-0.5 text-sm" data-testid="library-notices">
          {notices.map((notice) => <li className={notice.actionable ? "text-warning-text" : "text-ink-muted"} data-kind={notice.kind} key={notice.id}>{notice.text}</li>)}
        </ul>
      )}
    </section>
  );
}
