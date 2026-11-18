import { useCallback, useEffect, useRef, useState } from "react";

import { choosePlaybackLibrarySetlist, getPlaybackLibraryPlan, getPlaybackLibraryStatus, getSettings, updatePlaybackLibrarySettings } from "../api";
import { describeError } from "../errorText";
import type { LibraryPlan, LibraryStatus, SettingsResponse } from "../types";

/** How often an open window asks again. The backend asks the helper at most this often however many windows there are. */
export const LIBRARY_REFRESH_MS = 30_000;
/** While a setup box waits for the Library Link to answer, the window looks this often (the backend still asks the helper only every few seconds). */
export const LIBRARY_WATCH_MS = 3_000;

export interface PlaybackLibraryController {
  /** The computer the helper is asked at: the saved override, else the saved Playback host. Empty until a Playback computer is chosen. */
  target: string;
  /** The saved override (Advanced); empty means the Playback host. */
  address: string;
  status: LibraryStatus | null;
  plan: LibraryPlan | null;
  error: string | null;
  pending: "save" | "check" | "setlist" | null;
  /** Saves the override of the helper's computer (empty: the Playback host). */
  saveAddress: (address: string) => Promise<boolean>;
  /** Chooses which Playback setlist the loaded plan is matched with (null: the one on the plan's date). The backend keeps it in memory for this plan only; nothing is saved or sent to Playback. */
  chooseSetlist: (setlistId: number | null) => Promise<boolean>;
  /** The Check button: reads the helper's health now. */
  check: () => void;
  /** A setup box is waiting for the script: look every few seconds instead of every 30. */
  watch: (on: boolean) => void;
}

/**
 * The Playback library (the Library Link). There is no on/off switch: while a Playback computer is saved (or Advanced names another
 * address) this hook asks for the status and the plan every 30 s, only while the window is visible. A helper that is not there is a quiet
 * state, not an error. Read-only: the only thing it can write is its own address override.
 */
export function usePlaybackLibrary(canConfigure: boolean, settings: SettingsResponse | null, onSettings: (settings: SettingsResponse) => void): PlaybackLibraryController {
  const address = settings?.settings.playback_library?.address ?? "";
  const playbackHost = settings?.settings.playback_api?.host?.trim() ?? "";
  const target = address || playbackHost;
  const enabled = canConfigure && Boolean(target);
  const [status, setStatus] = useState<LibraryStatus | null>(null);
  const [plan, setPlan] = useState<LibraryPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<"save" | "check" | "setlist" | null>(null);
  const [watching, setWatching] = useState(false);
  const generation = useRef(0);
  // A background read clears the error a read left, never the one an action (save) left: that one stays until the next action.
  const errorFrom = useRef<"read" | "action" | null>(null);

  const read = useCallback(async (check: boolean, current: number) => {
    try {
      const [nextStatus, nextPlan] = await Promise.all([getPlaybackLibraryStatus(check), getPlaybackLibraryPlan(check)]);
      if (generation.current !== current) return;
      setStatus(nextStatus);
      setPlan(nextPlan);
      if (errorFrom.current !== "action") { errorFrom.current = null; setError(null); }
    } catch (cause) {
      if (generation.current === current && errorFrom.current !== "action") { errorFrom.current = "read"; setError(describeError(cause, "The Library Link could not be read.")); }
    }
  }, []);

  // Another computer, or none: what was held belongs to the old one.
  useEffect(() => {
    setStatus(null);
    setPlan(null);
    errorFrom.current = null;
    setError(null);
  }, [enabled, target]);

  useEffect(() => {
    const current = ++generation.current;
    if (!enabled) return;
    const gap = watching ? LIBRARY_WATCH_MS : LIBRARY_REFRESH_MS;
    let timer: number | undefined;
    const tick = async () => {
      if (typeof document === "undefined" || document.visibilityState !== "hidden") await read(false, current);
      if (generation.current === current) timer = window.setTimeout(() => void tick(), gap);
    };
    void tick();
    return () => {
      generation.current = current + 1;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [enabled, target, watching, read]);

  const saveAddress = useCallback(async (nextAddress: string): Promise<boolean> => {
    if (!canConfigure) return false;
    setPending("save");
    errorFrom.current = null;
    setError(null);
    try {
      const answer = await updatePlaybackLibrarySettings(nextAddress);
      setStatus(answer);
      onSettings(await getSettings());
      return true;
    } catch (cause) {
      errorFrom.current = "action";
      setError(describeError(cause, "The Library Link's address could not be saved."));
      return false;
    } finally {
      setPending(null);
    }
  }, [canConfigure, onSettings]);

  const chooseSetlist = useCallback(async (setlistId: number | null): Promise<boolean> => {
    if (!canConfigure) return false;
    setPending("setlist");
    errorFrom.current = null;
    setError(null);
    try {
      setPlan(await choosePlaybackLibrarySetlist(setlistId));
      return true;
    } catch (cause) {
      errorFrom.current = "action";
      setError(describeError(cause, "StagePilot could not switch the Playback setlist."));
      return false;
    } finally {
      setPending(null);
    }
  }, [canConfigure]);

  const check = useCallback(() => {
    if (!enabled) return;
    setPending("check");
    void read(true, generation.current).finally(() => setPending(null));
  }, [enabled, read]);

  const watch = useCallback((on: boolean) => setWatching(on), []);

  return { target, address, status, plan, error, pending, saveAddress, chooseSetlist, check, watch };
}
