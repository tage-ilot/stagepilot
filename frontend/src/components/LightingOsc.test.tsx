import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ApplicationState, LightsStatusResponse, SettingsResponse, Song } from "../types";
import { LightingSetupPanel } from "./LightingSetupPanel";
import {
  oscAddressError,
  oscPortError,
  parseOscArgument,
  parseOscArguments,
} from "./lightingOsc";

const song: Song = { id: "item-1", source_song_id: "song-1", title: "Holy Forever", duration_seconds: 336, order: 1, is_generic: false };

const baseLights = {
  osc_host: "127.0.0.1",
  osc_port: 21600,
  enabled: true,
  output_name: null,
  channel: 1,
  pulse_ms: 100,
};

const lights: LightsStatusResponse = {
  ...baseLights,
  method: "osc",
  connection_status: "connected",
  detail: null,
  outputs: [],
  last_cue: null,
  last_osc_cue: null,
  last_cue_at: null,
};

const settings = {
  planning_center_secret_saved: true,
  warning: null,
  restart_required: false,
  settings: {
    lights: {
      ...baseLights,
      method: "osc",
      cue_maps: {
        "song-1": {
          song_key: "song-1",
          song_title: "Holy Forever",
          cues: [{ id: "c17d19ab-1447-4e73-898e-468b2dfa87c7", at_seconds: 65, note: 72, velocity: 110, label: "Midi kept" }],
          osc_cues: [{
            id: "0c9a3d7e-7c5e-4a3a-9d54-0d6c7a1b2c3d",
            at_seconds: 30,
            address: "/lightkey/cue/1",
            args: [{ type: "float", value: 1 }],
            label: "Osc cue",
          }],
        },
      },
    },
  },
} as unknown as SettingsResponse;

const state = {
  plan: { id: "p", title: "Sunday", songs: [song] },
  current_song: song,
} as unknown as ApplicationState;

const mount = (extra: Partial<React.ComponentProps<typeof LightingSetupPanel>> = {}) =>
  render(
    <LightingSetupPanel
      error={null}
      lights={lights}
      message={null}
      onRefresh={vi.fn()}
      onSaveCues={vi.fn()}
      onSaveSettings={vi.fn()}
      onTest={vi.fn()}
      onTestOsc={vi.fn()}
      pendingOperation={null}
      settings={settings}
      state={state}
      {...extra}
    />,
  );

describe("OSC helpers", () => {
  it("validates addresses: leading slash, no spaces", () => {
    expect(oscAddressError("/cue/1")).toBeNull();
    expect(oscAddressError("cue/1")).toMatch(/Start the address with \//);
    expect(oscAddressError("/cue 1")).toMatch(/spaces/);
    expect(oscAddressError("  ")).toMatch(/Enter an OSC address/);
  });

  it("validates ports", () => {
    expect(oscPortError("21600")).toBeNull();
    expect(oscPortError("0")).not.toBeNull();
    expect(oscPortError("70000")).not.toBeNull();
    expect(oscPortError("")).not.toBeNull();
    expect(oscPortError("7.5")).not.toBeNull();
  });

  it("parses typed arguments and keeps floats as floats", () => {
    expect(parseOscArgument({ type: "int", text: "7" })).toEqual({ type: "int", value: 7 });
    expect(parseOscArgument({ type: "float", text: "1" })).toEqual({ type: "float", value: 1 });
    expect(parseOscArgument({ type: "bool", text: "false" })).toEqual({ type: "bool", value: false });
    expect(parseOscArgument({ type: "string", text: " go " })).toEqual({ type: "string", value: " go " });
    expect(typeof parseOscArgument({ type: "int", text: "1.5" })).toBe("string");
    expect(typeof parseOscArgument({ type: "float", text: "abc" })).toBe("string");
    expect(typeof parseOscArgument({ type: "int", text: "99999999999" })).toBe("string");
    expect(parseOscArguments([{ type: "int", text: "1" }, { type: "float", text: "x" }]).error).toBe("Enter a number.");
  });
});

describe("LightingSetupPanel OSC", () => {
  it("shows the Connection method switch with OSC primary and MIDI alternate, OSC chosen when saved", () => {
    mount();
    const group = screen.getByRole("radiogroup", { name: "Connection method" });
    const osc = within(group).getByRole("radio", { name: /OSC/ });
    expect(osc).toBeChecked();
    expect(within(group).getByRole("radio", { name: /MIDI/ })).not.toBeChecked();
    expect(group.querySelector("[data-method-pill]")?.className).toContain("integration-lighting-osc");
    expect(group.querySelector('[data-role="primary"]')).toHaveTextContent("OSC");
    expect(group.querySelector('[data-role="alternate"]')).toHaveTextContent("MIDI");
    expect(screen.queryByLabelText("MIDI Output")).not.toBeInTheDocument();
  });

  it("has no Preset option: the port is typed in, with no guessed default, and nothing is offered as a quick action", async () => {
    mount();
    expect(screen.queryByLabelText("Preset")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Action")).not.toBeInTheDocument();
    expect(screen.queryByText(/Fill from an action/)).not.toBeInTheDocument();
    for (const guess of [/Chamsys/i, /QLC\+/, /ETC Eos/i, /GrandMA/i, /Resolume/i, /Playback 1 go/, /Executor 201/]) {
      expect(screen.queryByText(guess)).not.toBeInTheDocument();
    }
    expect(screen.getByLabelText("UDP port")).toBeInTheDocument();
    expect(screen.getByText(/Enter the port your lighting app listens for OSC on/)).toBeInTheDocument();
  });

  it("saves the OSC settings with the method", async () => {
    const user = userEvent.setup();
    const onSaveSettings = vi.fn();
    mount({ onSaveSettings });
    await user.clear(screen.getByLabelText("Host or IP address"));
    await user.type(screen.getByLabelText("Host or IP address"), "192.168.4.50");
    await user.clear(screen.getByLabelText("UDP port"));
    await user.type(screen.getByLabelText("UDP port"), "8000");
    await user.click(screen.getByRole("button", { name: /Save/ }));
    expect(onSaveSettings).toHaveBeenCalledWith(expect.objectContaining({ enabled: true, method: "osc", osc_host: "192.168.4.50", osc_port: 8000 }));
    expect(onSaveSettings.mock.calls[0]?.[0]).not.toHaveProperty("osc_preset");
  });

  it("refuses to save an empty or invalid port", async () => {
    const user = userEvent.setup();
    const onSaveSettings = vi.fn();
    mount({ onSaveSettings });
    await user.clear(screen.getByLabelText("UDP port"));
    await user.click(screen.getByRole("button", { name: /Save/ }));
    expect(onSaveSettings).not.toHaveBeenCalled();
    expect(screen.getByText(/UDP port from 1 to 65535/)).toBeInTheDocument();
  });

  it("switching to MIDI shows MIDI cues and keeps the OSC cues; switching back restores them", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    mount({ onSaveCues });
    expect(screen.getByDisplayValue("Osc cue")).toBeInTheDocument();
    expect(screen.getByDisplayValue("/lightkey/cue/1")).toBeInTheDocument();
    expect(screen.queryByDisplayValue("Midi kept")).not.toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: /MIDI/ }));
    expect(screen.getByDisplayValue("Midi kept")).toBeInTheDocument();
    expect(screen.queryByDisplayValue("Osc cue")).not.toBeInTheDocument();
    expect(screen.getByLabelText("MIDI Output")).toBeInTheDocument();
    expect(onSaveCues).not.toHaveBeenCalled(); // nothing is deleted by switching

    await user.click(screen.getByRole("radio", { name: /OSC/ }));
    expect(screen.getByDisplayValue("Osc cue")).toBeInTheDocument();
    expect(onSaveCues).not.toHaveBeenCalled();
  });

  it("saving an OSC cue reports method osc and a typed argument", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    mount({ onSaveCues });
    const address = screen.getByDisplayValue("/lightkey/cue/1");
    await user.clear(address);
    await user.type(address, "/lightkey/cue/2");
    await user.tab();
    expect(onSaveCues).toHaveBeenLastCalledWith(
      song,
      [expect.objectContaining({ at_seconds: 30, address: "/lightkey/cue/2", args: [{ type: "float", value: 1 }], label: "Osc cue" })],
      "osc",
    );
  });

  it("does not save a cue with an invalid address and shows why", async () => {
    const user = userEvent.setup();
    const onSaveCues = vi.fn();
    mount({ onSaveCues });
    const address = screen.getByDisplayValue("/lightkey/cue/1");
    await user.clear(address);
    await user.type(address, "no slash");
    await user.tab();
    expect(onSaveCues).not.toHaveBeenCalled();
    expect(screen.getByText("Start the address with /.")).toBeInTheDocument();
  });

  it("Send test cue sends the address and typed args through onTestOsc", async () => {
    const user = userEvent.setup();
    const onTestOsc = vi.fn();
    mount({ onTestOsc });
    const group = screen.getByRole("group", { name: "OSC test cue" });
    const send = within(group).getByRole("button", { name: "Send test cue" });
    expect(send).toBeDisabled();
    await user.type(within(group).getByLabelText("OSC address"), "/test/go");
    await user.click(within(group).getByRole("button", { name: "Add argument" }));
    await user.clear(within(group).getByLabelText("Test argument 1 value"));
    await user.type(within(group).getByLabelText("Test argument 1 value"), "5");
    await user.click(send);
    expect(onTestOsc).toHaveBeenCalledWith("/test/go", [{ type: "int", value: 5 }]);
  });

  it("OSC has no note length or MIDI test controls", () => {
    mount();
    expect(screen.queryByLabelText("Note length (ms)")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Velocity")).not.toBeInTheDocument();
  });

  it("copy has no em dashes and says UDP does not confirm delivery", () => {
    const { container } = mount();
    expect(container.textContent).not.toContain("—");
    expect(container.textContent).toContain("UDP");
  });
});
