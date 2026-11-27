import { playbackLibraryArtworkUrl } from "../api";
import type { MatchConfidence } from "../types";
import type { PlaybackDraft } from "./PlaybackSaveFooter";
import { ArrangementBar } from "./ArrangementBar";
import { CoverSlot } from "./CoverSlot";
import { bpmText, lengthSourceTitle } from "./playbackLibrary";
import { FactChips, type Fact } from "./sections";
import { PairByHand, type PairActions } from "./PairByHand";
import { clock, matchedWords, type MatchRow, type RowStamp } from "./songMatchup";
import { Switch } from "./ui/Switch";
import { cx } from "./ui/cx";

const kicker = "font-type-badge text-[0.6875rem] uppercase tracking-[0.1em]";
const cell = "flex min-w-0 flex-col gap-1.5 border-2 p-3";
const tag = "w-max border border-dashed border-ink-muted px-2 py-px text-xs uppercase tracking-[0.1em] text-ink";

/** The key as a chip: the bare letter (as everywhere since the Presenter Link's plain song details), `Key Ab` for a screen reader. */
const keyFact = (key: string): Fact => ({ text: key, name: `Key ${key}` });

/** A stamp says its state in words and a mark, never in colour alone (DESIGN.md law 3). */
const STAMP_STYLE: Record<RowStamp, { mark: string; box: string }> = {
  matched: { mark: "✓", box: "border-success-soft bg-success/40 text-success-text" },
  "check-key": { mark: "!", box: "border-warning bg-warning/10 text-warning-text" },
  "check-length": { mark: "!", box: "border-warning bg-warning/10 text-warning-text" },
  "check-title": { mark: "?", box: "border-warning bg-warning/10 text-warning-text" },
  "check-order": { mark: "!", box: "border-warning bg-warning/10 text-warning-text" },
  "only-plan": { mark: "−", box: "border-warning bg-warning/10 text-warning-text" },
  "only-playback": { mark: "+", box: "border-info bg-info/10 text-info-text" },
  skipped: { mark: "⌀", box: "border-dashed border-ink-muted text-ink-muted" },
};

export function Stamp({ stamp, confidence = null }: { stamp: RowStamp; confidence?: MatchConfidence | null }) {
  const style = STAMP_STYLE[stamp];
  return (
    <span className={cx("arr-stamp inline-flex items-center gap-1.5 border-2 bg-surface-950 px-2 py-0.5 font-pixel !text-type-badge uppercase tracking-wider", style.box)} data-stamp={stamp}>
      <span aria-hidden="true">{style.mark}</span>
      {matchedWords(stamp, confidence)}
    </span>
  );
}

/** The join mark between the two columns: a solid line and "=" for a pair that is fine, "≈" for a pair that is only alike, "≠" for a pair that will not line up, a dashed grey line when a side is skipped, nothing when nothing joins. */
function Join({ row }: { row: MatchRow }) {
  if (row.kind !== "pair") return <div aria-hidden="true" className="arr-link" data-none="" />;
  const sure = row.stamp === "matched" || row.stamp === "check-key" || row.stamp === "check-length";
  const skipped = row.stamp === "skipped";
  const glyph = skipped ? "–" : row.stamp === "check-title" ? "≈" : row.stamp === "check-order" ? "≠" : "=";
  return (
    <div aria-hidden="true" className="arr-link" data-sure={sure || undefined} data-skipped={skipped || undefined}>
      <i className={cx("relative z-10 grid size-8 place-items-center border-2 bg-surface-950 font-mono text-base font-bold not-italic", sure ? "border-integration-playback-soft text-integration-playback-text" : skipped ? "border-dashed border-ink-muted text-ink-muted" : "border-dashed border-warning text-warning-text")}>{glyph}</i>
    </div>
  );
}

/**
 * One line of the matchup ledger: plan song on the left, Playback song on the right, a join mark between, and above them the stamp with the
 * differences, in plain words, beside it. The line is one list item and reads aloud as ONE sentence (`row.sentence`); the drawn parts are
 * hidden from a screen reader so nothing is said twice, but the two Skip switches (and, with `pair`, the pairing by hand under the row) stay real controls. `hideKeyAndBpm` leaves out the key and
 * BPM chips under the songs (Playback's Song order section); the length beside each song stays, and so does every other use of the row.
 */
export function MatchupRow({ row, draft, skips, hideKeyAndBpm = false, pair }: { row: MatchRow; draft?: PlaybackDraft; skips: boolean; hideKeyAndBpm?: boolean; pair?: PairActions }) {
  const { plan, playback } = row;
  const spacer = (words: string) => <div aria-hidden="true" className="arr-spacer grid place-items-center border-2 border-dashed border-edge-medium p-3 text-center font-pixel !text-type-badge uppercase tracking-wider text-ink-muted">{words}</div>;
  const planBox = plan && (
    <div className={cx("arr-plan", cell, "rounded-none", plan.skipped ? "border-dashed border-ink-body/70 bg-ink/[0.03]" : "border-edge-medium bg-surface-950/40")} data-skipped={plan.skipped || undefined}>
      <div aria-hidden="true" className="flex min-w-0 flex-col gap-1.5">
        <div className="flex items-baseline justify-between gap-2"><span className={cx(kicker, "text-ink-muted")}>Song {plan.number} in plan</span><span className="font-mono tabular-nums text-ink-body">{clock(plan.seconds)}</span></div>
        <span className={cx("break-words font-semibold", plan.skipped ? "text-ink-muted line-through decoration-ink-muted/50" : "text-ink")}>{plan.title}</span>
        {plan.skipped && <span className={tag}>No track</span>}
      </div>
      <div className="mt-auto flex items-end justify-between gap-2">
        {plan.key && !hideKeyAndBpm ? <FactChips facts={[keyFact(plan.key)]} /> : <span aria-hidden="true" />}
        {skips && <Switch className="self-end" label="Skip" aria-label={`Skip ${plan.title}`} checked={plan.skipped} disabled={!draft} onChange={(event) => draft?.setPlanSongSkipped(plan.id, event.target.checked)} />}
      </div>
    </div>
  );
  const song = playback?.song ?? null;
  const cover = playback?.cover && song?.artwork && song.setlist_song_id !== null ? playbackLibraryArtworkUrl(song.setlist_song_id, song.artwork.etag) : null;
  const facts: Fact[] = song ? [...(song.key ? [keyFact(song.key)] : []), ...(song.bpm !== null ? [{ text: bpmText(song.bpm) }] : [])] : [];
  const playbackBox = playback && (
    <div className={cx("arr-pb", cell, "rounded-none", playback.skipped ? "border-dashed border-ink-body/70 bg-ink/[0.03]" : "border-integration-playback-soft/70 bg-integration-playback-soft/10")} data-skipped={playback.skipped || undefined}>
      <div aria-hidden="true" className="flex min-w-0 flex-col gap-1.5">
        <div className="flex items-baseline justify-between gap-2">
          <span className={cx(kicker, "text-integration-playback-text")}>Song {playback.number} in Playback</span>
          <span className="font-mono tabular-nums text-ink-body" title={(song && lengthSourceTitle(song)) ?? undefined}>{clock(playback.seconds)}</span>
        </div>
        <div className="flex min-w-0 items-center gap-2.5">
          <CoverSlot src={cover} />
          <span className={cx("min-w-0 break-words font-semibold", playback.skipped ? "text-ink-muted line-through decoration-ink-muted/50" : "text-ink")}>{playback.title ?? `Playback song ${playback.number}`}</span>
        </div>
        {song && <ArrangementBar sections={song.sections} />}
        {playback.skipped && <span className={tag}>Not in this service</span>}
      </div>
      <div className="mt-auto flex items-end justify-between gap-2">
        {facts.length && !hideKeyAndBpm ? <FactChips facts={facts} /> : <span aria-hidden="true" />}
        {skips && playback.skipId !== null && <Switch className="self-end" label="Skip" aria-label={playback.title ? `Skip Playback song ${playback.number}, ${playback.title}` : `Skip Playback song ${playback.number}`} checked={playback.skipped} disabled={!draft} onChange={(event) => draft?.setPlaybackSongSkipped(playback.skipId!, event.target.checked)} />}
      </div>
    </div>
  );
  return (
    <li className="arr-line" data-kind={row.kind} data-stamp={row.stamp} data-confidence={row.confidence ?? undefined}>
      <p className="sr-only">{row.sentence}</p>
      <div aria-hidden="true" className="arr-head">
        <Stamp stamp={row.stamp} confidence={row.confidence} />
        {row.notes.length > 0 && <ul className="arr-notes m-0 min-w-0 list-none space-y-0.5 p-0 text-sm">{row.notes.map((note) => <li key={note} className="break-words text-ink-body">{note}</li>)}</ul>}
      </div>
      {planBox ?? spacer("Not in the plan")}
      <Join row={row} />
      {playbackBox ?? spacer("No Playback song")}
      {pair && row.pairing && row.plan && <PairByHand actions={pair} pairing={row.pairing} title={row.plan.title} />}
    </li>
  );
}
