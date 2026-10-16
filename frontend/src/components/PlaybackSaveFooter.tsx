import { useCallback, useEffect, useRef, useState } from "react";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import { SaveBar } from "./ui/SaveBar";
import type { PlaybackSave } from "./usePlaybackSave";

export type PlaybackMethod = "playback_api" | "real";

export interface PlaybackDraft {
  /** Connection method shown in the panel. A view switch held in the draft; only a save applies it. */
  method: PlaybackMethod;
  methodDirty: boolean;
  setMethod: (value: PlaybackMethod) => void;
  /** Writes the chosen method (selectSource) when it differs from the saved one. True when nothing is left to apply. */
  applyMethod: () => Promise<boolean>;
  dirty: boolean;
  saving: boolean;
  error: string | null;
  revert: () => void;
}

// One draft of every editable setting in the visible Playback panel. Scan Network and
// Connect are instant actions that save themselves; they never read or wait on this draft.
export function usePlaybackDraft(playback?: PlaybackController, savedMethod: PlaybackMethod = "playback_api"): PlaybackDraft {
  const status = playback?.status ?? null;
  const savedEnabled = status?.settings.enabled ?? true;
  const [method, setMethodState] = useState<PlaybackMethod>(savedMethod);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => setMethodState(savedMethod), [savedMethod]);
  useEffect(() => setMethodState(savedMethod), [savedMethod]);

  // Connecting automatically is always on (owner, 2026-10-16): there is no switch for it. An install saved with it off while Playback
  // API is the saved method is turned back on once, quietly, with the settings it already has (Scan Network and Connect do the same).
  const healed = useRef(false);
  useEffect(() => {
    if (healed.current || !playback || !status || savedMethod !== "playback_api" || savedEnabled) return;
    healed.current = true;
    void playback.save({ ...status.settings, enabled: true, auto_scan: true, fast_transport: true });
  }, [playback, savedEnabled, savedMethod, status]);

  const methodDirty = method !== savedMethod;
  const dirty = methodDirty;
  const setMethod = useCallback((value: PlaybackMethod) => { setError(null); setMethodState(value); }, []);

  const applyMethod = useCallback(async () => {
    if (!methodDirty) return true;
    if (!playback) return false;
    setError(null);
    setSaving(true);
    try {
      const ok = await playback.selectSource(method);
      if (!ok) setError("Your connection method was not saved. Your changes are still here; try again.");
      return ok;
    } finally {
      setSaving(false);
    }
  }, [method, methodDirty, playback]);

  const revert = useCallback(() => {
    setMethodState(savedMethod);
    setError(null);
  }, [savedMethod]);

  return { method, methodDirty, setMethod, applyMethod, dirty, saving, error, revert };
}

export function PlaybackSaveBar({ save }: { save: PlaybackSave }) {
  return (
    <>
      {save.reason && <p role="alert" className="mt-5 text-sm text-danger-text">{save.reason}</p>}
      <SaveBar className="mt-5" state={save.state} onSave={() => void save.save()} onRetry={() => void save.save()} onDiscard={save.discard} />
    </>
  );
}
