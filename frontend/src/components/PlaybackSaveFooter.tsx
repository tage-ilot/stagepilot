import { useCallback, useEffect, useState } from "react";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import { SaveBar } from "./ui/SaveBar";
import type { PlaybackSave } from "./usePlaybackSave";

export type PlaybackMethod = "playback_api" | "real" | "simulated";

export interface PlaybackDraft {
  /** Connection method shown in the panel. A view switch held in the draft; only a save applies it. */
  method: PlaybackMethod;
  methodDirty: boolean;
  setMethod: (value: PlaybackMethod) => void;
  /** Writes the chosen method (selectSource) when it differs from the saved one. True when nothing is left to apply. */
  applyMethod: () => Promise<boolean>;
  enabled: boolean;
  setEnabled: (value: boolean) => void;
  /** Playback settings (not the method) differ from the saved ones. */
  settingsDirty: boolean;
  /** Number of Playback settings that differ from the saved values. */
  settingsCount: number;
  /** Writes the Playback settings (updatePlaybackSettings). The payload is the same as the old Save settings. */
  applySettings: () => Promise<boolean>;
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
  const [enabled, setEnabledState] = useState(savedEnabled);
  const [method, setMethodState] = useState<PlaybackMethod>(savedMethod);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => setMethodState(savedMethod), [savedMethod]);
  useEffect(() => setEnabledState(savedEnabled), [savedEnabled]);

  const methodDirty = method !== savedMethod;
  const settingsCount = Number(enabled !== savedEnabled);
  const settingsDirty = settingsCount > 0;
  const dirty = settingsDirty || methodDirty;
  const setMethod = useCallback((value: PlaybackMethod) => { setError(null); setMethodState(value); }, []);
  const setEnabled = useCallback((value: boolean) => { setError(null); setEnabledState(value); }, []);

  const applyMethod = useCallback(async () => {
    if (!methodDirty || method === "simulated") return true;
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

  const applySettings = useCallback(async () => {
    if (!status || !playback) return false;
    setError(null);
    setSaving(true);
    try {
      const ok = await playback.save({ ...status.settings, enabled, auto_scan: true, fast_transport: true });
      if (!ok) setError("Your configuration was not saved. Your changes are still here; try again.");
      return ok;
    } finally {
      setSaving(false);
    }
  }, [enabled, playback, status]);

  const revert = useCallback(() => {
    setMethodState(savedMethod);
    setEnabledState(savedEnabled);
    setError(null);
  }, [savedEnabled, savedMethod]);

  return { method, methodDirty, setMethod, applyMethod, enabled, setEnabled, settingsDirty, settingsCount, applySettings, dirty, saving, error, revert };
}

export function PlaybackSaveBar({ save }: { save: PlaybackSave }) {
  return (
    <>
      {save.reason && <p role="alert" className="mt-5 text-sm text-danger-text">{save.reason}</p>}
      <SaveBar className="mt-5" state={save.state} onSave={() => void save.save()} onRetry={() => void save.save()} onDiscard={save.discard} />
    </>
  );
}
