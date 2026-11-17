import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { PlaybackStatusResponse } from "../types";
import { MatchupRow } from "./MatchupRow";
import type { PlaybackDraft } from "./PlaybackSaveFooter";
import { PlaybackSetlistLine } from "./PlaybackSetlistLine";
import { classicRows, countStamps, libraryMatchesPlan, libraryRows, type MatchRow, type MatchupInput } from "./songMatchup";
import { InlineAlert } from "./ui/InlineAlert";
import { cx } from "./ui/cx";

const plural = (n: number, one: string, many: string) => (n === 1 ? one : many);

/**
 * Which Playback song goes with which service-plan song (DESIGN.md "Song arrangement"), as one ledger: plan on the left, Playback
 * on the right, a join mark between them and a stamp on every row. Songs are never reordered; each side can only skip songs, and
 * the Nth song left on one side pairs with the Nth left on the other. Skips are a held change, saved by Save configuration.
 *
 * Without the Playback library it is the saved scan alone ("Playback song N"). With the library (the network script answering)
 * the Playback side has real titles, keys, lengths and arrangements, paired by title with a stated confidence, plus the setlist
 * line and its chooser. The library only adds information: nothing is written to Planning Center or Playback, and a scan that
 * cannot be tied to the setlist leaves the plain scan view in place.
 */
export function PlaybackArrangement({ status, draft, library }: { status: PlaybackStatusResponse; draft?: PlaybackDraft; library?: PlaybackLibraryController }) {
  const plan = status.plan_songs ?? [];
  const order = status.song_order;
  const scan = order.length > 0 && !status.stale && plan.length > 0;
  const libraryPlan = library?.status?.active && plan.length ? library.plan : null;
  const useLibrary = libraryMatchesPlan(libraryPlan, plan);
  const noSetlist = Boolean(libraryPlan?.enabled) && libraryPlan?.state === "no_setlist";

  const planSkips = draft?.skippedPlanSongs ?? new Set(status.skipped_plan_song_ids ?? []);
  const playbackSkips = draft?.skippedPlaybackSongs ?? new Set(status.skipped_song_ids ?? []);
  const input: MatchupInput = { plan, order: scan ? order : [], lengths: status.song_lengths ?? [], planSkips, playbackSkips };

  let rows: MatchRow[] = [];
  let mode: "classic" | "library" | null = null;
  let live = false;
  let join: "id" | "position" | null = null;
  let notScanOfThisSetlist = false;
  if (useLibrary) {
    const view = libraryRows(input, libraryPlan!, scan);
    if (view) { rows = view.rows; mode = "library"; live = view.live; join = view.join; }
    else { rows = classicRows(input); mode = "classic"; notScanOfThisSetlist = true; }
  } else if (scan) {
    rows = classicRows(input);
    mode = "classic";
  }
  const showLine = Boolean(library && (noSetlist || useLibrary));
  // Nothing to arrange without a scan or the library's setlist, and no setlist line to show.
  if (!rows.length && !(showLine && noSetlist)) return null;

  const skips = mode === "classic" || live;
  const planIds = plan.map((song) => song.id);
  const missing = [...planSkips].filter((id) => !planIds.includes(id));
  const counts = countStamps(rows);
  const linked = rows.filter((row) => row.kind === "pair" && row.stamp !== "skipped" && row.confidence !== "uncertain").length;
  const skippedPlan = rows.filter((row) => row.stamp === "skipped" && row.plan).length;
  const skippedPlayback = rows.filter((row) => row.stamp === "skipped" && !row.plan && row.playback).length;
  const alone = counts["only-plan"] + (mode === "classic" ? counts["only-playback"] : 0);
  const far = counts["check-length"];
  const toCheck = counts["check-key"] + counts["check-length"] + counts["check-title"] + counts["check-order"] + counts["only-plan"];
  const held = Boolean(draft?.arrangementDirty);
  const linesUp = mode === "classic" ? alone === 0 && far === 0 : toCheck === 0;
  const badge = mode === "classic" ? (alone > 0 ? "Does not line up" : far > 0 ? `${far} to check` : "Lines up") : toCheck > 0 ? `${toCheck} to check` : "Lines up";

  return (
    <div className="song-arrangement min-w-0 space-y-3" role="group" aria-label="Song arrangement">
      {showLine && library && <PlaybackSetlistLine library={library} />}
      {rows.length > 0 && (
        <>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p className="min-w-0 text-sm text-ink-body" data-testid="arrangement-summary">
              {mode === "classic" ? (
                <>
                  <b className="text-ink">{linked}</b> of {plan.length} service song{plan.length === 1 ? " is" : "s are"} linked to Playback songs.
                  {skippedPlan > 0 && <> <b className="text-ink">{skippedPlan}</b> service {skippedPlan === 1 ? "song has" : "songs have"} no track.</>}
                  {skippedPlayback > 0 && <> <b className="text-ink">{skippedPlayback}</b> Playback {skippedPlayback === 1 ? "song is" : "songs are"} not in this service.</>}
                </>
              ) : (
                <>
                  <b className="text-ink">{linked}</b> of {plan.length} service {plural(plan.length, "song goes", "songs go")} with a Playback song.
                  {counts["only-plan"] > 0 && <> <b className="text-ink">{counts["only-plan"]}</b> {plural(counts["only-plan"], "is", "are")} only in the plan.</>}
                  {counts["only-playback"] > 0 && <> <b className="text-ink">{counts["only-playback"]}</b> {plural(counts["only-playback"], "is", "are")} only in Playback.</>}
                  {counts["check-title"] > 0 && <> <b className="text-ink">{counts["check-title"]}</b> {plural(counts["check-title"], "is", "are")} only alike by title.</>}
                  {skippedPlan + skippedPlayback > 0 && <> <b className="text-ink">{skippedPlan + skippedPlayback}</b> skipped.</>}
                </>
              )}
            </p>
            <span className={cx("flex-none border-2 bg-surface-950 px-2.5 py-0.5 font-pixel !text-type-badge uppercase tracking-wider", linesUp ? "border-success-soft text-success-text" : "border-warning/60 text-warning-text")} data-testid="arrangement-badge">{badge}</span>
          </div>
          {held && <p className="inline-flex items-center gap-1.5 text-type-body-sm text-warning-text" data-held-marker><span aria-hidden="true" className="size-1.5 rounded-full bg-warning" />Not saved yet</p>}
          {missing.length > 0 && (
            <InlineAlert tone="warning" className="p-3 text-ink-body" data-testid="arrangement-stale">
              <p className="font-semibold">The service plan changed</p>
              <p className="mt-1">{missing.length === 1 ? "A song you skipped is" : `${missing.length} songs you skipped are`} no longer in the service plan, so Playback won't start plan songs until this is reviewed.</p>
              <button type="button" className="mt-2 min-h-ds-touch-target touch-manipulation text-integration-playback-text underline" onClick={() => draft?.dropMissingPlanSkips(planIds)}>Forget {missing.length === 1 ? "it" : "them"}</button>
            </InlineAlert>
          )}
          {notScanOfThisSetlist && <p className="text-sm text-ink-body" data-testid="matchup-scan-note">The saved scan does not match the Playback setlist above, so this is the scan alone. Scan the Playback songs again to compare the two.</p>}
          {mode === "library" && !scan && <p className="text-sm text-ink-body" data-testid="matchup-readonly-note">Scan the Playback songs once to turn on Skip and to see which Playback song will start each timer. Until then this is only a comparison.</p>}
          {join === "position" && <p className="text-sm text-ink-body" data-testid="matchup-join-note">The saved scan could not be checked against the network script's song numbers, so its songs are lined up with Playback's setlist by place.</p>}
          {libraryPlan?.stale && libraryPlan.message && mode === "library" && <p className="text-sm text-ink-muted">{libraryPlan.message}</p>}
          {counts["check-order"] > 0 && (
            <InlineAlert tone="info" className="p-3 text-ink-body" data-testid="matchup-order-note">
              Songs never change places. {live ? "Playback's Nth song starts the plan's Nth song once the skipped songs are set aside." : "The plan and Playback list some songs in a different order."} Turn on Skip for a song that is only on one side, or change the order in Playback or Planning Center. StagePilot does not reorder anything for you.
            </InlineAlert>
          )}
          <div className="arr-cols font-type-kicker text-type-kicker uppercase text-ink-muted" aria-hidden="true"><span>Service plan</span><span /><span>Playback</span></div>
          <ol className="arr-list m-0 grid list-none gap-0 p-0" aria-label="Song pairs">
            {rows.map((row) => <MatchupRow key={row.id} row={row} draft={draft} skips={skips} />)}
          </ol>
          <p className="text-xs text-ink-muted">
            {mode === "classic"
              ? "Songs always stay in order. Turn on Skip to leave a song out; the songs after it move up to stay lined up. A skipped Playback song does nothing to the timer when it plays. Playback lengths are read from where each song ends (the start of its last measure)."
              : "Songs always stay in order. Titles are compared by their words; a pair that is only alike is marked for you to check. Lengths are where Playback's countdown ends."}
            {live && " Turn on Skip to leave a song out; the songs after it move up to stay lined up."}
          </p>
        </>
      )}
    </div>
  );
}
