import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ApplicationState, LightkeyCleanGlitchesResponse, LightsStatusResponse, OscLightingCue, SettingsResponse, Song } from "../types";
import { LightingSetupPanel } from "./LightingSetupPanel";

const song: Song = { id: "item-1", source_song_id: "song-1", title: "Holy Forever", duration_seconds: 300, order: 1, is_generic: false };
const base = { osc_host: "127.0.0.1", osc_port: 21600, enabled: true, output_name: null, channel: 1, pulse_ms: 100 };
const lights = { ...base, method: "osc", connection_status: "connected", detail: null, outputs: [], last_cue: null, last_osc_cue: null, last_cue_at: null } as LightsStatusResponse;
const state = { plan: { id: "p", title: "Sunday", songs: [song] }, current_song: song } as unknown as ApplicationState;

vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), getLightkeyStatus: vi.fn(() => Promise.reject(new Error("offline"))) }));

const row = (id: string, at: number, source: "captured" | "manual" = "captured"): OscLightingCue => ({
  id, at_seconds: at, address: "", args: [], label: "", source,
  messages: [{ address: "/fixture/P1/overrides/dimmer", args: [{ type: "float", value: 0.5 }] }],
});
const settingsWith = (cues: OscLightingCue[], capturedEdited = false) => ({
  planning_center_secret_saved: true, warning: null, restart_required: false,
  settings: { lights: { ...base, method: "osc", cue_maps: { "song-1": { song_key: "song-1", song_title: "Holy Forever", cues: [], osc_cues: cues, ...(capturedEdited ? { captured_edited: true } : {}) } } } },
}) as unknown as SettingsResponse;

const answer = (over: Partial<LightkeyCleanGlitchesResponse>): LightkeyCleanGlitchesResponse => ({
  accepted: true, applied: false, message: "", flashes: 0, lights: 0, flashes_after: 0, rows_before: 0, rows_after: 0, messages_before: 0, messages_after: 0, loops: 0, ...over,
});

const mount = (cues: OscLightingCue[], onCleanGlitches: ((songKey: string, apply: boolean) => Promise<LightkeyCleanGlitchesResponse | null>) | undefined, onSaveCues = vi.fn(), capturedEdited = false) => {
  render(
    <LightingSetupPanel error={null} lights={lights} message={null} onCleanGlitches={onCleanGlitches} onRefresh={vi.fn()} onSaveCues={onSaveCues} onSaveSettings={vi.fn()}
      onTest={vi.fn()} onTestOsc={vi.fn()} pendingOperation={null} settings={settingsWith(cues, capturedEdited)} state={state} />,
  );
  return onSaveCues;
};

afterEach(() => vi.restoreAllMocks());

describe("Clean up glitches", () => {
  it("is one button in the timeline toolbar of a song that has captured rows, and nowhere else", () => {
    mount([row("a", 1)], vi.fn());
    expect(screen.getByRole("button", { name: "Clean up glitches…" })).toBeInTheDocument();
  });

  it("is not there for a song with only hand-made rows", () => {
    mount([row("a", 1, "manual")], vi.fn());
    expect(screen.queryByRole("button", { name: /Clean up glitches/ })).toBeNull();
  });

  it("asks first, with what it found, and changes nothing until the answer is yes", async () => {
    const clean = vi.fn(async (_key: string, apply: boolean) => apply ? answer({ applied: true, flashes: 312 }) : answer({ flashes: 312, lights: 16 }));
    const save = mount([row("a", 1)], clean);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Clean up glitches…" }));
    const dialog = await screen.findByRole("dialog", { name: "Clean up glitches?" });
    expect(dialog).toHaveTextContent("Found 312 flashes on 16 lights in Holy Forever. Remove them? You can Undo.");
    expect(clean).toHaveBeenCalledExactlyOnceWith("song-1", false);
    // the safe answer has the focus; saying no changes nothing
    expect(within(dialog).getByRole("button", { name: "Keep as is" })).toHaveFocus();
    await user.click(within(dialog).getByRole("button", { name: "Keep as is" }));
    expect(screen.queryByRole("dialog", { name: "Clean up glitches?" })).toBeNull();
    expect(clean).toHaveBeenCalledTimes(1);
    expect(save).not.toHaveBeenCalled();
  });

  it("says one flash on one light in the singular", async () => {
    mount([row("a", 1)], vi.fn(async () => answer({ flashes: 1, lights: 1 })));
    await userEvent.setup().click(screen.getByRole("button", { name: "Clean up glitches…" }));
    expect(await screen.findByRole("dialog", { name: "Clean up glitches?" })).toHaveTextContent("Found 1 flash on 1 light in");
  });

  it("repairs after yes and offers one Undo that puts the old rows back with the flag they had", async () => {
    const clean = vi.fn(async (_key: string, apply: boolean) => apply ? answer({ applied: true, flashes: 312 }) : answer({ flashes: 312, lights: 16 }));
    const rows = [row("a", 1), row("b", 2)];
    const save = mount(rows, clean, vi.fn(), true);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Clean up glitches…" }));
    await user.click(within(await screen.findByRole("dialog", { name: "Clean up glitches?" })).getByRole("button", { name: "Remove flashes" }));
    await waitFor(() => expect(clean).toHaveBeenCalledWith("song-1", true));
    const undo = await screen.findByRole("button", { name: "Undo clean-up" });
    expect(save).not.toHaveBeenCalled();
    await user.click(undo);
    expect(save).toHaveBeenCalledTimes(1);
    const [, cues, method, options] = save.mock.calls[0]!;
    expect(method).toBe("osc");
    expect(cues).toEqual(rows);
    expect(options).toEqual({ capturedEdited: true });
    expect(screen.queryByRole("button", { name: "Undo clean-up" })).toBeNull();
  });

  it("with nothing found (or nothing asked) there is no dialog and no Undo", async () => {
    const clean = vi.fn(async () => answer({ flashes: 0, message: "No glitches found in this song." }));
    mount([row("a", 1)], clean);
    await userEvent.setup().click(screen.getByRole("button", { name: "Clean up glitches…" }));
    await waitFor(() => expect(clean).toHaveBeenCalled());
    expect(screen.queryByRole("dialog", { name: "Clean up glitches?" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Undo clean-up" })).toBeNull();
  });

  it("is not offered when the app gives no way to ask", () => {
    mount([row("a", 1)], undefined);
    expect(screen.queryByRole("button", { name: /Clean up glitches/ })).toBeNull();
  });
});
