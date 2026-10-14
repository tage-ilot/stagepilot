export type ApplicationStatus = "starting" | "running" | "stopping" | "error";
export type ConnectionStatus = "connecting" | "connected" | "disconnected" | "error";
export type PluginStatus = "stopped" | "starting" | "running" | "stopping" | "error";
export type TimerStatus = "idle" | "running" | "stopped" | "error";
export type ServiceLoadStatus =
  | "idle"
  | "loading"
  | "loaded"
  | "not_found"
  | "ambiguous"
  | "error";

export type ActionName =
  | "start_next"
  | "restart_current"
  | "previous"
  | "next"
  | "stop_timer"
  | "reload_plan"
  | "reset_position";

export type MidiCueName =
  | "start_next"
  | "restart_current"
  | "previous"
  | "next"
  | "reload_plan"
  | "stop_timer";

export type MidiMessageDisposition =
  | "dispatched"
  | "action_rejected"
  | "duplicate"
  | "unmapped"
  | "wrong_channel"
  | "note_release"
  | "queue_full"
  | "error";

export interface Song {
  id: string;
  title: string;
  duration_seconds: number | null;
  formatted_duration?: string | null;
  order: number;
  service_sequence?: number | null;
  is_generic: boolean;
  source_song_id: string | null;
  description?: string | null;
}

export interface ServicePlan {
  id: string;
  title: string;
  date: string;
  service_type: string;
  service_type_id: string | null;
  service_times: string[];
  duration_source: string;
  songs: Song[];
}

export interface ServicePlanCandidate {
  id: string;
  title: string;
  service_type_id: string;
  service_type_name: string;
  target_date: string;
  service_times: string[];
}

export interface SkippedServiceItem {
  item_id: string;
  title: string;
  description?: string | null;
  item_type: string;
  sequence: number;
  duration_seconds?: number | null;
  reason: string;
}

export interface ServiceLoadState {
  status: ServiceLoadStatus;
  target_date: string | null;
  candidates: ServicePlanCandidate[];
  skipped_items: SkippedServiceItem[];
  message: string | null;
  is_stale: boolean;
  last_attempt_at: string | null;
}

export interface PluginHealth {
  name: string;
  version: string;
  status: PluginStatus;
  last_error: string | null;
  last_activity_at: string | null;
}

export interface EventSummary {
  id: string;
  type: string;
  timestamp: string;
  source: string;
}

export interface ErrorSummary {
  timestamp: string;
  component: string;
  message: string;
  event_id: string | null;
}

export interface ApplicationState {
  revision: number;
  updated_at: string;
  application_status: ApplicationStatus;
  plan: ServicePlan | null;
  /** The navigation cursor (Previous / Next move it without starting anything). */
  current_song: Song | null;
  next_song: Song | null;
  current_song_index: number | null;
  /** What the screen says: the song actually started, and the song the next Start next will play. */
  playing_song: Song | null;
  playing_song_index: number | null;
  up_next_song: Song | null;
  planning_center_status: ConnectionStatus;
  midi_status: ConnectionStatus;
  propresenter_status: ConnectionStatus;
  lights_status: ConnectionStatus;
  service_load: ServiceLoadState;
  timer: {
    status: TimerStatus;
    duration_seconds: number | null;
    started_at: string | null;
    position_seconds?: number | null;
    remaining_seconds?: number | null;
    last_error: string | null;
  };
  plugins: Record<string, PluginHealth>;
  recent_events: EventSummary[];
  recent_errors: ErrorSummary[];
  last_successful_plan_reload_at: string | null;
  last_action: string | null;
}

export interface HealthResponse {
  status: "healthy" | "degraded";
  version: string;
  application_status: ApplicationStatus;
  plugins: PluginHealth[];
  /** When this backend process started (UTC ISO). Older backends omit it. */
  started_at?: string | null;
}

export type ServiceSource = "demo" | "planning_center";
export type MidiSource = "simulated" | "real" | "playback_api";
export type TimerOutput = "simulated" | "propresenter";

export interface IntegrationModes {
  service_source: ServiceSource;
  midi_source: MidiSource;
  timer_output: TimerOutput;
}

export interface PlanningCenterPublicSettings {
  app_id: string | null;
  service_type_id: string | null;
  plan_title_preference: string | null;
  preferred_service_time: string | null;
  upcoming_lookahead_days: number;
  request_timeout_seconds: number;
}

export interface PersistentSettings {
  schema_version: 1 | 2;
  onboarding: {
    general_completed: boolean;
  };
  integration_modes: IntegrationModes;
  timezone: string;
  log_level: "DEBUG" | "INFO" | "WARNING" | "ERROR" | "CRITICAL";
  server_port: number;
  lan_access?: boolean;
  web_dashboard_pin_enabled?: boolean;
  release_channel?: "STABLE" | "BETA";
  planning_center: PlanningCenterPublicSettings;
  midi: {
    enabled: boolean;
    input_name: string | null;
    channel: number;
    note: number;
    mappings: Partial<Record<MidiCueName, number | null>>;
    debounce_ms: number;
  };
  playback_api?: PlaybackApiSettings;
  lights: LightsSettings;
  propresenter: {
    enabled: boolean;
    host: string;
    port: number;
    timer_name: string;
    look_id?: string | null;
    request_timeout_seconds: number;
    reconnect_initial_seconds: number;
    reconnect_max_seconds: number;
    health_check_interval_seconds: number;
  };
}

export interface LightingCue {
  id: string;
  at_seconds: number;
  note: number;
  velocity: number;
  label: string;
}

export interface SongLightingCueMap {
  song_key: string;
  song_title: string;
  cues: LightingCue[];
}

export interface LightsSettings {
  enabled: boolean;
  output_name: string | null;
  channel: number;
  pulse_ms: number;
  cue_maps: Record<string, SongLightingCueMap>;
}

export interface LightingOutput {
  name: string;
  ambiguous: boolean;
  selected: boolean;
  connected: boolean;
}

export interface LightsStatusResponse {
  enabled: boolean;
  output_name: string | null;
  channel: number;
  pulse_ms: number;
  connection_status: ConnectionStatus;
  detail: string | null;
  outputs: LightingOutput[];
  last_cue: LightingCue | null;
  last_cue_at: string | null;
}

export type LightsSettingsInput = Pick<
  LightsSettings,
  "enabled" | "output_name" | "channel" | "pulse_ms"
>;

export interface LightsOperationResponse {
  accepted: boolean;
  message: string;
  lights: LightsStatusResponse;
}

export interface SettingsResponse {
  settings: PersistentSettings;
  planning_center_secret_saved: boolean;
  warning: string | null;
  restart_required: boolean;
}

export type GeneralSettingsInput = Pick<
  PersistentSettings,
  "timezone" | "log_level" | "server_port" | "lan_access" | "web_dashboard_pin_enabled" | "release_channel"
> & {
  web_dashboard_pin?: string;
};

export interface DashboardAuthStatus {
  required: boolean;
  authenticated: boolean;
}

export type MidiSettingsInput = PersistentSettings["midi"];

export interface PlaybackSettingsInput {
  enabled: boolean;
  host: string | null;
  port: number;
  auto_scan: boolean;
  fast_transport: boolean;
}

export interface LengthRow { item_id: string; title: string; old_length: number | null; new_length: number | null; status: "updated" | "restored" | "skipped" | "failed"; reason: string | null }
export interface LengthPreview { token: string; category: string; plan_title: string; plan_date: string; items: LengthRow[]; message: string }

export interface PlaybackApiSettings extends PlaybackSettingsInput {
  discovery_duration_seconds?: number | null;
  planning_center_update_service_type_id?: string | null;
  song_order: number[];
  song_lengths?: (number | null)[];
  lengths_measured_at?: string | null;
  captured_version: number | null;
  setlist_id?: number | string | null;
  captured_at: string | null;
}

export interface PlaybackConnection {
  connected: boolean;
  active_source: "playback_api" | "midi" | "none";
  reason: string;
  sources: Record<"playback_api" | "midi", { connected: boolean; reason: string }>;
}

export interface PlaybackScanResult {
  state: "idle" | "scanning" | "found" | "not_found";
  candidates: { name: string; host: string }[];
  reason: string | null;
  current: number;
  total: number;
  phase?: string | null;
  error_class?: "permission_denied" | "no_route" | "refused" | "timed_out" | "not_playback" | "invalid_address" | "unexpected_error" | null;
  typed?: boolean;
  elapsed?: number;
  networks?: string[];
  hosts_probed?: number;
  hosts_total?: number;
  details?: string | null;
  settings_url?: string | null;
}

export interface PlaybackStatusResponse extends PlaybackConnection {
  planning_center_connected?: boolean;
  planning_center_undo_available?: boolean;
  planning_center_update_service_type_id?: string | null;
  planning_center_update_reason?: string | null;
  discovery_duration_seconds?: number | null;
  planning_center_lengths?: { status: string; message: string; items: LengthRow[]; reload: string };
  scan: PlaybackScanResult;
  selected: boolean;
  enabled: boolean;
  host: string | null;
  port: number;
  source: "manual" | "loopback" | "lan" | null;
  last_error: string | null;
  playing: boolean;
  setlist_cloud_version: number | null;
  discovery: "idle" | "running" | "failed" | "done";
  progress: number;
  discovery_song?: number;
  discovery_total?: number;
  song_lengths?: (number | null)[];
  lengths_measured_at?: string | null;
  plan_songs?: { id: string; title: string; duration_seconds: number | null }[];
  /** Songs left out of the pairing. Order never changes; see playbackArrangement.ts. */
  skipped_song_ids?: number[];
  skipped_plan_song_ids?: string[];
  /** False when a saved skip names a plan song the loaded plan no longer has. */
  arrangement_current?: boolean;
  song_count: number;
  plan_song_count: number;
  song_order: number[];
  captured_version: number | null;
  captured_at: string | null;
  stale: boolean;
  plan_changed?: boolean;
  lengths_applied?: boolean;
  settings: PlaybackSettingsInput;
}

export interface PlaybackMonitorEntry {
  event: {
    type: string;
    timestamp: number;
    song_id: number | null;
    position: number | null;
    previous_song_id: number | null;
    continues_playing: boolean;
    provisional?: boolean;
    playing: boolean | null;
    pad: boolean | null;
    setlist_version: number | null;
    previous_version: number | null;
    section_id: number | null;
    active: boolean | null;
    reason: string | null;
    message_kind: string | null;
  };
  discovery: boolean;
  at?: string;
}

export interface PlaybackEventsResponse {
  events: PlaybackMonitorEntry[];
  capacity: number;
}

export interface PlanningCenterStatusResponse {
  connection_status: ConnectionStatus;
  configured: boolean;
  app_id: string | null;
  service_type_id: string | null;
  planning_center_secret_saved: boolean;
  connection_method: "oauth" | "manual";
  oauth_connected: boolean;
  oauth_needs_reconnect: boolean;
  detail: string | null;
}

export interface PlanningCenterOAuthStartResponse {
  authorize_url: string;
  state: string;
}

export interface PlanningCenterOAuthStatusResponse {
  connection_method: "oauth" | "manual";
  connected: boolean;
  needs_reconnect: boolean;
  expires_at: number | null;
  scope?: string | null;
}

export interface PlanningCenterServiceType {
  id: string;
  name: string;
}

export interface PlanningCenterTestInput {
  app_id?: string;
  secret?: string;
}

export interface PlanningCenterTestResponse {
  authenticated: true;
  message: string;
  service_types: PlanningCenterServiceType[];
}

export interface PlanningCenterSettingsInput extends PlanningCenterPublicSettings {
  secret?: string;
  remove_secret?: boolean;
}

export interface StateEnvelope {
  type: "state.snapshot";
  data: ApplicationState;
}

export interface ActionResponse {
  action: ActionName;
  accepted: boolean;
  message: string;
  state: ApplicationState;
}

export interface PlanSelectionResponse {
  accepted: boolean;
  message: string;
  state: ApplicationState;
}

export interface MidiInput {
  id: string;
  name: string;
  ambiguous: boolean;
  selected: boolean;
  connected: boolean;
}

export interface MidiInputsResponse {
  enabled: boolean;
  channel: number;
  note: number;
  configured_input_name: string | null;
  selected_input_name: string | null;
  inputs: MidiInput[];
  mappings: Partial<Record<MidiCueName, number>>;
}

export interface MidiMonitorMessage {
  timestamp: string;
  input_name: string | null;
  message_type: "note_on" | "note_off";
  channel: number;
  note: number;
  note_name: string;
  velocity: number;
  disposition: MidiMessageDisposition;
  detail: string;
  action: ActionName | null;
  simulated: boolean;
}

export interface MidiMonitorResponse {
  messages: MidiMonitorMessage[];
}

export interface MidiInputSelectionResponse {
  accepted: boolean;
  message: string;
  midi: MidiInputsResponse;
}

export interface MidiCueSimulationResponse {
  cue: MidiCueName;
  action: ActionName;
  accepted: boolean;
  message: string;
  state: ApplicationState;
}

export interface ProPresenterTimer {
  id: string;
  name: string;
  index: number;
  is_countdown: boolean;
  state: string | null;
}

export interface ProPresenterLook {
  id: string;
  name: string;
  index: number;
}

export interface ProPresenterStatusResponse {
  enabled: boolean;
  host: string;
  port: number;
  timer_name: string;
  look_id: string | null;
  request_timeout_seconds: number;
  connection_status: ConnectionStatus;
  detail: string | null;
  timers: ProPresenterTimer[];
  selected_timer_id: string | null;
  timer_found: boolean;
  looks: ProPresenterLook[];
  current_look_id: string | null;
  look_found: boolean;
  last_checked_at: string | null;
}

export interface ProPresenterSettingsInput {
  host: string;
  port: number;
  timer_name: string;
  look_id?: string | null;
  request_timeout_seconds: number;
}

export interface ProPresenterOperationResponse {
  accepted: boolean;
  message: string;
  propresenter: ProPresenterStatusResponse;
}

/** UI affordances supplied by the backend; never a substitute for server authorization. */
export interface AccessCapabilities {
  canRead: boolean;
  canOperate: boolean;
  canConfigure: boolean;
  canActivateServices: boolean;
}

export interface DashboardAccess {
  mode: "desktop" | "lan" | "remote";
  authentication: "none" | "pin" | "password";
  authenticated: boolean;
  capabilities: AccessCapabilities;
  user: { email: string; role: "Viewer" | "Operator" } | null;
  expires_at: number | null;
  csrf_token: string | null;
}
