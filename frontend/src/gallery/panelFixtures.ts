// Made-up data for the template states of the section gallery: nothing here is a real computer, account or song. No backend is asked.
import type { ApplicationState, HealthResponse, LightsStatusResponse, MidiInputsResponse, PlanningCenterServiceType, PlanningCenterStatusResponse, SettingsResponse } from "../types";
import { playbackSettings, playbackState } from "../test/playbackFixtures";

export const madeUpState = (over: Partial<ApplicationState> = {}): ApplicationState => ({
  ...playbackState,
  plugins: { planning_center: { name: "planning_center", version: "0.0.0", status: "running", last_error: null, last_activity_at: null } },
  last_successful_plan_reload_at: "2026-11-15T17:42:00Z",
  ...over,
});

export const madeUpSettings = (over: { midi_source?: "real" | "playback_api"; lights?: Partial<SettingsResponse["settings"]["lights"]>; timezone?: string; lan_access?: boolean } = {}): SettingsResponse => ({
  ...playbackSettings,
  settings: {
    ...playbackSettings.settings,
    timezone: over.timezone ?? "America/Los_Angeles",
    lan_access: over.lan_access ?? false,
    integration_modes: { ...playbackSettings.settings.integration_modes, midi_source: over.midi_source ?? "playback_api" },
    midi: { ...playbackSettings.settings.midi, mappings: { start_next: 100, restart_current: 101, previous: 102, next: 103, reload_plan: 104, stop_timer: 105 } },
    lights: { ...playbackSettings.settings.lights, enabled: true, method: "midi", output_name: "StagePilot to Lightkey", channel: 3, pulse_ms: 100, ...over.lights },
  },
});

export const madeUpServiceTypes: PlanningCenterServiceType[] = [{ id: "sunday", name: "Sunday Morning" }, { id: "wednesday", name: "Wednesday Night" }];

export const madeUpPlanningCenter = (over: Partial<PlanningCenterStatusResponse> = {}): PlanningCenterStatusResponse => ({
  connection_status: "connected", configured: true, app_id: "example-app-id", service_type_id: "sunday", planning_center_secret_saved: true,
  connection_method: "manual", oauth_connected: false, oauth_needs_reconnect: false, detail: null, ...over,
});

export const madeUpHealth = (): HealthResponse => ({ status: "healthy", version: "0.0.0-gallery", application_status: "running", plugins: [], started_at: null });

export const madeUpMidi = (over: Partial<MidiInputsResponse> = {}): MidiInputsResponse => ({
  enabled: true, channel: 1, note: 112, configured_input_name: "Playback MIDI", selected_input_name: "Playback MIDI",
  inputs: [{ id: "in-1", name: "Playback MIDI", ambiguous: false, selected: true, connected: true }, { id: "in-2", name: "Keyboard", ambiguous: false, selected: false, connected: false }],
  mappings: {}, ...over,
});

export const madeUpLights = (over: Partial<LightsStatusResponse> = {}): LightsStatusResponse => ({
  enabled: true, method: "midi", osc_host: "192.0.2.10", osc_port: 21600, output_name: "StagePilot to Lightkey", channel: 3, pulse_ms: 100, connection_status: "connected",
  detail: "Connected to lighting MIDI output StagePilot to Lightkey.", outputs: [{ name: "StagePilot to Lightkey", ambiguous: false, selected: true, connected: true }],
  last_cue: null, last_cue_at: null, ...over,
});
