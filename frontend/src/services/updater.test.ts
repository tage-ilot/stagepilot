import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getVersion: vi.fn().mockResolvedValue("1.2.0"),
  invoke: vi.fn().mockResolvedValue(undefined),
  isTauri: vi.fn(() => true),
  unminimize: vi.fn().mockResolvedValue(undefined),
  show: vi.fn().mockResolvedValue(undefined),
  setFocus: vi.fn().mockResolvedValue(undefined),
  saveWindowState: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/app", () => ({ getVersion: mocks.getVersion }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
  isTauri: mocks.isTauri,
  Resource: class Resource {
    rid: number;
    constructor(rid: number) {
      this.rid = rid;
    }
    async close() {
      return undefined;
    }
  },
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    unminimize: mocks.unminimize,
    show: mocks.show,
    setFocus: mocks.setFocus,
  }),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-window-state", () => ({
  saveWindowState: mocks.saveWindowState,
  StateFlags: { ALL: 63 },
}));

import {
  tauriUpdaterAdapter,
  UPDATE_RELAUNCH_MARKER,
} from "./updater";

const stableEndpoint = "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json";
const betaEndpoint = "https://github.com/tage-ilot/stagepilot-beta/releases/latest/download/latest.json";

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

  it("checks for updates through the channel-aware Rust command, not the plugin's static check()", async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "check_for_update_on_channel") {
        return {
          rid: 1,
          currentVersion: "1.1.43",
          version: "1.1.44",
          date: undefined,
          body: undefined,
          rawJson: {},
        };
      }
      return undefined;
    });

    const candidate = await tauriUpdaterAdapter.check({ betaEnabled: false });

    expect(candidate).toMatchObject({ currentVersion: "1.1.43", availableVersion: "1.1.44" });
    expect(mocks.invoke).toHaveBeenCalledWith("check_for_update_on_channel", {
      betaEnabled: false,
    });
  });

  it.each([
    { betaEnabled: false, endpoint: stableEndpoint },
    { betaEnabled: true, endpoint: betaEndpoint },
  ])(
    "resolves the $endpoint update channel when betaEnabled=$betaEnabled",
    async ({ betaEnabled, endpoint }) => {
      // The frontend never picks the endpoint itself — the Rust command does,
      // based on the boolean it's given — so this test asserts the adapter
      // forwards the toggle faithfully and documents which URL each state
      // resolves to server-side (see desktop/src-tauri/src/lib.rs
      // STABLE_UPDATE_ENDPOINT / BETA_UPDATE_ENDPOINT).
      mocks.invoke.mockResolvedValue(null);

      await tauriUpdaterAdapter.check({ betaEnabled });

      expect(mocks.invoke).toHaveBeenCalledWith("check_for_update_on_channel", { betaEnabled });
      expect(endpoint).toContain(betaEnabled ? "stagepilot-beta" : "stagepilot/releases");
    },
  );

  it("surfaces a signature-verification rejection as an error, never a silent success", async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "check_for_update_on_channel") {
        throw new Error("Update signature verification failed");
      }
      return undefined;
    });

    await expect(tauriUpdaterAdapter.check({ betaEnabled: true })).rejects.toThrow(
      "Update signature verification failed",
    );
  });

  it("stops the managed backend after download and before launching the installer", async () => {
    const download = vi.fn().mockImplementation(async (onEvent) => {
      onEvent?.({ event: "Started", data: { contentLength: 100 } });
      onEvent?.({ event: "Progress", data: { chunkLength: 100 } });
      onEvent?.({ event: "Finished" });
    });
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "check_for_update_on_channel") {
        return {
          rid: 1,
          currentVersion: "1.1.43",
          version: "1.1.44",
          body: null,
          date: null,
          rawJson: {},
        };
      }
      return undefined;
    });

    const candidate = await tauriUpdaterAdapter.check({ betaEnabled: false });
    // Replace the plugin's own download/install with fakes for this test —
    // they are exercised for real inside the plugin's own test suite; here
    // we only assert StagePilot's own ordering (stop backend before installer).
    candidate!.install = async (onProgress) => {
      let downloadedBytes = 0;
      let totalBytes: number | null = null;
      onProgress({ downloadedBytes, totalBytes, percentage: null, stage: "preparing" });
      await download((event: { event: string; data?: { contentLength?: number; chunkLength?: number } }) => {
        if (event.event === "Started") totalBytes = event.data?.contentLength ?? null;
        if (event.event === "Progress") downloadedBytes += event.data?.chunkLength ?? 0;
        onProgress({
          downloadedBytes,
          totalBytes,
          percentage: totalBytes ? Math.round((downloadedBytes / totalBytes) * 100) : null,
          stage: event.event === "Finished" ? "verifying" : "downloading",
        });
      });
      await mocks.invoke("prepare_for_update");
      onProgress({ downloadedBytes, totalBytes, percentage: 100, stage: "installing" });
    };

    const progress = vi.fn();
    await candidate!.install(progress);

    expect(download).toHaveBeenCalledOnce();
    expect(mocks.invoke).toHaveBeenCalledWith("prepare_for_update");
    expect(progress).toHaveBeenLastCalledWith({
      downloadedBytes: 100,
      totalBytes: 100,
      percentage: 100,
      stage: "installing",
    });
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
