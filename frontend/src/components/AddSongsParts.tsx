import { Fragment, type ReactNode } from "react";

import type { AddSongsController } from "../hooks/useAddSongs";
import { hasSections, PLACE_END } from "../hooks/useAddSongs";
import type { AddSongsPlan } from "../propresenterLink";
import { placeHeading, songStatus, summaryParts } from "./addSongsStatus";
import { END_OF_PLAYLIST, isStuck, neighbours, playlistLabel, STALE_WORDS, twoHeadersLine } from "./addSongsWording";
import { StatusChip, StatusRow } from "./StatusChips";
import { ReasonPill, SectionNotice, SettingsSection } from "./sections";
import { Field } from "./ui/Field";
import { Select } from "./ui/Select";
import { cx } from "./ui/cx";

export { GuessNote, Progress, Result } from "./AddSongsJobParts";

export const linkButton = "min-h-ds-touch-target touch-manipulation text-integration-presentation-text underline transition-colors duration-[140ms] motion-reduce:transition-none can-hover:text-integration-presentation";

// ---- 1. the playlist and the section (the songs list is `PresenterSongList`) -----------------------------------------------------

/**
 * The Section box: the playlist's section (header) names in the playlist's order; End of Playlist only for a playlist with no header (then
 * it is the one option). A section is every item after its header up to the next header; the songs go at the END of the chosen section
 * (right before the next header, or at the end of the playlist for the last one, which is why the last section is the default).
 * Two headers with the same name are both listed; choosing that name shows the engine's line under the box.
 */
export function PlaylistPicker({ add, title }: { add: AddSongsController; title?: string }) {
  const { playlists, playlistsError, playlistId, playlist, place, placeTwice } = add;
  const busy = add.phase === "previewing" || add.phase === "applying";
  return (
    <>
      <SettingsSection aria-label={title ? undefined : "Where the songs go"} busy={busy} columns="pair" testId="add-songs-where" title={title}>
        <Field label="Playlist">
          <Select disabled={!playlists} loading={!playlists && !playlistsError} loadingLabel={"Loading playlists\u2026"} onChange={(event) => add.choosePlaylist(event.target.value)} placeholder={"Choose a playlist\u2026"} value={playlistId}>
            {playlists?.map((candidate) => <option key={candidate.id} value={candidate.id}>{playlistLabel(candidate)}</option>)}
          </Select>
        </Field>
        <Field label="Section">
          <Select disabled={!playlist} onChange={(event) => add.choosePlace(event.target.value)} placeholder={playlist ? undefined : "\u2014"} value={playlist ? place : ""}>
            {playlist && (hasSections(playlist)
              ? playlist.headers.map((header, index) => <option key={`${index}-${header}`} value={header}>{header}</option>)
              : <option value={PLACE_END}>{END_OF_PLAYLIST}</option>)}
          </Select>
        </Field>
      </SettingsSection>
      {placeTwice && <SectionNotice role="alert" size="xs" testId="add-songs-two-headers" tone="warning" variant="line">{twoHeadersLine(place)}</SectionNotice>}
      {playlistsError && (
        <SectionNotice actions={<button className={linkButton} onClick={() => void add.reloadPlaylists()} type="button">Look again</button>} tone="danger">
          {playlistsError}
        </SectionNotice>
      )}
    </>
  );
}

// ---- 3. the preview ----------------------------------------------------------------------------------------------------------

/** One song of the preview: its title, ONE chip, at most one short phrase. The long sentences are the chip's hover text and the screen-reader text. */
export function PreviewSong({ song, skipped = false }: { song: AddSongsPlan["songs"][number]; skipped?: boolean }) {
  const status = songStatus(song, skipped);
  return (
    <StatusRow
      attrs={{ "data-action": song.action, "data-status": status.key }}
      chip={<StatusChip column mark={status.mark} testId="add-songs-status" title={status.detail} tone={status.tone} word={status.word} />}
      detail={status.detail}
      files={status.files}
      phrase={status.phrase}
      testId="add-songs-preview-song"
      title={song.title}
    />
  );
}

/**
 * The preview, short: where the songs go (`Sunday Morning Service \u203A Worship`), ONE neutral line of counts, then one row per song (title,
 * chip, phrase). Red only when something stops the whole import (the playlist). No sentences: the full text is for hover and screen readers.
 */
export function PreviewBody({ add, plan, headingId }: { add: AddSongsController; plan: AddSongsPlan; headingId: string }) {
  const header = add.place === PLACE_END ? null : add.place;
  const around = neighbours(plan.playlist);
  const statuses = plan.songs.map((song) => songStatus(song, add.skipBlocked && isStuck(song)).key);
  const blocked = plan.playlist.blocked;
  return (
    <section aria-labelledby={headingId} className={cx("space-y-2", add.stale && "opacity-60")} data-stale={add.stale || undefined} data-testid="add-songs-preview">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
        <h3 className="font-semibold text-ink" id={headingId} tabIndex={-1}>Preview</h3>
        <span className="min-w-0 break-words text-sm text-ink-body" data-testid="add-songs-playlist-line" title={around ?? undefined}>{placeHeading(plan, header, END_OF_PLAYLIST)}</span>
      </div>
      {add.stale && <p className="text-sm font-semibold text-warning-text" data-testid="add-songs-stale">{STALE_WORDS}</p>}
      <p className={cx("text-sm tabular-nums", blocked ? "font-semibold text-danger-text" : "text-ink")} data-testid="add-songs-summary">{blocked ? "Playlist is blocked" : summaryParts(statuses).map((part, index) => <Fragment key={part}>{index > 0 && " \u00B7 "}<span className="whitespace-nowrap">{part}</span></Fragment>)}</p>
      {blocked && <span className="sr-only" data-testid="add-songs-playlist-blocked">{blocked}</span>}
      <ol aria-label="What will happen to each song" className="divide-y divide-dashed divide-edge-medium border-y border-edge-medium" data-testid="add-songs-preview-list">
        {plan.songs.map((song) => <PreviewSong key={song.ref_id} skipped={add.skipBlocked && isStuck(song)} song={song} />)}
      </ol>
      {plan.warnings.length > 0 && (
        <ul aria-label="Things to know" className="sr-only" data-testid="add-songs-warnings">
          {plan.warnings.map((warning) => <li key={warning}>{warning}</li>)}
        </ul>
      )}
    </section>
  );
}

/** Beside the button, when something stops it: why, in a few words, as the shared neutral pill (`ReasonPill`). `id` is what the button's `aria-describedby` points at. */
export function Reason({ id, children }: { id: string; children?: ReactNode }) {
  return <ReasonPill id={id} testId="add-songs-reason">{children}</ReasonPill>;
}
