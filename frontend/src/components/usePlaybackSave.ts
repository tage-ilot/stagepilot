import { useCallback, useEffect, useRef, useState } from "react";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import type { PlaybackDraft } from "./PlaybackSaveFooter";
import type { SaveBarState } from "./ui/SaveBar";

/** The MIDI step of the one Save configuration. The fields live in MidiSetupPanel; the sequence lives here. */
export interface PlaybackMidiStep {
  /** The MIDI settings must be written by this save (MIDI view, fields edited or method just chosen). */
  pending: boolean;
  /** Number of MIDI fields that differ from the saved values (0 unless the MIDI view is shown). */
  count: number;
  valid: boolean;
  /** Identifies the exact values to write, so a retry does not repeat a step that already succeeded. */
  snapshot: string;
  /** Resolves false when the save failed. */
  apply: () => Promise<boolean>;
  revert: () => void;
}

export const MIDI_INVALID_REASON = "The MIDI settings are not valid, so nothing was saved. Channel 1 to 16, note 0 to 127, debounce 0 to 2000 ms, and six different velocities from 100 to 127.";

export interface PlaybackSave {
  state: SaveBarState;
  count: number;
  dirty: boolean;
  reason: string | null;
  save: () => Promise<boolean>;
  discard: () => void;
}

const timeText = () => new Date().toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

// One Save configuration for the whole Playback panel. Order: method, song arrangement, MIDI settings.
// The first failure stops the save; steps that already succeeded are not repeated on retry.
export function usePlaybackSave(draft: PlaybackDraft, playback: PlaybackController | undefined, midi: PlaybackMidiStep): PlaybackSave {
  const [running, setRunning] = useState(false);
  const [failure, setFailure] = useState<{ key: string; step: "method" | "arrangement" | "midi"; reason?: string } | null>(null);
  const [autoAt, setAutoAt] = useState<string | null>(null);
  const inFlight = useRef(false);
  const done = useRef<{ method?: string; arrangement?: string; midi?: string }>({});
  const knownHost = useRef<{ host: string | null } | null>(null);

  const count = (draft.methodDirty ? 1 : 0) + draft.arrangementCount + midi.count;
  const arrangementKey = JSON.stringify([[...draft.skippedPlanSongs].sort(), [...draft.skippedPlaybackSongs].sort((a, b) => a - b)]);
  const key = `${draft.method}|${arrangementKey}|${midi.snapshot}`;

  // Scan and Connect save the computer by themselves; the bar then says so, with the time.
  const host = playback?.status?.settings.host;
  const hasStatus = Boolean(playback?.status);
  useEffect(() => {
    if (!hasStatus) return;
    const next = host ?? null;
    if (knownHost.current && knownHost.current.host !== next) setAutoAt(timeText());
    knownHost.current = { host: next };
  }, [hasStatus, host]);

  const save = useCallback(async () => {
    // The ref closes the gap before React re-renders: a double click or Enter repeat finds it taken.
    if (inFlight.current) return false;
    if (count === 0) return true;
    setFailure(null);
    if (midi.pending && !midi.valid) {
      setFailure({ key, step: "midi", reason: MIDI_INVALID_REASON });
      return false;
    }
    inFlight.current = true;
    setRunning(true);
    try {
      if (draft.methodDirty && done.current.method !== draft.method) {
        if (!(await draft.applyMethod())) { setFailure({ key, step: "method" }); return false; }
        done.current.method = draft.method;
      }
      if (draft.arrangementDirty && done.current.arrangement !== arrangementKey) {
        if (!(await draft.applyArrangement())) { setFailure({ key, step: "arrangement" }); return false; }
        done.current.arrangement = arrangementKey;
      }
      if (midi.pending && done.current.midi !== midi.snapshot) {
        if (!(await midi.apply())) { setFailure({ key, step: "midi" }); return false; }
        done.current.midi = midi.snapshot;
      }
      done.current = {};
      setAutoAt(null);
      return true;
    } finally {
      inFlight.current = false;
      setRunning(false);
    }
  }, [arrangementKey, count, draft, key, midi]);

  const discard = useCallback(() => {
    draft.revert();
    midi.revert();
    done.current = {};
    setFailure(null);
  }, [draft, midi]);

  const failed = failure !== null && failure.key === key ? failure : null;
  const dirty = count > 0;
  const reason = failed
    ? failed.reason ?? (failed.step === "midi" ? null : draft.error)
    : null;
  const state: SaveBarState = running || draft.saving
    ? { kind: "saving" }
    : failed
      ? { kind: "error", message: "Couldn't save" }
      : dirty
        ? { kind: "held", count }
        : autoAt
          ? { kind: "auto", at: autoAt }
          : { kind: "clean" };
  return { state, count, dirty, reason: reason ?? (dirty && !failed ? playback?.error ?? null : null), save, discard };
}

