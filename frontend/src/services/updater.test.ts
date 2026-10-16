import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getVersion: vi.fn().mockResolvedValue("1.2.0"),
  invoke: vi.fn().mockResolvedValue(undefined),
  isTauri: vi.fn(() => true),
  unminimize: vi.fn().mockResolvedValue(undefined),
  show: vi.fn().mockResolvedValue(undefined),
  setFocus: vi.fn().mockResolvedValue(undefined),
  saveWindowState: vi.fn().mockResolvedValue(undefined),
  download: vi.fn(),
  install: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({ getVersion: mocks.getVersion }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
  isTauri: mocks.isTauri,
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    unminimize: mocks.unminimize,
    show: mocks.show,
    setFocus: mocks.setFocus,
  }),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({
  // Minimal stand-in for the plugin's `Update` class: the adapter only
  // reads currentVersion/version/body/date off it and calls download/install.
  Update: class {
    currentVersion: string;
    version: string;
    body: string | null;
    date: string | null;
    download: typeof mocks.download;
    install: typeof mocks.install;
    constructor(metadata: {
      currentVersion: string;
      version: string;
      body?: string | null;
      date?: string | null;
    }) {
      this.currentVersion = metadata.currentVersion;
      this.version = metadata.version;
      this.body = metadata.body ?? null;
      this.date = metadata.date ?? null;
      this.download = mocks.download;
      this.install = mocks.install;
    }
  },
}));
vi.mock("@tauri-apps/plugin-window-state", () => ({
  saveWindowState: mocks.saveWindowState,
  StateFlags: { ALL: 63 },
}));

import {
  tauriUpdaterAdapter,
  UPDATE_RELAUNCH_MARKER,
} from "./updater";

describe("Tauri updater adapter relaunch state", () => {
  beforeEach(() => {
    localStorage.clear();
    location.hash = "";
    vi.clearAllMocks();
    mocks.isTauri.mockReturnValue(true);
    mocks.getVersion.mockResolvedValue("1.2.0");
  });

  it("is disabled in the browser runtime", () => {
    mocks.isTauri.mockReturnValue(false);
    expect(tauriUpdaterAdapter.isEnabled()).toBe(false);
  });

  it("saves window state and a version-bound marker before installation", async () => {
    location.hash = "#dashboard";

    await tauriUpdaterAdapter.prepareRelaunch("1.2.0");

    expect(mocks.saveWindowState).toHaveBeenCalledWith(63);
    expect(JSON.parse(localStorage.getItem(UPDATE_RELAUNCH_MARKER)!)).toMatchObject({
      targetVersion: "1.2.0",
      route: "#dashboard",
    });
  });

  it("stops the managed backend after download and before launching the installer", async () => {
    mocks.download.mockImplementation(async (onEvent: (event: unknown) => void) => {
      onEvent?.({ event: "Started", data: { contentLength: 100 } });
      onEvent?.({ event: "Progress", data: { chunkLength: 100 } });
      onEvent?.({ event: "Finished" });
    });
    mocks.install.mockResolvedValue(undefined);
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "check_for_update") {
        return {
          rid: 1,
          currentVersion: "1.1.43",
          version: "1.1.44",
          body: null,
          date: null,
        };
      }
      return undefined;
    });

    const candidate = await tauriUpdaterAdapter.check();
    const progress = vi.fn();
    await candidate!.install(progress);

    expect(mocks.download).toHaveBeenCalledOnce();
    expect(mocks.invoke).toHaveBeenCalledWith("prepare_for_update");
    expect(mocks.install).toHaveBeenCalledOnce();
    expect(mocks.download.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.invoke.mock.invocationCallOrder[1]!,
    );
    expect(mocks.invoke.mock.invocationCallOrder[1]).toBeLessThan(
      mocks.install.mock.invocationCallOrder[0]!,
    );
    expect(progress).toHaveBeenLastCalledWith({
      downloadedBytes: 100,
      totalBytes: 100,
      percentage: 100,
      stage: "installing",
    });
  });

  it("checks for updates by invoking check_for_update with no channel argument", async () => {
    mocks.invoke.mockResolvedValue(null);

    const candidate = await tauriUpdaterAdapter.check();

    expect(candidate).toBeNull();
    expect(mocks.invoke).toHaveBeenCalledWith("check_for_update");
  });

  it("returns null cleanly when no update is available", async () => {
    mocks.invoke.mockResolvedValue(null);
    const candidate = await tauriUpdaterAdapter.check();
    expect(candidate).toBeNull();
  });

  it("restores, unminimizes, shows, and focuses only after a matching update", async () => {
    localStorage.setItem(UPDATE_RELAUNCH_MARKER, JSON.stringify({
      targetVersion: "1.2.0",
      route: "#dashboard",
      createdAt: new Date().toISOString(),
    }));

    const result = await tauriUpdaterAdapter.restoreAfterRelaunch();

    expect(result).toEqual({ updatedVersion: "1.2.0", route: "#dashboard" });
    expect(mocks.unminimize).toHaveBeenCalledBefore(mocks.show);
    expect(mocks.show).toHaveBeenCalledBefore(mocks.setFocus);
    expect(localStorage.getItem(UPDATE_RELAUNCH_MARKER)).toBeNull();
  });

  it("does not claim success for an ordinary or mismatched launch", async () => {
    expect(await tauriUpdaterAdapter.restoreAfterRelaunch()).toBeNull();

    localStorage.setItem(UPDATE_RELAUNCH_MARKER, JSON.stringify({
      targetVersion: "1.3.0",
      route: null,
      createdAt: new Date().toISOString(),
    }));
    expect(await tauriUpdaterAdapter.restoreAfterRelaunch()).toBeNull();
    expect(mocks.show).not.toHaveBeenCalled();
  });
});
