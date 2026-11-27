import { useCallback, useEffect, useRef, useState } from "react";

import { getPresenterFollowStatus, getSettings, presenterFollowTryAgain, updateSettings } from "../api";
import { describeError } from "../errorText";
import { OFFSET_MAX_MS, OFFSET_MIN_MS, type FollowStatus } from "../presenterFollow";
import type { SettingsResponse } from "../types";

/** While the switch is on and the Presentation panel is open the window looks this often; with the switch off it only checks now and then (nothing is happening). */
export const FOLLOW_REFRESH_MS = 2_000;
export const FOLLOW_IDLE_REFRESH_MS = 30_000;

export interface PresenterFollowController {
  status: FollowStatus | null;
  /** Why the status could not be read, or the last save could not be made. */
  error: string | null;
  /** The master switch as saved (`propresenter.presenter_follow.enabled`; off when it was never set). */
  enabled: boolean;
  /** The timing offset as saved, in ms (positive shows every slide earlier). */
  offsetMs: number;
  pending: "switch" | "offset" | "retry" | null;
  /** Turns following on or off. Resolves true when it was saved. */
  setEnabled: (on: boolean) => Promise<boolean>;
  /** Saves the timing offset (a whole number from -2000 to 2000). Resolves true when it was saved. */
  setOffset: (ms: number) => Promise<boolean>;
  /** After following was paused by failures: tries ProPresenter again. */
  tryAgain: () => void;
}

/**
 * Following Playback with the lyrics in ProPresenter. It reads `GET propresenter-follow/status` only while the window that shows it is on
 * screen (`wanted`) and the page is visible, and saves the switch and the offset through the ordinary settings route, on top of the settings
 * as they are now (so nothing else the person set is touched). The answer never feeds the header's verdict.
 */
export function usePresenterFollow(
  canConfigure: boolean,
  wanted: boolean,
  settings: SettingsResponse | null,
  onSettings: (settings: SettingsResponse) => void,
): PresenterFollowController {
  const saved = settings?.settings.propresenter.presenter_follow;
  const [status, setStatus] = useState<FollowStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<PresenterFollowController["pending"]>(null);
  const generation = useRef(0);
  // Settings that are loaded and carry no `presenter_follow` mean off (the file leaves the key out while it holds the defaults).
  const enabled = settings ? (saved?.enabled ?? false) : (status?.enabled ?? false);
  const offsetMs = settings ? (saved?.offset_ms ?? 0) : (status?.offset_ms ?? 0);
  const active = canConfigure && wanted;
  // A save that is running. The reading stops looking while it runs (and once after), so an old answer never overwrites a newer one.
  const saving = useRef(false);

  const read = useCallback(async (current: number) => {
    try {
      const next = await getPresenterFollowStatus();
      if (generation.current !== current || saving.current) return;
      setStatus(next);
      setError(null);
    } catch (cause) {
      if (generation.current === current) setError(describeError(cause, "The state of following Playback could not be read."));
    }
  }, []);

  useEffect(() => {
    const current = ++generation.current;
    if (!active) return undefined;
    const gap = enabled ? FOLLOW_REFRESH_MS : FOLLOW_IDLE_REFRESH_MS;
    let timer: number | undefined;
    const tick = async () => {
      if (typeof document === "undefined" || document.visibilityState !== "hidden") await read(current);
      if (generation.current === current) timer = window.setTimeout(() => void tick(), gap);
    };
    void tick();
    return () => {
      generation.current = current + 1;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [active, enabled, read]);

  const save = useCallback(async (kind: "switch" | "offset", change: { enabled?: boolean; offset_ms?: number }, failure: string): Promise<boolean> => {
    if (!canConfigure || saving.current) return false;
    saving.current = true;
    setPending(kind);
    setError(null);
    try {
      // The settings as they are now, so a page opened a while ago never writes old values back.
      const now = await getSettings();
      const follow = now.settings.propresenter.presenter_follow ?? { enabled: false, offset_ms: 0 };
      const next = await updateSettings({
        ...now.settings,
        propresenter: { ...now.settings.propresenter, presenter_follow: { enabled: change.enabled ?? follow.enabled, offset_ms: change.offset_ms ?? follow.offset_ms } },
      });
      onSettings(next);
      saving.current = false;
      await read(generation.current);
      return true;
    } catch (cause) {
      setError(describeError(cause, failure));
      return false;
    } finally {
      saving.current = false;
      setPending(null);
    }
  }, [canConfigure, onSettings, read]);

  const setEnabled = useCallback((on: boolean) => save("switch", { enabled: on }, "The switch could not be saved."), [save]);
  const setOffset = useCallback((ms: number) => {
    if (!Number.isInteger(ms) || ms < OFFSET_MIN_MS || ms > OFFSET_MAX_MS) return Promise.resolve(false);
    return save("offset", { offset_ms: ms }, "The timing offset could not be saved.");
  }, [save]);

  const tryAgain = useCallback(() => {
    if (!canConfigure) return;
    setPending("retry");
    setError(null);
    void presenterFollowTryAgain()
      .then((next) => { setStatus(next); })
      .catch((cause) => setError(describeError(cause, "Following could not be tried again.")))
      .finally(() => setPending(null));
  }, [canConfigure]);

  return { status, error, enabled, offsetMs, pending, setEnabled, setOffset, tryAgain };
}
