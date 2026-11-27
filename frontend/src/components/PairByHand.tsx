import { useId, useState } from "react";

import { clock, type RowPairing } from "./songMatchup";
import { Button } from "./ui/Button";
import { cx } from "./ui/cx";

/** What a row may do about a pairing; the Library Link controller gives it, a remote viewer gets `canPair: false`. */
export interface PairActions {
  canPair: boolean;
  busy: boolean;
  /** The words of the last pairing action that failed, or null. */
  error: string | null;
  pair: (planKey: string, setlistSongId: number) => Promise<boolean>;
  undo: (planKey: string) => Promise<boolean>;
}

const plainAction = "min-h-ds-touch-target touch-manipulation text-sm text-integration-playback-text underline disabled:opacity-60";

/**
 * The pairing of one plan song by hand, under its row in the Song order section (decided 2026-11-27): "Pick the Playback song" opens a short
 * list of the Playback songs that could go with it (the alike one first), a person picks one and presses Confirm; a pair a person made shows
 * "Paired by you" with Undo. Nothing is paired until Confirm. Wording is for the operator, never the backend's.
 */
export function PairByHand({ pairing, title, actions }: { pairing: RowPairing; title: string; actions: PairActions }) {
  const [open, setOpen] = useState(false);
  const [picked, setPicked] = useState<number | null>(null);
  // Only the row a person just acted on shows what went wrong (the controller keeps one error for the whole section).
  const [failed, setFailed] = useState(false);
  const group = useId();
  if (!actions.canPair) return null;

  const undo = async () => setFailed(!(await actions.undo(pairing.planKey)));
  if (pairing.byHand) {
    return (
      <div className="arr-pair flex flex-wrap items-center gap-x-3 gap-y-1 text-sm" data-testid="pair-by-hand">
        <button aria-label={`Undo the pair for ${title}`} className={plainAction} disabled={actions.busy} onClick={() => void undo()} type="button">Undo</button>
        {failed && actions.error && <span className="text-danger-text" role="alert">{actions.error}</span>}
      </div>
    );
  }
  if (!pairing.choices.length) {
    return <p className="arr-pair text-sm text-ink-muted" data-testid="pair-no-choices">Playback has no other song to pair</p>;
  }
  if (!open) {
    return (
      <div className="arr-pair flex flex-wrap items-center gap-x-3 gap-y-1">
        <button aria-label={`Pick the Playback song for ${title}`} className={plainAction} onClick={() => setOpen(true)} type="button">Pick the Playback song</button>
      </div>
    );
  }
  const close = () => { setOpen(false); setPicked(null); setFailed(false); };
  const confirm = async () => {
    if (picked === null) return;
    if (await actions.pair(pairing.planKey, picked)) close();
    else setFailed(true);
  };
  return (
    <fieldset className="arr-pair min-w-0 space-y-2 border-2 border-edge-medium bg-surface-950/40 p-3" data-testid="pair-chooser">
      <legend className="px-1 text-sm font-semibold text-ink">Which Playback song is “{title}”?</legend>
      <ul aria-label="Playback songs" className="m-0 grid list-none gap-1 p-0">
        {pairing.choices.map((choice) => {
          const id = `${group}-${choice.content_id}`;
          const ready = choice.setlist_song_id !== null;
          return (
            <li key={choice.content_id}>
              <label className={cx("flex min-h-ds-touch-target min-w-0 cursor-pointer items-center gap-3 border border-edge-medium px-3 py-1.5", picked === choice.setlist_song_id && ready ? "border-integration-playback-soft bg-integration-playback-soft/10" : "can-hover:bg-surface-900", !ready && "opacity-60")} htmlFor={id}>
                <input checked={ready && picked === choice.setlist_song_id} className="size-5 flex-none accent-integration-playback-soft" disabled={!ready || actions.busy} id={id} name={group} onChange={() => setPicked(choice.setlist_song_id)} type="radio" />
                <span className="min-w-0 flex-1 break-words">
                  <span className="font-semibold text-ink">{choice.title}</span>
                  {choice.suggested && <span className="ml-2 inline-block whitespace-nowrap border border-dashed border-ink-muted px-1.5 py-px text-xs uppercase tracking-[0.1em] text-ink">Looks alike</span>}
                </span>
                <span className="flex-none font-mono text-sm tabular-nums text-ink-body">{[choice.key, clock(choice.countdown_length_s)].filter(Boolean).join(" · ")}</span>
              </label>
            </li>
          );
        })}
      </ul>
      {failed && actions.error && <p className="text-sm text-danger-text" role="alert">{actions.error}</p>}
      <div className="flex flex-wrap items-center gap-2">
        <Button disabled={picked === null} loading={actions.busy} loadingLabel="Pairing…" onClick={() => void confirm()} size="touch" variant="primary">Confirm</Button>
        <Button disabled={actions.busy} onClick={close} size="touch" variant="secondary">Cancel</Button>
        <span className="text-xs text-ink-muted">Nothing changes in Playback.</span>
      </div>
    </fieldset>
  );
}
