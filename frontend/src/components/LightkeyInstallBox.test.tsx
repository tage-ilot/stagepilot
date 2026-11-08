import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { LightkeyHelperInstall, LightkeyStatus } from "../types";
import { LightkeyInstallBox } from "./LightkeyInstallBox";
import { countdownText } from "./lightkeyProject";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  startLightkeyHelperInstall: vi.fn(),
  endLightkeyHelperInstall: vi.fn(),
}));

const status = (over: Partial<LightkeyStatus> = {}): LightkeyStatus => ({
  api_url: "http://192.168.1.20:8770",
  link: "unreachable",
  detail: "No answer.",
  active_count: null,
  recording: false,
  recording_song: null,
  replay: true,
  replay_blocked: null,
  replaying_song: null,
  message: null,
  ...over,
});
const connected = (over: Partial<LightkeyStatus> = {}) => status({ link: "connected", detail: null, ...over });
const oldHelper = { outdated: true } as LightkeyStatus["helper"];
const newHelper = { outdated: false, version: "0.5.0" } as LightkeyStatus["helper"];

const made = (minutes = 30): LightkeyHelperInstall => ({
  command: "curl -fsS http://192.168.1.20:8000/lightkey-helper/abc123/install.sh | sh",
  url: "http://192.168.1.20:8000/lightkey-helper/abc123/install.sh",
  expires_at: new Date(Date.now() + minutes * 60_000).toISOString(),
  lan_ip: "192.168.1.20",
  port: 8000,
  bundled_version: "0.5.0",
});

beforeEach(() => {
  vi.mocked(api.endLightkeyHelperInstall).mockResolvedValue({ ended: true });
});
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); });

describe("LightkeyInstallBox", () => {
  it("offers the setup button while no lighting computer is connected", () => {
    render(<LightkeyInstallBox status={status()} />);
    expect(screen.getByRole("region", { name: "Set up the Lighting computer" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Set up the Lighting computer" })).toBeEnabled();
    expect(api.startLightkeyHelperInstall).not.toHaveBeenCalled();
  });

  it("shows the box for every link state except connected (and Off, which has nothing to set up yet)", () => {
    for (const link of ["unreachable", "lightkey_closed"] as const) {
      const { unmount } = render(<LightkeyInstallBox status={status({ link })} />);
      expect(screen.getByTestId("lightkey-install")).toBeInTheDocument();
      unmount();
    }
    for (const over of [connected({ helper: newHelper }), connected(), status({ link: "off" })]) {
      const { unmount } = render(<LightkeyInstallBox status={over} />);
      expect(screen.queryByTestId("lightkey-install")).not.toBeInTheDocument();
      unmount();
    }
  });

  it("makes the command and shows the numbered steps, the command, a countdown and the waiting line", async () => {
    vi.mocked(api.startLightkeyHelperInstall).mockResolvedValue(made());
    render(<LightkeyInstallBox status={status()} />);
    await userEvent.click(screen.getByRole("button", { name: "Set up the Lighting computer" }));
    const ready = await screen.findByTestId("lightkey-install-ready");
    expect(api.startLightkeyHelperInstall).toHaveBeenCalledWith(undefined);
    const steps = ready.querySelectorAll("ol > li");
    expect(steps).toHaveLength(3);
    expect(steps[0]).toHaveTextContent("turn on sACN output in Lightkey and open your project");
    expect(steps[1]).toHaveTextContent("Open Terminal there and paste this command");
    expect(steps[2]).toHaveTextContent("Wait here. It connects by itself.");
    expect(screen.getByTestId("lightkey-install-command")).toHaveTextContent(made().command);
    expect(screen.getByTestId("lightkey-install-command").tagName).toBe("CODE");
    expect(screen.getByTestId("lightkey-install-waiting")).toHaveTextContent(/Waiting for the helper… The command works for (30:00|29:5\d) more/);
    expect(ready).toHaveTextContent("xcode-select --install");
    expect(ready).toHaveTextContent("Lightkey API");
    expect(ready).toHaveTextContent("local network");
    expect(ready).toHaveTextContent("Documents and Downloads");
    expect(ready).toHaveTextContent("Click Allow");
    expect(screen.getByTestId("lightkey-install-live")).toHaveTextContent("Waiting for the helper…");
    expect(screen.getByTestId("lightkey-install-live")).toHaveAttribute("aria-live", "polite");
  });

  it("copies the command", async () => {
    vi.mocked(api.startLightkeyHelperInstall).mockResolvedValue(made());
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<LightkeyInstallBox status={status()} />);
    await userEvent.click(screen.getByRole("button", { name: "Set up the Lighting computer" }));
    await userEvent.click(await screen.findByRole("button", { name: "Copy" }));
    expect(writeText).toHaveBeenCalledWith(made().command);
    expect(await screen.findByRole("button", { name: "Copied" })).toBeInTheDocument();
  });

  it("says so when copying is not possible and keeps the command to copy by hand", async () => {
    vi.mocked(api.startLightkeyHelperInstall).mockResolvedValue(made());
    Object.defineProperty(navigator, "clipboard", { value: { writeText: vi.fn().mockRejectedValue(new Error("no")) }, configurable: true });
    render(<LightkeyInstallBox status={status()} />);
    await userEvent.click(screen.getByRole("button", { name: "Set up the Lighting computer" }));
    await userEvent.click(await screen.findByRole("button", { name: "Copy" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Select the command and copy it by hand");
    expect(screen.getByTestId("lightkey-install-command")).toBeInTheDocument();
  });

  it("collapses and ends the session when the helper connects", async () => {
    vi.mocked(api.startLightkeyHelperInstall).mockResolvedValue(made());
    const { rerender } = render(<LightkeyInstallBox status={status()} />);
    await userEvent.click(screen.getByRole("button", { name: "Set up the Lighting computer" }));
    await screen.findByTestId("lightkey-install-ready");
    rerender(<LightkeyInstallBox status={connected({ helper: newHelper })} />);
    await waitFor(() => expect(screen.queryByTestId("lightkey-install")).not.toBeInTheDocument());
    expect(api.endLightkeyHelperInstall).toHaveBeenCalledTimes(1);
    // The live region stays so the change is announced.
    expect(screen.getByTestId("lightkey-install-live")).toHaveTextContent("The Lighting computer is connected.");
    expect(screen.getByTestId("lightkey-install-live")).toHaveAttribute("aria-live", "polite");
  });

  it("expires with a plain message and a button to make a new command", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(api.startLightkeyHelperInstall).mockResolvedValueOnce(made(1)).mockResolvedValueOnce(made());
    render(<LightkeyInstallBox status={status()} />);
    await act(async () => { screen.getByRole("button", { name: "Set up the Lighting computer" }).click(); });
    await screen.findByTestId("lightkey-install-ready");
    await act(async () => { await vi.advanceTimersByTimeAsync(61_000); });
    expect(screen.getByTestId("lightkey-install-problem")).toHaveTextContent("That command has expired. Make a new one.");
    expect(screen.queryByTestId("lightkey-install-ready")).not.toBeInTheDocument();
    await act(async () => { screen.getByRole("button", { name: "Make a new command" }).click(); });
    expect(await screen.findByTestId("lightkey-install-ready")).toBeInTheDocument();
    expect(api.startLightkeyHelperInstall).toHaveBeenCalledTimes(2);
  });

  it("counts down", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(api.startLightkeyHelperInstall).mockResolvedValue(made(10));
    render(<LightkeyInstallBox status={status()} />);
    await act(async () => { screen.getByRole("button", { name: "Set up the Lighting computer" }).click(); });
    await screen.findByTestId("lightkey-install-ready");
    expect(screen.getByTestId("lightkey-install-waiting")).toHaveTextContent(/(10:00|9:5\d)/);
    await act(async () => { await vi.advanceTimersByTimeAsync(5 * 60_000); });
    expect(screen.getByTestId("lightkey-install-waiting")).toHaveTextContent(/(5:00|4:5\d)/);
  });

  it("formats the countdown", () => {
    expect(countdownText(30 * 60_000)).toBe("30:00");
    expect(countdownText(61_000)).toBe("1:01");
    expect(countdownText(500)).toBe("0:01");
    expect(countdownText(-5)).toBe("0:00");
  });

  it("shows a plain error and lets the person try again", async () => {
    vi.mocked(api.startLightkeyHelperInstall).mockRejectedValueOnce(new api.ApiError("StagePilot hit an error.", 500)).mockResolvedValueOnce(made());
    render(<LightkeyInstallBox status={status()} />);
    await userEvent.click(screen.getByRole("button", { name: "Set up the Lighting computer" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("StagePilot hit an error. Try again.");
    expect(screen.queryByRole("textbox", { name: "This computer's address" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByTestId("lightkey-install-ready")).toBeInTheDocument();
  });

  it("asks for this computer's address when it could not be found, then uses it", async () => {
    vi.mocked(api.startLightkeyHelperInstall)
      .mockRejectedValueOnce(new api.ApiError("Could not find this computer's address on the Lighting network", 409))
      .mockResolvedValueOnce(made());
    render(<LightkeyInstallBox status={status()} />);
    await userEvent.click(screen.getByRole("button", { name: "Set up the Lighting computer" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not find this computer's address on the Lighting network");
    const field = screen.getByRole("textbox", { name: "This computer's address" });
    const use = screen.getByRole("button", { name: "Use this address" });
    expect(use).toBeDisabled();
    await userEvent.type(field, "192.168.1.20");
    await userEvent.click(use);
    expect(api.startLightkeyHelperInstall).toHaveBeenLastCalledWith("192.168.1.20");
    expect(await screen.findByTestId("lightkey-install-ready")).toBeInTheDocument();
  });

  it("can be cancelled, which ends the session", async () => {
    vi.mocked(api.startLightkeyHelperInstall).mockResolvedValue(made());
    render(<LightkeyInstallBox status={status()} />);
    await userEvent.click(screen.getByRole("button", { name: "Set up the Lighting computer" }));
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(api.endLightkeyHelperInstall).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Set up the Lighting computer" })).toBeInTheDocument();
  });

  describe("Update the helper", () => {
    it("is offered when a connected helper is older than the bundled one", () => {
      render(<LightkeyInstallBox status={connected({ helper: oldHelper })} />);
      expect(screen.getByRole("region", { name: "Update the helper" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Update the helper" })).toBeEnabled();
      expect(screen.getByTestId("lightkey-install")).toHaveTextContent("older than the one StagePilot carries");
    });

    it("shows the same steps and stays until the helper is current", async () => {
      vi.mocked(api.startLightkeyHelperInstall).mockResolvedValue(made());
      const { rerender } = render(<LightkeyInstallBox status={connected({ helper: oldHelper })} />);
      await userEvent.click(screen.getByRole("button", { name: "Update the helper" }));
      await screen.findByTestId("lightkey-install-ready");
      // Still connected, still old: keep waiting (a restart may drop and return the link).
      rerender(<LightkeyInstallBox status={connected({ helper: oldHelper })} />);
      rerender(<LightkeyInstallBox status={status()} />);
      expect(screen.getByTestId("lightkey-install-ready")).toBeInTheDocument();
      expect(api.endLightkeyHelperInstall).not.toHaveBeenCalled();
      rerender(<LightkeyInstallBox status={connected({ helper: newHelper })} />);
      await waitFor(() => expect(screen.queryByTestId("lightkey-install")).not.toBeInTheDocument());
      expect(api.endLightkeyHelperInstall).toHaveBeenCalledTimes(1);
      expect(screen.getByTestId("lightkey-install-live")).toHaveTextContent("connected");
    });
  });
});
