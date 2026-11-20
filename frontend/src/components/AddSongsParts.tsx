import type { ReactNode } from "react";

import type { AddSongsController } from "../hooks/useAddSongs";
import { PLACE_END } from "../hooks/useAddSongs";
import type { AddSongsJob, AddSongsJobStep, AddSongsPlan, AddSongsReport, AddSongsSong } from "../propresenterLink";
import type { AddSongsRow } from "./addSongsSongs";
import {
  ACTION_STAMP, afterHeaderLabel, AT_THE_END, backupName, isStuck, KEY_NOTE, mappingLines, neighbours, NOTHING_CHANGES, OPEN_LIBRARY_ONCE, OUTCOME,
  playlistLabel, playlistSentence, resultCounts, SONG_STATUS, songSentence, STEP_STATE, STEP_WORDS,
} from "./addSongsWording";
import { Field } from "./ui/Field";
import { InlineAlert } from "./ui/InlineAlert";
import { Select } from "./ui/Select";
import { cx } from "./ui/cx";
import type { Tone } from "./ui/types";

/** A square stamp in the retro look (DESIGN.md \u201CSong arrangement\u201D): a mark and a word, never colour alone. */
const STAMP_TONE: Partial<Record<Tone, string>> = {
  success: "border-success-soft text-success-text",
  info: "border-info/60 text-info-text",
  warning: "border-warning/60 text-warning-text",
  danger: "border-danger/60 text-danger-text",
  idle: "border-edge-medium text-ink-muted",
};
export function Stamp({ tone, mark, word, testId }: { tone: Tone; mark: string; word: string; testId?: string }) {
  return (
    <span className={cx("inline-flex flex-none items-center gap-1 border-2 bg-surface-950 px-2 py-0.5 font-pixel !text-type-badge uppercase tracking-wider", STAMP_TONE[tone] ?? STAMP_TONE.idle)} data-testid={testId} data-tone={tone}>
      <span aria-hidden="true">{mark}</span>{word}
    </span>
  );
}

/** The key Playback plays a song in, as a small square chip (the same words as the matchup ledger: `Key Ab`). */
export function KeyChip({ keyName }: { keyName: string | null }) {
  return <span className="inline-flex flex-none items-center border border-edge-medium bg-surface-950 px-1.5 py-0.5 font-mono text-[0.8125rem] tabular-nums text-ink" data-testid="add-songs-key">{keyName ? `Key ${keyName}` : "No key"}</span>;
}

const linkButton = "min-h-ds-touch-target touch-manipulation text-integration-presentation-text underline transition-colors duration-[140ms] motion-reduce:transition-none can-hover:text-integration-presentation";

// ---- 1. the songs, 2. the playlist and the place -----------------------------------------------------------------------------

export function SongPicker({ add, headingId }: { add: AddSongsController; headingId: string }) {
  const { rows, unnamed, selected, selectedCount } = add;
  const all = selectedCount === rows.length;
  return (
    <section aria-labelledby={headingId} data-testid="add-songs-songs">
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <h3 className="font-semibold text-ink" id={headingId}>Songs</h3>
        <div className="flex flex-wrap items-center gap-x-3">
          <span className="text-sm tabular-nums text-ink-muted" data-testid="add-songs-count">{selectedCount} of {rows.length} ticked</span>
          <button className={linkButton} onClick={() => add.toggleAll(!all)} type="button">{all ? "Untick all" : "Tick all"}</button>
        </div>
      </div>
      <ul aria-label="Songs to add" className="mt-1 divide-y divide-dashed divide-edge-medium border-y border-edge-medium" data-testid="add-songs-list">
        {rows.map((row) => <SongRow checked={selected.has(row.ref)} key={row.ref} onToggle={() => add.toggle(row.ref)} row={row} />)}
        {unnamed.map((row) => (
          <li className="flex min-h-ds-touch-target flex-wrap items-center gap-x-3 gap-y-1 py-2 text-ink-muted" data-testid="add-songs-unnamed" key={`unnamed-${row.title}`}>
            <span className="min-w-0 flex-[1_1_12rem] break-words">{row.title}</span>
            <span className="text-sm">Playback gave this song no number, so it cannot be added.</span>
          </li>
        ))}
      </ul>
      <p className="mt-2 text-sm text-ink-muted" data-testid="add-songs-key-note">{KEY_NOTE}</p>
    </section>
  );
}

function SongRow({ row, checked, onToggle }: { row: AddSongsRow; checked: boolean; onToggle: () => void }) {
  return (
    <li>
      <label className="flex min-h-ds-touch-target cursor-pointer touch-manipulation flex-wrap items-center gap-x-3 gap-y-1 py-2">
        <input checked={checked} className="size-5 flex-none accent-integration-presentation" onChange={onToggle} type="checkbox" />
        <span className="min-w-0 flex-[1_1_12rem] break-words font-semibold text-ink">{row.title}</span>
        <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 pl-8 text-sm text-ink-body min-[560px]:pl-0">
          <KeyChip keyName={row.keyName} />
          <span data-testid="add-songs-arrangement">{row.arrangement ?? "No arrangement name"}</span>
        </span>
      </label>
    </li>
  );
}

export function PlaylistPicker({ add }: { add: AddSongsController }) {
  const { playlists, playlistsError, playlistId, playlist, place } = add;
  const busy = add.phase === "previewing";
  return (
    <section aria-label="Where the songs go" className="space-y-2" data-testid="add-songs-where">
      <div className="grid gap-3 min-[560px]:grid-cols-2">
        <Field label="Playlist">
          <Select disabled={busy || !playlists} loading={!playlists && !playlistsError} loadingLabel={"Loading playlists\u2026"} onChange={(event) => add.choosePlaylist(event.target.value)} placeholder={"Choose a playlist\u2026"} value={playlistId}>
            {playlists?.map((candidate) => <option key={candidate.id} value={candidate.id}>{playlistLabel(candidate)}</option>)}
          </Select>
        </Field>
        <Field label="Place">
          <Select disabled={busy || !playlist} onChange={(event) => add.choosePlace(event.target.value)} value={playlist ? place : PLACE_END}>
            {playlist?.headers.map((header, index) => <option key={`${index}-${header}`} value={header}>{afterHeaderLabel(header)}</option>)}
            <option value={PLACE_END}>{AT_THE_END}</option>
          </Select>
        </Field>
      </div>
      {playlistsError && (
        <InlineAlert className="flex flex-wrap items-center gap-x-3 gap-y-1" tone="danger">
          <span className="min-w-0 flex-[1_1_14rem]">{playlistsError}</span>
          <button className={linkButton} onClick={() => void add.reloadPlaylists()} type="button">Look again</button>
        </InlineAlert>
      )}
    </section>
  );
}

// ---- 3. the preview ----------------------------------------------------------------------------------------------------------

function mappingTone(song: AddSongsSong): string {
  return song.mapping.some((m) => m.how === "ambiguous" || m.how === "missing") ? "text-warning-text" : "text-ink-muted";
}

export function PreviewSong({ song, row, skipped = false }: { song: AddSongsSong; row: AddSongsRow | undefined; skipped?: boolean }) {
  const stamp = ACTION_STAMP[song.action];
  const lines = mappingLines(song);
  // The engine's notes repeat the sentences of a match that was not the plain one: say each once.
  const notes = song.notes.filter((note) => !lines.includes(note));
  return (
    <li className="space-y-1 py-2" data-action={song.action} data-testid="add-songs-preview-song">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="min-w-0 flex-[1_1_12rem] break-words font-semibold text-ink">{song.title}</span>
        <Stamp mark={stamp.mark} tone={stamp.tone} word={stamp.word} />
      </div>
      <p className="break-words text-sm text-ink-body" data-testid="add-songs-song-sentence">{songSentence(song, row?.song)}</p>
      {skipped && <p className="text-sm font-semibold text-ink" data-testid="add-songs-song-skipped">\u2212 This song will be skipped.</p>}
      {song.source && (song.action === "copy" || song.action === "import_then_copy") && <p className="break-words text-xs text-ink-muted">Made from {song.source}.</p>}
      {lines.length > 0 && (
        <ul aria-label={`How ${song.title} was matched`} className={cx("space-y-0.5 text-xs", mappingTone(song))} data-testid="add-songs-mapping">
          {lines.map((line) => <li className="break-words" key={line}>{line}</li>)}
        </ul>
      )}
      {notes.length > 0 && (
        <ul aria-label={`Notes about ${song.title}`} className="space-y-0.5 text-xs text-ink-muted" data-testid="add-songs-notes">
          {notes.map((note) => <li className="break-words" key={note}>{note}</li>)}
        </ul>
      )}
    </li>
  );
}

export function PreviewBody({ add, plan, headingId }: { add: AddSongsController; plan: AddSongsPlan; headingId: string }) {
  const header = add.place === PLACE_END ? null : add.place;
  const around = neighbours(plan.playlist);
  const byRef = new Map(add.rows.map((row) => [row.ref, row]));
  return (
    <section aria-labelledby={headingId} className={cx("space-y-2", add.stale && "opacity-60")} data-stale={add.stale || undefined} data-testid="add-songs-preview">
      <h3 className="font-semibold text-ink" id={headingId} tabIndex={-1}>Preview</h3>
      {add.stale && <p className="text-sm font-semibold text-warning-text" data-testid="add-songs-stale">You changed something since this preview. Press Preview again.</p>}
      <p className="text-sm font-semibold text-ink" data-testid="add-songs-nothing-changes">{NOTHING_CHANGES}</p>
      <p className="text-sm text-ink-body" data-testid="add-songs-playlist-line">
        {playlistSentence(plan, header)}{around && plan.to_add > 0 ? ` ${around}` : ""}
      </p>
      {plan.playlist.blocked && <InlineAlert tone="warning"><span data-testid="add-songs-playlist-blocked">Blocked: {plan.playlist.blocked}</span></InlineAlert>}
      {plan.stuck > 0 && (
        <label className="flex min-h-ds-touch-target cursor-pointer touch-manipulation items-center gap-3 text-sm text-ink-body">
          <input checked={add.skipBlocked} className="size-5 flex-none accent-integration-presentation" data-testid="add-songs-skip" onChange={(event) => add.setSkipBlocked(event.target.checked)} type="checkbox" />
          Skip blocked songs ({plan.stuck})
        </label>
      )}
      <ol aria-label="What will happen to each song" className="divide-y divide-dashed divide-edge-medium border-y border-edge-medium" data-testid="add-songs-preview-list">
        {plan.songs.map((song) => <PreviewSong key={song.ref_id} row={byRef.get(song.ref_id)} skipped={add.skipBlocked && isStuck(song)} song={song} />)}
      </ol>
      {plan.warnings.length > 0 && (
        <ul aria-label="Things to know" className="space-y-0.5 text-sm text-warning-text" data-testid="add-songs-warnings">
          {plan.warnings.map((warning) => <li key={warning}>{warning}</li>)}
        </ul>
      )}
      {plan.propresenter && <p className="text-xs text-ink-muted" data-testid="add-songs-version">{plan.propresenter}</p>}
    </section>
  );
}

// ---- 4. the job and its result -----------------------------------------------------------------------------------------------

function StepLine({ step }: { step: AddSongsJobStep }) {
  const state = STEP_STATE[step.state];
  return (
    <li className="flex flex-wrap items-baseline gap-x-2 text-sm" data-state={step.state} data-step={step.step}>
      <span aria-hidden="true" className="w-4 flex-none text-center font-mono">{state.mark}</span>
      <span className="font-semibold text-ink">{STEP_WORDS[step.step]}</span>
      <span className="text-ink-muted">{state.word}</span>
      {step.text && <span className="min-w-0 flex-[1_1_100%] break-words pl-6 text-ink-body">{step.text}</span>}
      {step.notes.map((note) => <span className="min-w-0 flex-[1_1_100%] break-words pl-6 text-xs text-ink-muted" key={note}>{note}</span>)}
    </li>
  );
}

export function Progress({ add, plan, headingId }: { add: AddSongsController; plan: AddSongsPlan | null; headingId: string }) {
  const job: AddSongsJob | null = add.job;
  // Before the first answer the songs are the plan's: the list does not grow when the first answer arrives.
  const songs = job?.songs ?? (plan?.songs.filter((song) => !add.skipBlocked || (song.action !== "blocked" && song.action !== "needs_choice")).map((song) => ({ ref_id: song.ref_id, title: song.title, steps: [], result: null })) ?? []);
  const last = job?.events[job.events.length - 1];
  return (
    <section aria-labelledby={headingId} className="space-y-2" data-testid="add-songs-progress">
      <h3 className="font-semibold text-ink" id={headingId} tabIndex={-1}>Adding songs</h3>
      <p className="min-h-[1.5rem] text-sm text-ink-body" data-testid="add-songs-now">{last ? `Now: ${last.text}` : "Starting\u2026"}</p>
      {add.lostContact && <InlineAlert tone="warning">StagePilot lost contact with itself for a moment. It keeps trying; the songs are still being added.</InlineAlert>}
      {job && job.steps.length > 0 && (
        <ol aria-label="Steps for the playlist" className="space-y-1" data-testid="add-songs-steps">
          {job.steps.map((step) => <StepLine key={step.step} step={step} />)}
        </ol>
      )}
      <ol aria-label="Songs being added" className="divide-y divide-dashed divide-edge-medium border-y border-edge-medium" data-testid="add-songs-job-songs">
        {songs.map((song) => (
          <li className="space-y-1 py-2" data-testid="add-songs-job-song" key={song.ref_id}>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
              <span className="min-w-0 flex-[1_1_12rem] break-words font-semibold text-ink">{song.title}</span>
              {song.result && <Stamp mark={SONG_STATUS[song.result.status].mark} tone={SONG_STATUS[song.result.status].tone} word={SONG_STATUS[song.result.status].word} />}
            </div>
            {song.steps.length > 0 && <ul aria-label={`Steps for ${song.title}`} className="space-y-1">{song.steps.map((step) => <StepLine key={step.step} step={step} />)}</ul>}
          </li>
        ))}
      </ol>
    </section>
  );
}

export function Result({ add, headingId }: { add: AddSongsController; headingId: string }) {
  const report: AddSongsReport | null = add.report;
  if (!report) {
    return (
      <section aria-labelledby={headingId} className="space-y-2" data-outcome="stopped" data-testid="add-songs-result">
        <h3 className="font-semibold text-warning-text" id={headingId} tabIndex={-1}><span aria-hidden="true">! </span>The job stopped</h3>
        <p className="text-sm text-ink-body" data-testid="add-songs-result-message">{add.job?.error ?? "The job stopped unexpectedly. Look at the playlist in ProPresenter."}</p>
      </section>
    );
  }
  const words = OUTCOME[report.outcome];
  const backup = backupName(report.backup);
  const counts = resultCounts(report);
  const heading: Record<string, string> = { success: "text-success-text", warning: "text-warning-text", danger: "text-danger-text", info: "text-info-text" };
  return (
    <section aria-labelledby={headingId} className="space-y-2" data-outcome={report.outcome} data-testid="add-songs-result">
      <h3 className={cx("font-semibold", heading[words.tone] ?? "text-ink")} id={headingId} tabIndex={-1}><span aria-hidden="true">{words.mark} </span>{words.heading}</h3>
      <p className="text-sm text-ink-body" data-testid="add-songs-result-message">{report.message}</p>
      {report.outcome === "needs_library_open" && (
        <InlineAlert tone="warning"><span data-testid="add-songs-open-library">{OPEN_LIBRARY_ONCE}</span></InlineAlert>
      )}
      {counts && <p className="text-sm tabular-nums text-ink-muted" data-testid="add-songs-counts">{counts}</p>}
      <ul aria-label="Result for each song" className="divide-y divide-dashed divide-edge-medium border-y border-edge-medium" data-testid="add-songs-result-songs">
        {report.songs.map((song) => {
          const status = SONG_STATUS[song.status];
          return (
            <li className="space-y-1 py-2" data-status={song.status} data-testid="add-songs-result-song" key={song.ref_id}>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                <span className="min-w-0 flex-[1_1_12rem] break-words font-semibold text-ink">{song.title}</span>
                <Stamp mark={status.mark} tone={status.tone} word={status.word} />
              </div>
              {song.text && <p className="break-words text-sm text-ink-body">{song.text}</p>}
            </li>
          );
        })}
      </ul>
      {report.differences.length > 0 && (
        <div data-testid="add-songs-differences">
          <p className="text-sm font-semibold text-warning-text">ProPresenter shows something different from what was planned:</p>
          <ul className="mt-1 list-disc space-y-0.5 pl-5 text-sm text-ink-body">{report.differences.map((difference) => <li className="break-words" key={difference}>{difference}</li>)}</ul>
        </div>
      )}
      {backup && <p className="break-words text-sm text-ink-body" data-testid="add-songs-backup">The playlist was saved first as <code className="font-mono text-ink">{backup}</code>, beside StagePilot&apos;s settings.</p>}
      {report.version && <p className="text-xs text-ink-muted">{report.version}</p>}
    </section>
  );
}

/** A footer line for the buttons: why one of them is off, in words. */
export function Reason({ id, children }: { id: string; children: ReactNode }) {
  return <p className="min-w-0 break-words text-sm text-ink-body max-[480px]:col-span-3 min-[481px]:mr-auto min-[481px]:!w-auto min-[481px]:flex-[1_1_0%] min-[481px]:self-center" data-testid="add-songs-reason" id={id}>{children}</p>;
}
