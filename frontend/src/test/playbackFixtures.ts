import type { ApplicationState, PlaybackStatusResponse, SettingsResponse } from "../types";

export const playbackStatus = (overrides: Partial<PlaybackStatusResponse> = {}): PlaybackStatusResponse => ({
  selected: true, enabled: true, connected: true, active_source: "playback_api",
  reason: "Connected to Playback on 192.0.2.10.",
  scan: { state: "idle", candidates: [], reason: null, current: 0, total: 0 },
  sources: {
    playback_api: { connected: true, reason: "Playback is connected." },
    midi: { connected: false, reason: "MIDI is not connected." },
  },
  host: "192.0.2.10", port: 8080, source: "lan", last_error: null,
  playing: false, setlist_cloud_version: 1, discovery: "done", progress: 0,
  discovery_song: 0, discovery_total: 0, song_lengths: [], lengths_measured_at: null, plan_songs: [],
  song_order: [101, 202], song_count: 2, plan_song_count: 2, captured_version: 1, captured_at: null, stale: false,
  settings: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true }, ...overrides,
});

export const playbackSettings: SettingsResponse = {
  settings: {
    schema_version: 2, onboarding: { general_completed: true },
    integration_modes: { service_source: "planning_center", midi_source: "playback_api", timer_output: "propresenter" },
    timezone: "America/Los_Angeles", log_level: "INFO", server_port: 8765,
    planning_center: { app_id: "example", service_type_id: "example", plan_title_preference: null, preferred_service_time: null, upcoming_lookahead_days: 30, request_timeout_seconds: 10 },
    midi: { enabled: true, input_name: "Example MIDI", channel: 1, note: 112, debounce_ms: 250, mappings: { start_next: 100 } },
    playback_api: { enabled: true, host: null, port: 8080, auto_scan: true, fast_transport: true, song_order: [101, 202], song_lengths: [], lengths_measured_at: null, captured_version: 1, captured_at: null },
    lights: { enabled: false, output_name: null, channel: 1, pulse_ms: 100, cue_maps: {} },
    propresenter: { enabled: true, host: "127.0.0.1", port: 1025, timer_name: "Example Timer", request_timeout_seconds: 3, reconnect_initial_seconds: 1, reconnect_max_seconds: 30, health_check_interval_seconds: 10 },
  }, planning_center_secret_saved: true, warning: null, restart_required: false,
};

export const playbackState: ApplicationState = {
  revision: 1, updated_at: "2026-01-01T00:00:00Z", application_status: "running",
  plan: null, current_song: null, next_song: null, current_song_index: null, playing_song: null, playing_song_index: null, up_next_song: null,
  planning_center_status: "connected", midi_status: "connected", propresenter_status: "connected", lights_status: "disconnected",
  service_load: { status: "loaded", target_date: "2026-01-01", candidates: [], skipped_items: [], message: null, is_stale: false, last_attempt_at: null },
  timer: { status: "idle", duration_seconds: null, started_at: null, last_error: null },
  plugins: {}, recent_events: [], recent_errors: [], last_successful_plan_reload_at: null, last_action: null,
};
