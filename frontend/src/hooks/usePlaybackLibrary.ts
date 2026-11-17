import { useCallback, useEffect, useRef, useState } from "react";

import { getPlaybackLibraryPlan, getPlaybackLibraryStatus, getSettings, updatePlaybackLibrarySettings } from "../api";
import { describeError } from "../errorText";
import type { LibraryPlan, LibraryStatus, SettingsResponse } from "../types";

/** How often an open window asks again. The backend asks the helper at most this often however many windows there are. */
export const LIBRARY_REFRESH_MS = 30_000;

export interface PlaybackLibraryController {
  /** The saved setting. */
  enabled: boolean;
  address: string;
  status: LibraryStatus | null;
  plan: LibraryPlan | null;
  error: string | null;
  pending: "save" | "check" | null;
  save: (enabled: boolean, address: string) => Promise<boolean>;
  /** The Check button: reads the helper's health now. */
  check: () => void;
}

/**
 * The optional Playback library. While the setting is off NOTHING is asked (not even the status): this hook only wakes up when the
 * saved settings say it is on. While on, it asks for the status and the plan every 30 s, only while the window is visible.
 * Read-only: the one thing it can write is its own setting (`save`).
 */
export function usePlaybackLibrary(canConfigure: boolean, settings: SettingsResponse | null, onSettings: (settings: SettingsResponse) => void): PlaybackLibraryController {
  const saved = settings?.settings.playback_library;
  const enabled = canConfigure && Boolean(saved?.enabled);
  const address = saved?.address ?? "";
  const [status, setStatus] = useState<LibraryStatus | null>(null);
  const [plan, setPlan] = useState<LibraryPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<"save" | "check" | null>(null);
  const generation = useRef(0);

  const read = useCallback(async (check: boolean, current: number) => {
    try {
      const [nextStatus, nextPlan] = await Promise.all([getPlaybackLibraryStatus(check), getPlaybackLibraryPlan(check)]);
      if (generation.current !== current) return;
      setStatus(nextStatus);
      setPlan(nextPlan);
      setError(null);
    } catch (cause) {
      if (generation.current === current) setError(describeError(cause, "The Playback library could not be read."));
    }
  }, []);

  useEffect(() => {
    const current = ++generation.current;
    setStatus(null);
    setPlan(null);
    setError(null);
    if (!enabled) return;
    let timer: number | undefined;
    const tick = async () => {
      if (typeof document === "undefined" || document.visibilityState !== "hidden") await read(false, current);
      if (generation.current === current) timer = window.setTimeout(() => void tick(), LIBRARY_REFRESH_MS);
    };
    void tick();
    return () => {
      generation.current = current + 1;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [enabled, address, read]);

  const save = useCallback(async (nextEnabled: boolean, nextAddress: string): Promise<boolean> => {
    if (!canConfigure) return false;
    setPending("save");
    setError(null);
    try {
      const answer = await updatePlaybackLibrarySettings(nextEnabled, nextAddress);
      setStatus(answer);
      onSettings(await getSettings());
      return true;
    } catch (cause) {
      setError(describeError(cause, "The Playback library setting could not be saved."));
      return false;
    } finally {
      setPending(null);
    }
  }, [canConfigure, onSettings]);

  const check = useCallback(() => {
    if (!enabled) return;
    setPending("check");
    void read(true, generation.current).finally(() => setPending(null));
  }, [enabled, read]);

  return { enabled, address, status, plan, error, pending, save, check };
}
