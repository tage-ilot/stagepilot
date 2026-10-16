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
  /**
   * Non-null when in-app updates cannot work from the current install location
   * (macOS App Translocation). Carries the actionable fix instructions.
   */
  installBlockedReason: string | null;
};

export type UseUpdaterOptions = {
  adapter?: UpdaterAdapter;
  ready: boolean;
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
  installBlockedReason: null,
};

export function useUpdater({
  adapter = tauriUpdaterAdapter,
  ready,
  startupDelayMs = 5_000,
  checkIntervalMs = 6 * 60 * 60 * 1_000,
}: UseUpdaterOptions) {
  const [state, setState] = useState(initialState);
  const candidate = useRef<UpdateCandidate | null>(null);
  const checking = useRef<Promise<void> | null>(null);
  const installing = useRef(false);
  const lastCheckedAt = useRef(0);
  const installBlocked = useRef<string | null>(null);

  const checkForUpdate = useCallback(async () => {
    if (!ready || !adapter.isEnabled() || checking.current || installing.current) return;
    const operation = (async () => {
      setState((current) => ({ ...current, status: "checking", error: null }));
      // Probe the real install location first. On macOS a translocated bundle
      // can never be updated in place, so the user gets the actionable fix
      // instead of a full download that fails at install time.
      let blockedReason: string | null = null;
      try {
        const environment = (await adapter.installEnvironment?.()) ?? null;
        blockedReason = environment?.updatesBlocked
          ? environment.guidance ?? "StagePilot cannot install updates from its current location."
          : null;
      } catch (cause) {
        console.warn("StagePilot could not determine its install location.", cause);
      }
      installBlocked.current = blockedReason;
      try {
        const update = await adapter.check();
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
              installBlockedReason: blockedReason,
            }
          : {
              ...initialState,
              status: "current",
              successMessage: current.successMessage,
              installBlockedReason: blockedReason,
            });
      } catch (cause) {
        candidate.current = null;
        lastCheckedAt.current = Date.now();
        console.warn("StagePilot update check failed.", cause);
        setState((current) => ({
          ...current,
          status: "error",
          error: cause instanceof Error ? cause.message : "Update check failed.",
          errorDialogOpen: false,
          installBlockedReason: blockedReason,
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
    // Refuse to start an install that cannot land. Doing nothing but showing the
    // fix is strictly better than downloading, stopping the backend, and then
    // failing to write into a read-only translocated bundle.
    let blockedReason = installBlocked.current;
    try {
      const environment = (await adapter.installEnvironment?.()) ?? null;
      if (environment) {
        blockedReason = environment.updatesBlocked
          ? environment.guidance ?? "StagePilot cannot install updates from its current location."
          : null;
        installBlocked.current = blockedReason;
      }
    } catch (cause) {
      console.warn("StagePilot could not determine its install location.", cause);
    }
    if (blockedReason) {
      setState((current) => ({
        ...current,
        status: "error",
        error: blockedReason,
        errorDialogOpen: true,
        progress: null,
        installBlockedReason: blockedReason,
      }));
      return;
    }
    installing.current = true;
    // `prepare_for_update` (run inside the candidate's install, immediately
    // before the "installing" stage) deliberately kills the managed backend so
    // the installer can replace the binary on disk. Reaching that stage is the
    // signal that the backend is actually down and has to be brought back if
    // the install then fails.
    let backendStopped = false;
    setState((current) => ({ ...current, status: "downloading", error: null }));
    try {
      await adapter.prepareRelaunch(update.availableVersion);
      await update.install((progress) => {
        if (progress.stage === "installing") backendStopped = true;
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
      const reason = cause instanceof Error
        ? cause.message
        : "StagePilot could not install the update.";
      let recovery = "";
      if (backendStopped) {
        let restarted = false;
        try {
          restarted = (await adapter.restartBackend?.()) ?? false;
        } catch (restartCause) {
          console.warn("StagePilot could not restart the backend after a failed update.", restartCause);
          restarted = false;
        }
        recovery = restarted
          ? " The local backend was restarted — you are still on the old version."
          : " The local backend could not be restarted — restart StagePilot manually.";
      }
      setState((current) => ({
        ...current,
        status: "error",
        error: `${reason}${recovery}`,
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
