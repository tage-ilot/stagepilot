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

export type UpdateRelaunchResult = {
  updatedVersion: string;
  route: string | null;
} | null;

export interface UpdaterAdapter {
  isEnabled(): boolean;
  check(options?: { betaEnabled?: boolean }): Promise<UpdateCandidate | null>;
  prepareRelaunch(version: string): Promise<void>;
  clearRelaunchMarker(): void;
  relaunch(): Promise<void>;
  restoreAfterRelaunch(): Promise<UpdateRelaunchResult>;
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

type ChannelUpdateMetadata = {
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
  check: async (options) => {
    // The stock `check()` export from `@tauri-apps/plugin-updater` cannot
    // select a non-default endpoint at runtime (its `CheckOptions` has no
    // endpoints override in 2.10.1), so channel selection goes through a
    // custom Rust command instead. That command still builds its updater
    // via the plugin's own `updater_builder()` and returns the same
    // resource-table-backed metadata shape the plugin's own `check` IPC
    // command does, so `Update.download()`/`Update.install()` — and their
    // signature verification — are completely unchanged.
    const metadata = await invoke<ChannelUpdateMetadata | null>(
      "check_for_update_on_channel",
      { betaEnabled: options?.betaEnabled ?? false },
    );
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
