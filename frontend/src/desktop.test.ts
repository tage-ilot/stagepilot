import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn().mockResolvedValue(undefined),
  isTauri: vi.fn(() => true),
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/core", () => tauri);
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: tauri.openUrl }));

import { backendStartupTitle, hideDesktopWindow, openExternalUrl } from "./desktop";

describe("backend startup labels", () => {
  it("distinguishes actionable backend failures", () => {
    expect(
      backendStartupTitle({
        state: "failed",
        message: "stopped",
        port: 8765,
        managed: true,
        failure_kind: "server_exited",
        log_path: "/tmp/backend.log",
      }),
    ).toBe("The StagePilot server stopped");
    expect(
      backendStartupTitle({
        state: "failed",
        message: "occupied",
        port: 8765,
        managed: false,
        failure_kind: "port_occupied",
      }),
    ).toBe("Backend port is occupied");
  });
});

describe("desktop window controls", () => {
  beforeEach(() => {
    tauri.invoke.mockClear();
    tauri.openUrl.mockClear();
    tauri.isTauri.mockReturnValue(true);
  });

  it("hides the native window without quitting the application", async () => {
    await hideDesktopWindow();

    expect(tauri.invoke).toHaveBeenCalledWith("hide_application_window");
  });

  it("does nothing in the browser dashboard", async () => {
    tauri.isTauri.mockReturnValue(false);

    await hideDesktopWindow();

    expect(tauri.invoke).not.toHaveBeenCalled();
  });
});

describe("external links", () => {
  beforeEach(() => {
    tauri.openUrl.mockClear();
    tauri.isTauri.mockReturnValue(true);
  });

  it("uses the operating system browser from the desktop shell", async () => {
    await openExternalUrl("https://example.com/help");

    expect(tauri.openUrl).toHaveBeenCalledWith("https://example.com/help");
  });

  it("opens a new browser tab outside the desktop shell", async () => {
    tauri.isTauri.mockReturnValue(false);
    const open = vi.spyOn(window, "open").mockImplementation(() => null);

    await openExternalUrl("https://example.com/help");

    expect(open).toHaveBeenCalledWith(
      "https://example.com/help",
      "_blank",
      "noopener,noreferrer",
    );
    open.mockRestore();
  });
});
