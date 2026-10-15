import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import type { LightingMethod, LightingPad, PadColor } from "../types";
import { argumentToDraft, oscAddressError, parseOscArguments, type OscArgumentDraft } from "./lightingOsc";
import { OscArgumentRows } from "./LightingOscParts";
import {
  describePad,
  movePad,
  neutralPad,
  padColorClasses,
  padColors,
  padForShortcut,
  padIsActive,
  padIsDimmed,
} from "./lightingTimeline";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { Input } from "./ui/Input";
import { Select } from "./ui/Select";

const newPadId = () => {
  if (typeof globalThis.crypto?.randomUUID === "function") return globalThis.crypto.randomUUID();
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (value) => {
    const random = Math.floor(Math.random() * 16);
    return (value === "x" ? random : (random & 0x3) | 0x8).toString(16);
  });
};

interface PadDraft {
  id: string | null;
  label: string;
  color: PadColor | "";
  address: string;
  args: OscArgumentDraft[];
  note: string;
  velocity: string;
}

const emptyDraft = (): PadDraft => ({ id: null, label: "", color: "", address: "", args: [], note: "60", velocity: "127" });
const draftFromPad = (pad: LightingPad): PadDraft => ({
  id: pad.id,
  label: pad.label,
  color: pad.color ?? "",
  address: pad.address ?? "",
  args: pad.args.map(argumentToDraft),
  note: String(pad.note ?? 60),
  velocity: String(pad.velocity),
});

export type CaptureMode = "capture" | "preview";

/**
 * The cue board: saved, reusable pads. Pressing a pad fires it (and, while a song runs and capture is
 * on, the parent adds it to the timeline). Pads of the other method stay saved but dimmed and disabled.
 */
export function LightingCueBoard({
  pads, viewMethod, savedMethod, busy, captureMode, onCaptureMode, running, canUndo, status, onPress, onUndo, onSavePads,
}: {
  pads: LightingPad[];
  viewMethod: LightingMethod;
  savedMethod: LightingMethod;
  busy: boolean;
  captureMode: CaptureMode;
  onCaptureMode: (mode: CaptureMode) => void;
  /** True while the selected song's timer runs. */
  running: boolean;
  canUndo: boolean;
  /** The last confirmation, e.g. "Added at 01:12". */
  status: string | null;
  onPress: (pad: LightingPad) => void;
  onUndo: () => void;
  onSavePads: (pads: LightingPad[]) => Promise<boolean> | void;
}) {
  const [draft, setDraft] = useState<PadDraft | null>(null);
  const [attempted, setAttempted] = useState(false);
  const [editing, setEditing] = useState(false);
  const labelInput = useRef<HTMLInputElement>(null);
  useEffect(() => { if (draft) labelInput.current?.focus(); }, [draft?.id === null && draft !== null]); // eslint-disable-line react-hooks/exhaustive-deps

  const viewPads = pads.filter((pad) => pad.method === viewMethod);
  const dimmedPads = pads.filter((pad) => pad.method !== viewMethod);

  const labelError = draft && !draft.label.trim() ? "Name the pad." : null;
  const addressError = draft && viewMethod === "osc" ? oscAddressError(draft.address.trim()) : null;
  const parsedArgs = draft && viewMethod === "osc" ? parseOscArguments(draft.args) : { args: [], error: null };
  const noteNumber = draft ? Number(draft.note) : 0;
  const velocityNumber = draft ? Number(draft.velocity) : 0;
  const midiError = draft && viewMethod === "midi"
    ? !Number.isInteger(noteNumber) || noteNumber < 0 || noteNumber > 127
      ? "Enter a note from 0 to 127."
      : !Number.isInteger(velocityNumber) || velocityNumber < 1 || velocityNumber > 127
        ? "Enter a velocity from 1 to 127."
        : null
    : null;
  const draftError = labelError ?? addressError ?? parsedArgs.error ?? midiError;

  const commitDraft = async () => {
    if (!draft) return;
    setAttempted(true);
    if (draftError) return;
    const pad: LightingPad = {
      id: draft.id ?? newPadId(),
      label: draft.label.trim(),
      color: draft.color || null,
      method: viewMethod,
      address: viewMethod === "osc" ? draft.address.trim() : null,
      args: viewMethod === "osc" ? parsedArgs.args : [],
      note: viewMethod === "midi" ? noteNumber : null,
      velocity: viewMethod === "midi" ? velocityNumber : 127,
    };
    const next = draft.id === null ? [...pads, pad] : pads.map((item) => (item.id === pad.id ? pad : item));
    const saved = await onSavePads(next);
    if (saved !== false) { setDraft(null); setAttempted(false); }
  };

  const remove = (pad: LightingPad) => {
    void onSavePads(pads.filter((item) => item.id !== pad.id));
    if (draft?.id === pad.id) setDraft(null);
  };
  const reorder = (pad: LightingPad, delta: -1 | 1) => {
    // Move within the method on screen; pads of the other method keep their slots.
    const indexInView = viewPads.findIndex((item) => item.id === pad.id);
    const reordered = movePad(viewPads, indexInView, delta);
    if (reordered === viewPads) return;
    let cursor = 0;
    void onSavePads(pads.map((item) => (item.method === viewMethod ? reordered[cursor++]! : item)));
  };

  const onBoardKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    if (target.closest("input, select, textarea")) return;
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const pad = padForShortcut(viewPads, event.key);
    if (!pad || busy || !padIsActive(pad, viewMethod, savedMethod)) return;
    event.preventDefault();
    onPress(pad);
  };

  const captureHint = captureMode === "preview"
    ? "Preview only: pads fire but nothing is added to the song."
    : running
      ? "Pads fire and add a cue at the current time."
      : "No song is running: pads only fire.";

  return (
    <section aria-label="Cue board" className="lighting-board mt-5 border-t border-edge-subtle pt-5" data-testid="lighting-cue-board" onKeyDown={onBoardKey}>
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <h3 className="font-semibold text-ink">Cue board</h3>
        <div className="flex flex-wrap items-center gap-2">
          <label className="inline-flex min-h-[44px] cursor-pointer items-center gap-2 text-type-body-sm text-ink-body">
            <input
              checked={captureMode === "preview"}
              className="h-4 w-4"
              onChange={(event) => onCaptureMode(event.target.checked ? "preview" : "capture")}
              type="checkbox"
            />
            Preview only
          </label>
          <Button disabled={!canUndo || busy} onClick={onUndo} tone="lighting" variant="tinted">Undo last capture</Button>
          <Button aria-pressed={editing} onClick={() => { setEditing((value) => !value); setDraft(null); }} tone="lighting" variant="tinted">{editing ? "Done" : "Edit pads"}</Button>
        </div>
      </div>
      <p className="mt-1 text-type-body-sm text-ink-muted" data-testid="capture-hint">{captureHint} Keys 1-9 press a pad when the board has focus.</p>
      <p aria-live="polite" className="min-h-[1.5rem] text-type-body-sm text-ink-body" data-testid="capture-status" role="status">{status}</p>

      <div className="lighting-pads mt-2 grid gap-2" data-testid="lighting-pads" role="group" aria-label="Pads">
        {viewPads.map((pad, index) => {
          const colors = pad.color ? padColorClasses[pad.color] : null;
          const active = padIsActive(pad, viewMethod, savedMethod);
          return (
            <div className="flex min-w-0 flex-col gap-1" key={pad.id}>
              <button
                aria-keyshortcuts={index < 9 ? String(index + 1) : undefined}
                className={`min-h-[56px] w-full touch-manipulation rounded-ds-lg border px-3 py-2 text-left text-ink transition-colors duration-[140ms] motion-reduce:transition-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink disabled:cursor-not-allowed disabled:opacity-50 ${colors?.pad ?? neutralPad.pad}`}
                data-testid="lighting-pad"
                disabled={busy || !active}
                onClick={() => onPress(pad)}
                title={describePad(pad)}
                type="button"
              >
                <span className="block break-words font-semibold">{index < 9 && <span aria-hidden="true" className="mr-2 text-ink-muted">{index + 1}</span>}{pad.label}</span>
                <span className="block truncate font-mono text-type-body-sm text-ink-muted">{describePad(pad)}</span>
              </button>
              {editing && (
                <div className="flex flex-wrap gap-1">
                  <Button aria-label={`Move ${pad.label} earlier`} disabled={index === 0 || busy} onClick={() => reorder(pad, -1)} variant="secondary">←</Button>
                  <Button aria-label={`Move ${pad.label} later`} disabled={index === viewPads.length - 1 || busy} onClick={() => reorder(pad, 1)} variant="secondary">→</Button>
                  <Button aria-label={`Edit ${pad.label}`} disabled={busy} onClick={() => { setDraft(draftFromPad(pad)); setAttempted(false); }} tone="lighting" variant="tinted">Edit</Button>
                  <Button aria-label={`Delete ${pad.label}`} disabled={busy} onClick={() => remove(pad)} tone="danger" variant="tinted">Delete</Button>
                </div>
              )}
            </div>
          );
        })}
        {dimmedPads.map((pad) => (
          <div className="flex min-w-0 flex-col gap-1" key={pad.id}>
            <button
              aria-disabled="true"
              className="min-h-[56px] w-full cursor-not-allowed rounded-ds-lg border border-edge-subtle bg-surface-950 px-3 py-2 text-left text-ink-muted opacity-50"
              data-dimmed={padIsDimmed(pad, viewMethod)}
              data-testid="lighting-pad-dimmed"
              disabled
              title={`${pad.method.toUpperCase()} pad. Switch to ${pad.method.toUpperCase()} to use it.`}
              type="button"
            >
              <span className="block break-words font-semibold">{pad.label}</span>
              <span className="block truncate text-type-body-sm">{pad.method.toUpperCase()} pad, kept for later</span>
            </button>
            {editing && (
              <div className="flex flex-wrap gap-1">
                <Button aria-label={`Delete ${pad.label}`} disabled={busy} onClick={() => remove(pad)} tone="danger" variant="tinted">Delete</Button>
              </div>
            )}
          </div>
        ))}
        {!pads.length && <p className="text-type-body-sm text-ink-muted">No pads yet. Choose Edit pads, then Add pad.</p>}
      </div>

      {editing && !draft && (
        <div className="mt-3">
          <Button disabled={busy || pads.length >= 64} onClick={() => { setDraft(emptyDraft()); setAttempted(false); }} tone="lighting" variant="tinted">Add pad</Button>
        </div>
      )}
      {editing && draft && (
        <form
          aria-label={draft.id ? "Edit pad" : "New pad"}
          className="lighting-pad-form mt-3 grid gap-3 rounded-ds-lg border border-edge-medium bg-surface-panel p-3"
          onSubmit={(event) => { event.preventDefault(); void commitDraft(); }}
        >
          <div className="lighting-pad-fields grid gap-3">
            <Field error={attempted ? labelError ?? undefined : undefined} label="Pad name">
              <Input maxLength={60} onChange={(event) => setDraft({ ...draft, label: event.target.value })} placeholder="For example, Chorus wash…" ref={labelInput} value={draft.label} />
            </Field>
            <Field label="Colour">
              <Select onChange={(event) => setDraft({ ...draft, color: event.target.value as PadColor | "" })} value={draft.color}>
                <option value="">None</option>
                {padColors.map((color) => <option key={color} value={color}>{padColorClasses[color].label}</option>)}
              </Select>
            </Field>
            {viewMethod === "osc" ? (
              <Field error={attempted ? addressError ?? undefined : undefined} label="OSC address">
                <Input autoCapitalize="none" autoCorrect="off" className="font-mono" onChange={(event) => setDraft({ ...draft, address: event.target.value })} spellCheck={false} value={draft.address} />
              </Field>
            ) : (
              <>
                <Field label="MIDI note (0-127)">
                  <Input className="tabular-nums" inputMode="numeric" onChange={(event) => setDraft({ ...draft, note: event.target.value })} value={draft.note} />
                </Field>
                <Field error={attempted ? midiError ?? undefined : undefined} label="Velocity (1-127)">
                  <Input className="tabular-nums" inputMode="numeric" onChange={(event) => setDraft({ ...draft, velocity: event.target.value })} value={draft.velocity} />
                </Field>
              </>
            )}
          </div>
          {viewMethod === "osc" && <OscArgumentRows drafts={draft.args} label="Pad argument" onChange={(args) => setDraft({ ...draft, args })} />}
          {attempted && parsedArgs.error && <p className="text-type-body-sm text-danger-text" role="alert">{parsedArgs.error}</p>}
          <div className="flex flex-wrap gap-2">
            <Button disabled={busy} type="submit" variant="primary">{draft.id ? "Save pad" : "Add pad"}</Button>
            <Button onClick={() => { setDraft(null); setAttempted(false); }} variant="secondary">Cancel</Button>
          </div>
        </form>
      )}
    </section>
  );
}
