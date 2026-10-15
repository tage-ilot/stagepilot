import type {
  ApplicationState,
  ConnectionStatus,
  LightsStatusResponse,
  MidiInputsResponse,
  PlanningCenterStatusResponse,
  ProPresenterStatusResponse,
  SettingsResponse,
  PlaybackStatusResponse,
} from "../../types";
import { connectionWord } from "../ui/statusWord";
import { playbackConnectionDetail, playbackOrderNotice } from "../playbackStatus";

export type ConnectionCardView = {
  status: ConnectionStatus;
  detail: string;
  configured: boolean;
  mode: "real" | "simulated" | "demo" | "disabled";
  actionNotice?: string | null;
};

export type ReadinessCheck = {
  id: string;
  label: string;
  passed: boolean;
  required: boolean;
  severity: "info" | "warning" | "blocking";
  status: "connected" | "disconnected" | "error";
  detail?: string;
};

type DashboardIntegrationViews = {
  planningCenter: ConnectionCardView;
  midi: ConnectionCardView;
  propresenter: ConnectionCardView;
  lights: ConnectionCardView;
};

const statusDetail = (status: ConnectionStatus, detail: string | null | undefined) => {
  if (detail) return detail;
  if (status === "connecting") return "Connecting";
  if (status === "error") return "Connection failed";
  if (status === "connected") return "Connected";
  return "Disconnected";
};

export function buildConnectionCardViews({
  state,
  settings,
  planningCenterStatus,
  midi,
  playback,
  propresenter,
  lights,
  stateOnly = false,
}: {
  state: ApplicationState;
  stateOnly?: boolean;
  settings: SettingsResponse | null;
  planningCenterStatus?: PlanningCenterStatusResponse | null;
  midi: MidiInputsResponse | null;
  playback?: PlaybackStatusResponse | null;
  propresenter: ProPresenterStatusResponse | null;
  lights: LightsStatusResponse | null;
}): DashboardIntegrationViews {
  if (stateOnly) {
    // No configuration inference: Viewers see only the backend's live projection.
    const reported = (status: ConnectionStatus): ConnectionCardView => ({
      status, detail: statusDetail(status, null), configured: false, mode: "real",
    });
    return {
      planningCenter: reported(state.planning_center_status), midi: reported(state.midi_status),
      propresenter: reported(state.propresenter_status), lights: reported(state.lights_status),
    };
  }
  const modes = settings?.settings.integration_modes;
  const serviceIsDemo = modes?.service_source === "demo" || (!modes && Boolean(state.plugins.demo));
  const midiIsSimulated = modes?.midi_source === "simulated" || (!modes && Boolean(state.plugins.demo));
  const timerIsSimulated = modes?.timer_output === "simulated" || (!modes && Boolean(state.plugins.demo));

  const planningAuthConfigured = Boolean(
    planningCenterStatus?.connection_method === "oauth"
      ? planningCenterStatus.oauth_connected && !planningCenterStatus.oauth_needs_reconnect
      : (settings?.settings.planning_center.app_id
        && settings.planning_center_secret_saved),
  );
  const planningConfigured = Boolean(
    planningAuthConfigured
      && settings?.settings.planning_center.service_type_id,
  );
  const midiEnabled = settings?.settings.midi.enabled ?? midi?.enabled ?? false;
  const selectedMidi = midi?.selected_input_name ?? settings?.settings.midi.input_name ?? null;
  const connectedMidi = midi?.inputs.find((input) => input.connected);
  const propresenterEnabled = settings?.settings.propresenter.enabled ?? propresenter?.enabled ?? false;
  const lightingEnabled = settings?.settings.lights.enabled ?? lights?.enabled ?? false;
  const lightingMethod = lights?.method ?? settings?.settings.lights.method ?? "midi";
  const lightOutput = lights?.output_name ?? settings?.settings.lights.output_name ?? null;

  return {
    planningCenter: serviceIsDemo
      ? {
          status: "disconnected",
          detail: "Demo service plan loaded — configure Services",
          configured: false,
          mode: "demo",
        }
      : {
          status: planningConfigured ? state.planning_center_status : "disconnected",
          detail: planningConfigured
            ? statusDetail(
                state.planning_center_status,
                state.planning_center_status === "connected"
                  ? state.last_successful_plan_reload_at
                    ? `Last plan sync ${new Intl.DateTimeFormat(undefined, {
                        hour: "numeric",
                        minute: "2-digit",
                        second: "2-digit",
                      }).format(new Date(state.last_successful_plan_reload_at))}`
                    : "Services API connected"
                  : undefined,
              )
            : "Configure Services credentials",
          configured: planningConfigured,
          mode: planningConfigured ? "real" : "disabled",
        },
    midi: playback && !midiIsSimulated
      ? {
          status: playback.connected ? "connected" : "disconnected",
          detail: playbackConnectionDetail(playback),
          configured: playback.selected ? playback.enabled : Boolean(selectedMidi),
          mode: "real",
          actionNotice: playback.active_source === "playback_api" ? playbackOrderNotice(playback) : null,
        }
      : modes?.midi_source === "playback_api"
        ? { status: state.midi_status, detail: statusDetail(state.midi_status, null), configured: Boolean(settings?.settings.playback_api?.enabled), mode: "real" }
        : midiIsSimulated
      ? {
          status: "disconnected",
          detail: midiEnabled
            ? "MIDI simulation active — select a real input to connect"
            : "MIDI Playback disabled",
          configured: false,
          mode: "simulated",
        }
      : !midiEnabled
        ? {
            status: "disconnected",
            detail: "MIDI Playback disabled",
            configured: false,
            mode: "disabled",
          }
        : {
            status: connectedMidi
              ? state.midi_status
              : state.midi_status === "error" && selectedMidi
                ? "error"
                : "disconnected",
            detail: connectedMidi
              ? `Connected to ${connectedMidi.name}`
              : selectedMidi
                ? `Waiting for ${selectedMidi}`
                : midi && midi.inputs.length === 0
                  ? "No MIDI inputs found"
                  : "No MIDI input selected",
            configured: Boolean(selectedMidi),
            mode: "real",
          },
    propresenter: timerIsSimulated
      ? {
          status: "disconnected",
          detail: propresenterEnabled
            ? "Simulated timer active — configure Presentation to connect"
            : "Presentation disabled",
          configured: false,
          mode: "simulated",
        }
      : !propresenterEnabled
        ? {
            status: "disconnected",
            detail: "Presentation disabled",
            configured: false,
            mode: "disabled",
          }
        : {
            status: state.propresenter_status,
            detail: statusDetail(
              state.propresenter_status,
              propresenter?.detail ?? (propresenter?.timer_found ? "Timer ready" : "Timer not selected"),
            ),
            configured: true,
            mode: "real",
          },
    lights: !lightingEnabled
      ? {
          status: "disconnected",
          detail: lightingMethod === "osc" ? "Configure Lighting OSC" : "Configure a Lighting MIDI output",
          configured: false,
          mode: "disabled",
        }
      : {
          status: state.lights_status,
          detail: statusDetail(
            state.lights_status,
            lights?.detail ?? (lightingMethod === "osc" ? "OSC not sent yet" : lightOutput ? `Waiting for ${lightOutput}` : "No Lighting output selected"),
          ),
          configured: lightingMethod === "osc" || Boolean(lightOutput),
          mode: "real",
        },
  };
}

export function buildReadinessChecks({
  state,
  settings,
  propresenter,
  live,
  views,
  stateOnly = false,
}: {
  state: ApplicationState;
  stateOnly?: boolean;
  settings: SettingsResponse | null;
  propresenter: ProPresenterStatusResponse | null;
  live: boolean;
  views: DashboardIntegrationViews;
}): ReadinessCheck[] {
  if (stateOnly) {
    return [
      // Lighting is optional and has its own card; it is not part of the system check (owner, 2026-10-15).
      ...Object.entries(views).filter(([id]) => id !== "lights").map(([id, view]) => ({
        id, label: `${({ planningCenter: "Services", midi: "Playback", propresenter: "Presentation" } as Record<string, string>)[id]} ${connectionWord[view.status].toLowerCase()}`,
        passed: view.status === "connected", required: true, severity: "info" as const,
        status: view.status === "connecting" ? "disconnected" as const : view.status,
        detail: "Live status reported by StagePilot. Configuration is not available in this view.",
      })),
      { id: "backend", label: live ? "StagePilot connected" : "StagePilot disconnected",
        passed: live, required: true, severity: "info", status: live ? "connected" : "disconnected" },
    ];
  }
  const plan = state.plan;
  const productionPlan = settings?.settings.integration_modes.service_source === "planning_center";
  const servicePlanReady = Boolean(
    productionPlan
      && plan
      && plan.date === state.service_load.target_date
      && state.service_load.status === "loaded"
      && !state.service_load.is_stale,
  );
  const durationReady = Boolean(servicePlanReady && plan?.songs.length)
    && plan!.songs.every((song) => Boolean(song.duration_seconds));
  // The entry names the saved connection method: "Playback API connected" or "Playback MIDI disconnected".
  const playbackLabel = settings?.settings.integration_modes.midi_source === "playback_api" ? "Playback API" : "Playback MIDI";
  const checks: ReadinessCheck[] = [
    {
      id: "planning-center",
      label: views.planningCenter.status === "connected"
        ? "Services connected"
        : "Services disconnected",
      passed: views.planningCenter.status === "connected",
      required: true,
      severity: "blocking",
      status: views.planningCenter.status === "error" ? "error" : views.planningCenter.status === "connected" ? "connected" : "disconnected",
      detail: views.planningCenter.detail,
    },
    {
      id: "service-plan",
      label: servicePlanReady ? "Service plan loaded" : "Service plan not loaded",
      passed: servicePlanReady,
      required: true,
      severity: "blocking",
      status: servicePlanReady ? "connected" : "disconnected",
      detail: productionPlan ? undefined : "Demo service plan is available for testing.",
    },
    {
      id: "durations",
      label: durationReady ? "Song durations valid" : "Song durations invalid",
      passed: durationReady,
      required: true,
      severity: "blocking",
      status: durationReady
        ? "connected"
        : servicePlanReady && Boolean(plan?.songs.length)
          ? "error"
          : "disconnected",
    },
    {
      id: "midi",
      label: `${playbackLabel} ${views.midi.status === "connected" ? "connected" : "disconnected"}`,
      passed: views.midi.status === "connected",
      required: true,
      severity: "blocking",
      status: views.midi.status === "error" ? "error" : views.midi.status === "connected" ? "connected" : "disconnected",
      detail: views.midi.detail,
    },
    {
      id: "propresenter",
      label: views.propresenter.status === "connected"
        ? "Presentation connected"
        : "Presentation disconnected",
      passed: views.propresenter.status === "connected",
      required: true,
      severity: "blocking",
      status: views.propresenter.status === "error" ? "error" : views.propresenter.status === "connected" ? "connected" : "disconnected",
      detail: views.propresenter.detail,
    },
  ];
  if (views.propresenter.mode === "real" && views.propresenter.configured) {
    checks.push({
      id: "propresenter-timer",
      label: propresenter?.timer_found ? "Presentation timer found" : "Presentation timer not found",
      passed: Boolean(propresenter?.timer_found),
      required: true,
      severity: "blocking",
      status: propresenter?.timer_found
        ? "connected"
        : views.propresenter.status === "connected"
          ? "error"
          : "disconnected",
    });
  }
  if (views.midi.actionNotice) checks.push({
    id: "playback-order", label: views.midi.actionNotice, passed: false,
    required: true, severity: "blocking", status: "disconnected",
    detail: "Connection is healthy. Discover Song Order in the Playback panel before starting songs.",
  });
  checks.push({
    id: "backend",
    label: live ? "StagePilot connected" : "StagePilot disconnected",
    passed: live,
    required: true,
    severity: "blocking",
    status: live ? "connected" : "disconnected",
  });
  return checks;
}

export const readinessPassed = (checks: ReadinessCheck[]) =>
  checks.filter((check) => check.required).every((check) => check.passed);

// Only a blocking subsystem in an error state turns the pill red; optional/non-blocking ones never do.
export const readinessErrorCauses = (checks: ReadinessCheck[]) =>
  checks.filter((check) => check.required && check.severity === "blocking" && check.status === "error");

export const readinessHasError = (checks: ReadinessCheck[]) => readinessErrorCauses(checks).length > 0;
