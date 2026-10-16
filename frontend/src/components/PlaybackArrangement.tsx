import type { PlaybackDraft } from "./PlaybackSaveFooter";
import { arrangeSongs, CLOSE_SECONDS, LIKELY_DIFFERENT_SECONDS, secondsApart, type ArrangementLine } from "./playbackArrangement";
import { InlineAlert } from "./ui/InlineAlert";
import { Switch } from "./ui/Switch";
import { cx } from "./ui/cx";
import type { PlaybackStatusResponse } from "../types";

const known = (seconds: number | null | undefined): seconds is number => typeof seconds === "number" && Number.isFinite(seconds) && seconds >= 0;

function duration(seconds: number | null | undefined): string {
  if (!known(seconds)) return "Unknown";
  const rounded = Math.floor(seconds + 0.5);
  return `${Math.floor(rounded / 60)}:${String(rounded % 60).padStart(2, "0")}`;
}

const cell = "flex min-w-0 flex-col gap-1.5 rounded-ds-lg border p-3";
const kicker = "font-type-badge text-[0.6875rem] uppercase tracking-[0.1em]";
const tag = "w-max rounded-md border border-dashed border-ink-muted px-2 py-px text-xs uppercase tracking-[0.1em] text-ink";

/**
 * Which Playback song goes with which service-plan song (DESIGN.md "Song arrangement"). Songs are never reordered;
 * each side can only skip songs, and the Nth song left on one side pairs with the Nth left on the other.
 * Skips are a held change: they are saved by Save configuration, like every other field in the panel.
 */
export function PlaybackArrangement({ status, draft }: { status: PlaybackStatusResponse; draft?: PlaybackDraft }) {
  const plan = status.plan_songs ?? [];
  const order = status.song_order;
  const lengths = status.song_lengths ?? [];
  // Nothing to arrange without a scanned order and a loaded service plan.
  if (!order.length || status.stale || !plan.length) return null;

  const planSkips = draft?.skippedPlanSongs ?? new Set(status.skipped_plan_song_ids ?? []);
  const playbackSkips = draft?.skippedPlaybackSongs ?? new Set(status.skipped_song_ids ?? []);
  const planIds = plan.map((song) => song.id);
  const lines = arrangeSongs(planIds, order.length, planSkips, playbackSkips, order);
  const pairs = lines.filter((line) => line.kind === "pair").length;
  const alone = lines.filter((line) => line.kind === "plan-alone" || line.kind === "playback-alone").length;
  const missing = [...planSkips].filter((id) => !planIds.includes(id));
  const skippedPlan = lines.filter((line) => line.kind === "plan-skip").length;
  const skippedPlayback = lines.filter((line) => line.kind === "playback-skip").length;
  const far = lines.filter((line) => line.kind === "pair" && (secondsApart(plan[line.plan]?.duration_seconds, lengths[line.playback]) ?? 0) > LIKELY_DIFFERENT_SECONDS).length;
  const held = Boolean(draft?.arrangementDirty);
  const lines_up = alone === 0 && far === 0;
  const badge = alone > 0 ? "Does not line up" : far > 0 ? `${far} to check` : "Lines up";

  let linkNumber = 0;
  const rows = lines.map((line) => {
    const key = `${line.kind}-${"plan" in line ? line.plan : ""}-${"playback" in line ? line.playback : ""}`;
    if (line.kind === "pair") linkNumber += 1;
    return <ArrangementRow key={key} line={line} number={line.kind === "pair" ? linkNumber : 0} status={status} draft={draft} />;
  });

  return (
    <div className="song-arrangement min-w-0 space-y-3" role="group" aria-label="Song arrangement">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="min-w-0 text-sm text-ink-body" data-testid="arrangement-summary">
          <b className="text-ink">{pairs}</b> of {plan.length} service song{plan.length === 1 ? " is" : "s are"} linked to Playback songs.
          {skippedPlan > 0 && <> <b className="text-ink">{skippedPlan}</b> service {skippedPlan === 1 ? "song has" : "songs have"} no track.</>}
          {skippedPlayback > 0 && <> <b className="text-ink">{skippedPlayback}</b> Playback {skippedPlayback === 1 ? "song is" : "songs are"} not in this service.</>}
        </p>
        <span className={cx("flex-none rounded-full border px-2.5 py-0.5 text-xs", lines_up ? "border-success-soft bg-success text-ink" : "border-warning/50 text-warning-text")} data-testid="arrangement-badge">{badge}</span>
      </div>
      {held && <p className="inline-flex items-center gap-1.5 text-type-body-sm text-warning-text" data-held-marker><span aria-hidden="true" className="size-1.5 rounded-full bg-warning" />Not saved yet</p>}
      {missing.length > 0 && (
        <InlineAlert tone="warning" className="p-3 text-ink-body" data-testid="arrangement-stale">
          <p className="font-semibold">The service plan changed</p>
          <p className="mt-1">{missing.length === 1 ? "A song you skipped is" : `${missing.length} songs you skipped are`} no longer in the service plan, so Playback won't start plan songs until this is reviewed.</p>
          <button type="button" className="mt-2 min-h-ds-touch-target touch-manipulation text-integration-playback-text underline" onClick={() => draft?.dropMissingPlanSkips(planIds)}>Forget {missing.length === 1 ? "it" : "them"}</button>
        </InlineAlert>
      )}
      <div className="arr-cols font-type-kicker text-type-kicker uppercase text-ink-muted" aria-hidden="true"><span>Service plan</span><span /><span>Playback</span></div>
      <ol className="arr-list m-0 grid list-none gap-3 p-0" aria-label="Song pairs">{rows}</ol>
      <p className="text-xs text-ink-muted">Songs always stay in order. Turn on Skip to leave a song out; the songs after it move up to stay lined up. A skipped Playback song does nothing to the timer when it plays. Playback lengths are read from where each song ends (the start of its last measure).</p>
    </div>
  );
}

function ArrangementRow({ line, number, status, draft }: { line: ArrangementLine; number: number; status: PlaybackStatusResponse; draft?: PlaybackDraft }) {
  const plan = status.plan_songs ?? [];
  const lengths = status.song_lengths ?? [];
  const planBox = (index: number, skipped: boolean, note?: string) => {
    const song = plan[index]!;
    return (
      <div className={cx("arr-plan", cell, skipped ? "border-2 border-dashed border-ink-body/70 bg-ink/[0.03]" : "border-edge-subtle bg-surface-950/40")} data-skipped={skipped || undefined}>
        <div className="flex items-baseline justify-between gap-2"><span className={cx(kicker, "text-ink-muted")}>Song {index + 1} in plan</span><span className="tabular-nums text-ink-body">{duration(song.duration_seconds)}</span></div>
        <span className={cx("break-words font-semibold", skipped ? "text-ink-muted line-through decoration-ink-muted/50" : "text-ink")}>{song.title}</span>
        {skipped && <span className={tag}>No track</span>}
        {note && <span className="text-[0.8125rem] text-warning-text">{note}</span>}
        <Switch className="mt-auto self-end" label="Skip" aria-label={`Skip ${song.title}`} checked={skipped} disabled={!draft} onChange={(event) => draft?.setPlanSongSkipped(song.id, event.target.checked)} />
      </div>
    );
  };
  const playbackBox = (index: number, skipped: boolean, apart?: ReactApart, note?: string) => {
    const id = status.song_order[index]!;
    return (
      <div className={cx("arr-pb", cell, skipped ? "border-2 border-dashed border-ink-body/70 bg-ink/[0.03]" : "border-integration-playback-soft/55 bg-integration-playback-soft/10")} data-skipped={skipped || undefined}>
        <div className="flex items-baseline justify-between gap-2"><span className={cx(kicker, "text-integration-playback-text")}>Song {index + 1} in Playback</span><span className="tabular-nums text-ink-body">{duration(lengths[index])}</span></div>
        <span className={cx("break-words font-semibold", skipped ? "text-ink-muted line-through decoration-ink-muted/50" : "text-ink")}>Playback song {index + 1}</span>
        {skipped && <span className={tag}>Not in this service</span>}
        {apart && <span className={cx("text-[0.8125rem] tabular-nums", apart.tone)}>{apart.text}</span>}
        {note && <span className="text-[0.8125rem] text-warning-text">{note}</span>}
        <Switch className="mt-auto self-end" label="Skip" aria-label={`Skip Playback song ${index + 1}`} checked={skipped} disabled={!draft} onChange={(event) => draft?.setPlaybackSongSkipped(id, event.target.checked)} />
      </div>
    );
  };
  const link = (n: number) => (n ? <div className="arr-link" aria-hidden="true"><i className="relative z-10 grid size-8 place-items-center rounded-full border-2 border-integration-playback-soft bg-surface-950 text-sm font-bold not-italic text-integration-playback-text">{n}</i></div> : <div className="arr-link" data-none="" aria-hidden="true" />);
  const spacer = <div className="arr-spacer" aria-hidden="true" />;

  if (line.kind === "pair") {
    const apart = secondsApart(plan[line.plan]?.duration_seconds, lengths[line.playback]);
    const text = apart === null ? undefined : apart === 0 ? { text: "Same length", tone: "text-success-text" }
      : apart <= CLOSE_SECONDS ? { text: `${apart} s apart`, tone: "text-success-text" }
        : apart <= LIKELY_DIFFERENT_SECONDS ? { text: `${apart} s apart`, tone: "text-ink-body" }
          : { text: `${apart} s apart. Probably a different song.`, tone: "text-warning-text" };
    return <li className="arr-line" data-pair>{planBox(line.plan, false)}{link(number)}{playbackBox(line.playback, false, text)}</li>;
  }
  if (line.kind === "plan-skip") return <li className="arr-line" data-lone>{planBox(line.plan, true)}{link(0)}{spacer}</li>;
  if (line.kind === "plan-alone") return <li className="arr-line" data-lone>{planBox(line.plan, false, "No Playback song left to pair with.")}{link(0)}{spacer}</li>;
  if (line.kind === "playback-skip") return <li className="arr-line" data-lone>{spacer}{link(0)}{playbackBox(line.playback, true)}</li>;
  return <li className="arr-line" data-lone>{spacer}{link(0)}{playbackBox(line.playback, false, undefined, "No service song left to pair with.")}</li>;
}

interface ReactApart { text: string; tone: string }
