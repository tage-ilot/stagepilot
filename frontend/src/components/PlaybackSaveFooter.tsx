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
  autoScan: boolean;
  fastTransport: boolean;
  setEnabled: (value: boolean) => void;
  setAutoScan: (value: boolean) => void;
  setFastTransport: (value: boolean) => void;
  /** Songs skipped on each side, held until Save. Order is never touched; see playbackArrangement.ts. */
  skippedPlanSongs: ReadonlySet<string>;
  skippedPlaybackSongs: ReadonlySet<number>;
  setPlanSongSkipped: (id: string, skipped: boolean) => void;
  setPlaybackSongSkipped: (id: number, skipped: boolean) => void;
  /** Forget skipped plan songs that the loaded plan no longer has. */
  dropMissingPlanSkips: (planSongIds: readonly string[]) => void;
  arrangementDirty: boolean;
  arrangementCount: number;
  applyArrangement: () => Promise<boolean>;
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
  const savedAutoScan = status?.settings.auto_scan ?? true;
  const savedFastTransport = status?.settings.fast_transport ?? true;
  const [fastTransport, setFastTransportState] = useState(savedFastTransport);
  const [enabled, setEnabledState] = useState(savedEnabled);
  const [autoScan, setAutoScanState] = useState(savedAutoScan);
  const [method, setMethodState] = useState<PlaybackMethod>(savedMethod);
  const savedPlanSkips = status?.skipped_plan_song_ids;
  const savedPlaybackSkips = status?.skipped_song_ids;
  const [planSkips, setPlanSkips] = useState<ReadonlySet<string>>(() => new Set(savedPlanSkips ?? []));
  const [playbackSkips, setPlaybackSkips] = useState<ReadonlySet<number>>(() => new Set(savedPlaybackSkips ?? []));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => setMethodState(savedMethod), [savedMethod]);
  useEffect(() => setEnabledState(savedEnabled), [savedEnabled]);
  useEffect(() => setAutoScanState(savedAutoScan), [savedAutoScan]);
  useEffect(() => setFastTransportState(savedFastTransport), [savedFastTransport]);
  const savedPlanKey = JSON.stringify(savedPlanSkips ?? []);
  const savedPlaybackKey = JSON.stringify(savedPlaybackSkips ?? []);
  useEffect(() => setPlanSkips(new Set(JSON.parse(savedPlanKey) as string[])), [savedPlanKey]);
  useEffect(() => setPlaybackSkips(new Set(JSON.parse(savedPlaybackKey) as number[])), [savedPlaybackKey]);

  const methodDirty = method !== savedMethod;
  const settingsCount = Number(enabled !== savedEnabled) + Number(autoScan !== savedAutoScan) + Number(fastTransport !== savedFastTransport);
  const settingsDirty = settingsCount > 0;
  const differs = <T,>(held: ReadonlySet<T>, saved: readonly T[] | undefined) => (saved ?? []).filter((id) => !held.has(id)).length + [...held].filter((id) => !(saved ?? []).includes(id)).length;
  const arrangementCount = differs(planSkips, savedPlanSkips) + differs(playbackSkips, savedPlaybackSkips);
  const arrangementDirty = arrangementCount > 0;
  const dirty = settingsDirty || methodDirty || arrangementDirty;
  const setMethod = useCallback((value: PlaybackMethod) => { setError(null); setMethodState(value); }, []);
  const setEnabled = useCallback((value: boolean) => { setError(null); setEnabledState(value); }, []);
  const setAutoScan = useCallback((value: boolean) => { setError(null); setAutoScanState(value); }, []);

  const setFastTransport = useCallback((value: boolean) => { setError(null); setFastTransportState(value); }, []);

  const setPlanSongSkipped = useCallback((id: string, skipped: boolean) => {
    setError(null);
    setPlanSkips((held) => { const next = new Set(held); if (skipped) next.add(id); else next.delete(id); return next; });
  }, []);
  const setPlaybackSongSkipped = useCallback((id: number, skipped: boolean) => {
    setError(null);
    setPlaybackSkips((held) => { const next = new Set(held); if (skipped) next.add(id); else next.delete(id); return next; });
  }, []);
  const dropMissingPlanSkips = useCallback((planSongIds: readonly string[]) => {
    setError(null);
    setPlanSkips((held) => new Set([...held].filter((id) => planSongIds.includes(id))));
  }, []);

  const applyArrangement = useCallback(async () => {
    if (!playback) return false;
    setError(null);
    setSaving(true);
    try {
      const ok = await playback.saveArrangement([...playbackSkips], [...planSkips]);
      if (!ok) setError("Your song arrangement was not saved. Your changes are still here; try again.");
      return ok;
    } finally {
      setSaving(false);
    }
  }, [planSkips, playback, playbackSkips]);

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
      const ok = await playback.save({ ...status.settings, enabled, auto_scan: autoScan, fast_transport: fastTransport });
      if (!ok) setError("Your configuration was not saved. Your changes are still here; try again.");
      return ok;
    } finally {
      setSaving(false);
    }
  }, [autoScan, enabled, fastTransport, playback, status]);

  const revert = useCallback(() => {
    setMethodState(savedMethod);
    setEnabledState(savedEnabled);
    setAutoScanState(savedAutoScan);
    setFastTransportState(savedFastTransport);
    setPlanSkips(new Set(JSON.parse(savedPlanKey) as string[]));
    setPlaybackSkips(new Set(JSON.parse(savedPlaybackKey) as number[]));
    setError(null);
  }, [savedAutoScan, savedEnabled, savedFastTransport, savedMethod, savedPlanKey, savedPlaybackKey]);

  return { method, methodDirty, setMethod, applyMethod, skippedPlanSongs: planSkips, skippedPlaybackSongs: playbackSkips, setPlanSongSkipped, setPlaybackSongSkipped, dropMissingPlanSkips, arrangementDirty, arrangementCount, applyArrangement, enabled, autoScan, fastTransport, setEnabled, setAutoScan, setFastTransport, settingsDirty, settingsCount, applySettings, dirty, saving, error, revert };
}

export function PlaybackSaveBar({ save }: { save: PlaybackSave }) {
  return (
    <>
      {save.reason && <p role="alert" className="mt-5 text-sm text-danger-text">{save.reason}</p>}
      <SaveBar className="mt-5" state={save.state} onSave={() => void save.save()} onRetry={() => void save.save()} onDiscard={save.discard} />
    </>
  );
}
