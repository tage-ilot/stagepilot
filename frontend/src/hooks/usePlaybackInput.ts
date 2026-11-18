import { useCallback, useEffect, useRef, useState } from "react";

import { cancelPlaybackScan, discoverPlaybackSongOrder, refreshPlaybackSongOrderFromLibrary, updatePlaybackArrangement, findPlayback, getPlaybackEvents, getPlaybackStatus, getSettings, updatePlaybackSettings, updateSettings } from "../api";
import { describeError } from "../errorText";
import type { PlaybackMonitorEntry, PlaybackSettingsInput, PlaybackStatusResponse, SettingsResponse } from "../types";

export type PlaybackOperation = "save" | "source" | "scan" | "discover" | "library" | null;

/** A fast answer from the Library Link is still seen: the button's loading animation lasts at least this long. */
export const LIBRARY_READ_MIN_MS = 700;
export interface PlaybackController {
  status: PlaybackStatusResponse | null;
  events: PlaybackMonitorEntry[];
  error: string | null;
  message: string | null;
  pending: PlaybackOperation;
  save: (settings: PlaybackSettingsInput) => Promise<boolean>;
  saveArrangement: (skippedSongIds: number[], skippedPlanSongIds: string[]) => Promise<boolean>;
  selectSource: (source: "playback_api" | "real") => Promise<boolean>;
  scan: (host?: string) => void;
  cancelScan: () => void;
  discover: () => void;
  /** "Scan Playback Songs" while the Library Link works: reads the Library Link fresh, steps through nothing. */
  refreshFromLibrary?: () => void;
  /** How many songs the last press read from the Library Link, once the answer is in and confirmed; null otherwise. */
  libraryRead?: number | null;
  acceptStatus?: (status: PlaybackStatusResponse) => void;
}

export function usePlaybackInput(canConfigure: boolean, onSettings: (settings: SettingsResponse) => void): PlaybackController {
  const [status, setStatus] = useState<PlaybackStatusResponse | null>(null);
  const [events, setEvents] = useState<PlaybackMonitorEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [pending, setPending] = useState<PlaybackOperation>(null);
  const [libraryRead, setLibraryRead] = useState<number | null>(null);
  const generation = useRef(0);
  const statusRevision = useRef(0);
  const operation = useRef<PlaybackOperation>(null);
  const scanStarted = useRef<number | null>(null);
  // Poll continuously for failover/failback even when the overall connection stays green.
  // A serialized poll and generation checks prevent overlapping or obsolete responses.
  useEffect(() => {
    const current = ++generation.current;
    let timer: number | undefined;
    operation.current = null;
    scanStarted.current = null;
    setPending(null);
    setStatus(null);
    setEvents([]);
    setError(null);
    setStatusError(null);
    setMessage(null);
    setLibraryRead(null);
    if (!canConfigure) return;
    const poll = async () => {
      const revision = statusRevision.current;
      try {
        const next = await getPlaybackStatus();
        if (generation.current !== current || statusRevision.current !== revision) return;
        setStatus(next);
        setStatusError(null);
        if (scanStarted.current !== null) {
          // The backend owns scan progress; a time bound only guards a lost backend.
          if (next.scan.state !== "scanning" || Date.now() - scanStarted.current >= 120_000) {
            scanStarted.current = null;
            operation.current = null;
            setPending(null);
          }
        }
        try {
          const monitor = await getPlaybackEvents();
          if (generation.current === current) setEvents(monitor.events);
        } catch {
          // A monitor error must not hide verified connection status.
        }
      } catch (cause) {
        if (generation.current === current && statusRevision.current === revision) {
          setStatus(null);
          setStatusError(describeError(cause, "Playback status unavailable."));
          if (operation.current === "scan" && scanStarted.current !== null) {
            operation.current = null;
            scanStarted.current = null;
            setPending(null);
          }
        }
      } finally {
        if (generation.current === current) timer = window.setTimeout(poll, 750);
      }
    };
    void poll();
    return () => {
      generation.current = current + 1;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [canConfigure]);

  const run = useCallback(async (kind: Exclude<PlaybackOperation, null>, action: () => Promise<void>): Promise<boolean> => {
    if (!canConfigure || operation.current !== null) return false;
    const current = generation.current;
    operation.current = kind;
    ++statusRevision.current;
    setPending(kind);
    setError(null);
    setMessage(null);
    try {
      await action();
      return generation.current === current;
    } catch (cause) {
      if (generation.current === current) {
        scanStarted.current = null;
        setError(describeError(cause, "Playback operation failed."));
      }
      return false;
    } finally {
      if (generation.current === current && scanStarted.current === null) {
        operation.current = null;
        setPending(null);
      }
    }
  }, [canConfigure]);

  const save = useCallback((input: PlaybackSettingsInput): Promise<boolean> => {
    const current = generation.current;
    return run("save", async () => {
      const next = await updatePlaybackSettings(input);
      if (generation.current !== current) return;
      ++statusRevision.current;
      setStatus(next);
      const saved = await getSettings();
      if (generation.current !== current) return;
      onSettings(saved);
      setMessage("Configuration saved.");
    });
  }, [onSettings, run]);

  const saveArrangement = useCallback((skippedSongIds: number[], skippedPlanSongIds: string[]): Promise<boolean> => {
    const current = generation.current;
    return run("save", async () => {
      const next = await updatePlaybackArrangement(skippedSongIds, skippedPlanSongIds);
      if (generation.current !== current) return;
      ++statusRevision.current;
      setStatus(next);
      setMessage("Configuration saved.");
    });
  }, [run]);

  const selectSource = useCallback((source: "playback_api" | "real"): Promise<boolean> => {
    const current = generation.current;
    return run("source", async () => {
      // Read fresh settings so source changes preserve discovered order and MIDI values.
      const saved = await getSettings();
      if (generation.current !== current) return;
      const next = await updateSettings({ ...saved.settings,
        integration_modes: { ...saved.settings.integration_modes, midi_source: source },
      });
      if (generation.current !== current) return;
      onSettings(next);
      const connection = await getPlaybackStatus();
      if (generation.current !== current) return;
      ++statusRevision.current;
      setStatus(connection);
      setMessage("Connection method saved.");
    });
  }, [onSettings, run]);

  const scan = useCallback((host?: string) => {
    const current = generation.current;
    void run("scan", async () => {
      const next = await findPlayback(host);
      if (generation.current !== current) return;
      ++statusRevision.current;
      setStatus(next);
      scanStarted.current = Date.now();
    });
  }, [run]);

  const cancelScan = useCallback(() => {
    const current = generation.current;
    void (async () => {
      try {
        const next = await cancelPlaybackScan();
        if (generation.current !== current) return;
        ++statusRevision.current;
        setStatus(next);
      } catch (cause) {
        if (generation.current === current) setError(describeError(cause, "Could not cancel the scan."));
      } finally {
        if (generation.current === current) {
          scanStarted.current = null;
          operation.current = null;
          setPending(null);
        }
      }
    })();
  }, []);

  const discover = useCallback(() => {
    const current = generation.current;
    void run("discover", async () => {
      const next = await discoverPlaybackSongOrder();
      if (generation.current !== current) return;
      ++statusRevision.current;
      setStatus(next);
      if (next.discovery === "failed") throw new Error(next.last_error ?? "Song order discovery failed.");
      const saved = await getSettings();
      if (generation.current !== current) return;
      onSettings(saved);
      setMessage("Song order saved.");
    });
  }, [onSettings, run]);

  const refreshFromLibrary = useCallback(() => {
    const current = generation.current;
    void run("library", async () => {
      setLibraryRead(null);
      const startedAt = Date.now();
      const next = await refreshPlaybackSongOrderFromLibrary();
      if (generation.current !== current) return;
      ++statusRevision.current;
      setStatus(next);
      const read = next.order_source === "library" && !next.order_library_note;
      if (read) {
        // The confirmation is shown after the animation has been seen, never before.
        const left = LIBRARY_READ_MIN_MS - (Date.now() - startedAt);
        if (left > 0) await new Promise((resolve) => window.setTimeout(resolve, left));
        if (generation.current !== current) return;
      }
      const saved = await getSettings();
      if (generation.current !== current) return;
      onSettings(saved);
      if (read) setLibraryRead(next.song_order.length);
    });
  }, [onSettings, run]);

  const acceptStatus = useCallback((next: PlaybackStatusResponse) => { ++statusRevision.current; setStatus(next); void getSettings().then(onSettings).catch(() => undefined); }, [onSettings]);

  return { acceptStatus, status, events, error: error ?? statusError, message, pending, save, saveArrangement, selectSource, scan, cancelScan, discover, refreshFromLibrary, libraryRead };
}
