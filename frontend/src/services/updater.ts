import { getVersion } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { relaunch } from "@tauri-apps/plugin-process";
import { Update } from "@tauri-apps/plugin-updater";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";
import {
  saveWindowState,
  StateFlags,
} from "@tauri-apps/plugin-window-state";

import { restartDesktopBackend } from "../desktop";

export const UPDATE_RELAUNCH_MARKER = "stagepilot.update-pending-relaunch.v1";
export const UPDATE_CHECK_TIMEOUT_MS = 15_000;

export type UpdateProgress = {
  downloadedBytes: number;
  totalBytes: number | null;
  percentage: number | null;
  stage: "preparing" | "downloading" | "verifying" | "installing";
};
export type UpdateCandidate = {
  currentVersion: string;
  availableVersion: string;
  releaseNotes: string | null;
  releaseDate: string | null;
  install: (onProgress: (progress: UpdateProgress) => void) => Promise<void>;
};

/**
 * Where StagePilot is actually running from, as reported by the Rust
 * `install_environment` command. Mirrors `macos_install::InstallEnvironment`.
 *
 * `updatesBlocked` is true when macOS Gatekeeper App Translocation is running
 * the bundle from a randomized read-only image: any in-place update install
 * against that path is guaranteed to fail *after* the download completes and
 * the managed backend has already been stopped, which is exactly the confusing
 * "UPDATE INTERRUPTED" failure this guard exists to prevent.
 */
export type InstallEnvironment = {
  executablePath: string | null;
  translocated: boolean;
  updatesBlocked: boolean;
  guidance: string | null;
};

export type UpdateRelaunchResult = {
  updatedVersion: string;
  route: string | null;
} | null;

export interface UpdaterAdapter {
  isEnabled(): boolean;
  check(): Promise<UpdateCandidate | null>;
  prepareRelaunch(version: string): Promise<void>;
  clearRelaunchMarker(): void;
  relaunch(): Promise<void>;
  restoreAfterRelaunch(): Promise<UpdateRelaunchResult>;
  /**
   * Bring the managed backend back after a failed install. `prepare_for_update`
   * deliberately kills the managed backend so the installer can replace the
   * binary on disk; if the install then fails, nothing else restarts it.
   * Resolves `true` when the backend was restarted.
   */
  restartBackend?(): Promise<boolean>;
  /**
   * Reports the real install location. Resolves `null` when the host cannot
   * answer (older shell, non-Tauri runtime), which is treated as "not blocked"
   * so the happy path is never gated on this probe.
   */
  installEnvironment?(): Promise<InstallEnvironment | null>;
}

type RelaunchMarker = {
  targetVersion: string;
  route: string | null;
  createdAt: string;
};

const safeReadMarker = (): RelaunchMarker | null => {
  try {
    const parsed = JSON.parse(localStorage.getItem(UPDATE_RELAUNCH_MARKER) ?? "null");
    if (
      parsed
      && typeof parsed.targetVersion === "string"
      && (parsed.route === null || typeof parsed.route === "string")
      && typeof parsed.createdAt === "string"
    ) {
      return parsed as RelaunchMarker;
    }
  } catch {
    // Invalid marker data must never block StagePilot startup.
  }
  return null;
};

const normalizeProgress = (
  event: DownloadEvent,
  downloadedBytes: number,
  totalBytes: number | null,
) => {
  if (event.event === "Started") {
    totalBytes = event.data.contentLength ?? null;
  } else if (event.event === "Progress") {
    downloadedBytes += event.data.chunkLength;
  }
  return {
    downloadedBytes,
    totalBytes,
    percentage: totalBytes && totalBytes > 0
      ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100))
      : null,
  };
};

type UpdateMetadata = {
  rid: number;
  currentVersion: string;
  version: string;
  date?: string;
  body?: string;
  rawJson: Record<string, unknown>;
};

const candidateFromUpdate = (update: Update): UpdateCandidate => ({
  currentVersion: update.currentVersion,
  availableVersion: update.version,
  releaseNotes: update.body?.trim() || null,
  releaseDate: update.date ?? null,
  install: async (onProgress) => {
    let downloadedBytes = 0;
    let totalBytes: number | null = null;
    onProgress({ downloadedBytes, totalBytes, percentage: null, stage: "preparing" });
    await update.download((event) => {
      const normalized = normalizeProgress(event, downloadedBytes, totalBytes);
      downloadedBytes = normalized.downloadedBytes;
      totalBytes = normalized.totalBytes;
      if (event.event === "Finished") {
        onProgress({ ...normalized, stage: "verifying" });
      } else {
        onProgress({ ...normalized, stage: "downloading" });
      }
    }, { timeout: 120_000 });
    await invoke("prepare_for_update");
    onProgress({
      downloadedBytes,
      totalBytes,
      percentage: totalBytes ? 100 : null,
      stage: "installing",
    });
    await update.install();
  },
});

export const tauriUpdaterAdapter: UpdaterAdapter = {
  isEnabled: () => {
    if (!isTauri()) return false;
    if (import.meta.env.PROD) return true;
    return import.meta.env.VITE_STAGEPILOT_ENABLE_UPDATER === "true";
  },
  check: async () => {
    // The stock plugin `check()` cannot hand its resource id back to us, so the check goes through one
    // Rust command that builds the updater from tauri.conf.json (same signature verification as the default
    // path) and returns the same resource-table-backed metadata shape.
    const metadata = await invoke<UpdateMetadata | null>("check_for_update");
    if (!metadata) return null;
    const update = new Update(metadata);
    return candidateFromUpdate(update);
  },
  prepareRelaunch: async (version) => {
    const marker: RelaunchMarker = {
      targetVersion: version,
      route: window.location.hash || null,
      createdAt: new Date().toISOString(),
    };
    localStorage.setItem(UPDATE_RELAUNCH_MARKER, JSON.stringify(marker));
    await saveWindowState(StateFlags.ALL);
  },
  clearRelaunchMarker: () => {
    localStorage.removeItem(UPDATE_RELAUNCH_MARKER);
  },
  relaunch,
  restartBackend: restartDesktopBackend,
  installEnvironment: async () => {
    try {
      return await invoke<InstallEnvironment>("install_environment");
    } catch (cause) {
      // A shell that predates this command must not break update checks.
      console.warn("StagePilot could not determine its install location.", cause);
      return null;
    }
  },
  restoreAfterRelaunch: async () => {
    const marker = safeReadMarker();
    if (!marker) return null;
    const currentVersion = await getVersion();
    const createdAt = Date.parse(marker.createdAt);
    const stale = !Number.isFinite(createdAt) || Date.now() - createdAt > 24 * 60 * 60 * 1_000;
    if (stale || currentVersion !== marker.targetVersion) {
      localStorage.removeItem(UPDATE_RELAUNCH_MARKER);
      return null;
    }
    if (marker.route?.startsWith("#")) window.location.hash = marker.route;
    const mainWindow = getCurrentWindow();
    await mainWindow.unminimize();
    await mainWindow.show();
    await mainWindow.setFocus();
    localStorage.removeItem(UPDATE_RELAUNCH_MARKER);
    return { updatedVersion: currentVersion, route: marker.route };
  },
};
