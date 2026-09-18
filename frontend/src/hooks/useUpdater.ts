import { useCallback, useEffect, useRef, useState } from "react";

import {
  tauriUpdaterAdapter,
  type UpdateCandidate,
  type UpdateProgress,
  type UpdaterAdapter,
} from "../services/updater";

export type UpdaterStatus =
  | "idle"
  | "checking"
  | "available"
  | "confirmation"
  | "downloading"
  | "installing"
  | "restarting"
  | "current"
  | "error";

export type UpdaterState = {
  status: UpdaterStatus;
  currentVersion: string | null;
  availableVersion: string | null;
  releaseNotes: string | null;
  releaseDate: string | null;
  progress: UpdateProgress | null;
  error: string | null;
  errorDialogOpen: boolean;
  successMessage: string | null;
};

export type UseUpdaterOptions = {
  adapter?: UpdaterAdapter;
  ready: boolean;
  betaEnabled?: boolean;
  startupDelayMs?: number;
  checkIntervalMs?: number;
};

const initialState: UpdaterState = {
  status: "idle",
  currentVersion: null,
  availableVersion: null,
  releaseNotes: null,
  releaseDate: null,
  progress: null,
  error: null,
  errorDialogOpen: false,
  successMessage: null,
};

export function useUpdater({
  adapter = tauriUpdaterAdapter,
  ready,
  betaEnabled = false,
  startupDelayMs = 5_000,
  checkIntervalMs = 6 * 60 * 60 * 1_000,
}: UseUpdaterOptions) {
  const [state, setState] = useState(initialState);
  const candidate = useRef<UpdateCandidate | null>(null);
  const checking = useRef<Promise<void> | null>(null);
  const installing = useRef(false);
  const lastCheckedAt = useRef(0);
  const betaEnabledRef = useRef(betaEnabled);
  betaEnabledRef.current = betaEnabled;

  const checkForUpdate = useCallback(async () => {
    if (!ready || !adapter.isEnabled() || checking.current || installing.current) return;
    const operation = (async () => {
      setState((current) => ({ ...current, status: "checking", error: null }));
      try {
        const update = await adapter.check({ betaEnabled: betaEnabledRef.current });
        candidate.current = update;
        lastCheckedAt.current = Date.now();
        setState((current) => update
          ? {
              ...current,
              status: "available",
              currentVersion: update.currentVersion,
              availableVersion: update.availableVersion,
              releaseNotes: update.releaseNotes,
              releaseDate: update.releaseDate,
              error: null,
              errorDialogOpen: false,
            }
          : { ...initialState, status: "current", successMessage: current.successMessage });
      } catch (cause) {
        candidate.current = null;
        lastCheckedAt.current = Date.now();
        console.warn("StagePilot update check failed.", cause);
        setState((current) => ({
          ...current,
          status: "error",
          error: cause instanceof Error ? cause.message : "Update check failed.",
          errorDialogOpen: false,
        }));
      }
    })().finally(() => {
      checking.current = null;
    });
    checking.current = operation;
    await operation;
  }, [adapter, ready]);

  useEffect(() => {
    if (!ready || !adapter.isEnabled()) return;
    const startup = window.setTimeout(() => void checkForUpdate(), startupDelayMs);
    const interval = window.setInterval(() => void checkForUpdate(), checkIntervalMs);
    const onFocus = () => {
      if (Date.now() - lastCheckedAt.current >= checkIntervalMs) void checkForUpdate();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      window.clearTimeout(startup);
      window.clearInterval(interval);
      window.removeEventListener("focus", onFocus);
    };
  }, [adapter, checkForUpdate, checkIntervalMs, ready, startupDelayMs]);

  useEffect(() => {
    if (!ready || !adapter.isEnabled()) return;
    void adapter.restoreAfterRelaunch()
      .then((result) => {
        if (result) {
          setState((current) => ({
            ...current,
            successMessage: `StagePilot updated to ${result.updatedVersion}.`,
          }));
        }
      })
      .catch((cause) => console.warn("StagePilot could not restore update state.", cause));
  }, [adapter, ready]);

  const openConfirmation = useCallback(() => {
    if (!candidate.current || state.status !== "available") return;
    setState((current) => ({ ...current, status: "confirmation" }));
  }, [state.status]);

  const cancelConfirmation = useCallback(() => {
    if (installing.current) return;
    setState((current) => ({ ...current, status: "available" }));
  }, []);

  const install = useCallback(async () => {
    const update = candidate.current;
    if (!update || installing.current) return;
    installing.current = true;
    setState((current) => ({ ...current, status: "downloading", error: null }));
    try {
      await adapter.prepareRelaunch(update.availableVersion);
      await update.install((progress) => {
        setState((current) => ({
          ...current,
          progress,
          status: progress.stage === "installing" ? "installing" : "downloading",
        }));
      });
      setState((current) => ({ ...current, status: "restarting" }));
      await adapter.relaunch();
    } catch (cause) {
      adapter.clearRelaunchMarker();
      installing.current = false;
      setState((current) => ({
        ...current,
        status: "error",
        error: cause instanceof Error ? cause.message : "StagePilot could not install the update.",
        errorDialogOpen: true,
      }));
    }
  }, [adapter]);

  const closeError = useCallback(() => {
    installing.current = false;
    setState((current) => ({
      ...current,
      status: candidate.current ? "available" : "idle",
      error: null,
      errorDialogOpen: false,
      progress: null,
    }));
  }, []);

  return {
    ...state,
    checkForUpdate,
    openConfirmation,
    cancelConfirmation,
    install,
    retry: install,
    closeError,
  };
}

export type UpdaterController = ReturnType<typeof useUpdater>;
