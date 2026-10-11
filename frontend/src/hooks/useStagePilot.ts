import { useCallback, useEffect, useRef, useState } from "react";

import {
  completePlanningCenterOAuth,
  disconnectPlanningCenterOAuth,
  getAccess,
  getHealth,
  getLightsStatus,
  getMidiInputs,
  getMidiMessages,
  getPlanningCenterServiceTypes,
  getPlanningCenterStatus,
  getProPresenterStatus,
  getSettings,
  getState,
  rememberServerPort,
  performAction,
  refreshMidiInputs,
  refreshLightingOutputs,
  refreshProPresenterTimers,
  selectMidiInput,
  selectPlanningCenterPlan,
  simulateMidiCue,
  startPlanningCenterOAuth,
  testLightingCue,
  testPlanningCenter,
  testProPresenter,
  updatePlanningCenterSettings,
  updateLightingCueMap,
  updateLightsSettings,
  updateProPresenterSettings,
  updateSettings,
  updateDashboardAccess,
  websocketUrl,
} from "../api";
import type {
  ActionName,
  ApplicationState,
  ConnectionStatus,
  HealthResponse,
  LightingCue,
  LightsSettingsInput,
  LightsStatusResponse,
  GeneralSettingsInput,
  MidiCueName,
  MidiInputsResponse,
  MidiMonitorMessage,
  MidiSettingsInput,
  PlanningCenterServiceType,
  PlanningCenterSettingsInput,
  PlanningCenterStatusResponse,
  PlanningCenterTestInput,
  ProPresenterSettingsInput,
  ProPresenterStatusResponse,
  SettingsResponse,
  Song,
  SongLightingCueMap,
  StateEnvelope,
} from "../types";
import { restartDesktopBackend, signInWithPlanningCenter } from "../desktop";
import { describeError } from "../errorText";
import { usePlaybackInput } from "./usePlaybackInput";

import { useDashboardAccess } from "../access/AccessContext";
import { accessGeneration, invalidateAccess, onAccessInvalidated } from "../access/accessState";

const MAX_RECONNECT_DELAY = 10_000;
const MIDI_MONITOR_INTERVAL = 750;
const PROPRESENTER_MONITOR_INTERVAL = 3_000;
const LIGHTS_MONITOR_INTERVAL = 3_000;

export function useStagePilot() {
  const access = useDashboardAccess();
  const { canOperate, canConfigure, canActivateServices } = access.capabilities;
  const [state, setState] = useState<ApplicationState | null>(null);
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [live, setLive] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [pendingAction, setPendingAction] = useState<ActionName | null>(null);
  const [pendingPlanId, setPendingPlanId] = useState<string | null>(null);

  const [settings, setSettings] = useState<SettingsResponse | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [settingsMessage, setSettingsMessage] = useState<string | null>(null);
  const [pendingSettingsOperation, setPendingSettingsOperation] = useState(false);
  const playback = usePlaybackInput(canConfigure, setSettings);

  const [planningCenterStatus, setPlanningCenterStatus] =
    useState<PlanningCenterStatusResponse | null>(null);
  const [planningCenterServiceTypes, setPlanningCenterServiceTypes] = useState<
    PlanningCenterServiceType[]
  >([]);
  const [planningCenterError, setPlanningCenterError] = useState<string | null>(null);
  const [planningCenterMessage, setPlanningCenterMessage] = useState<string | null>(null);
  const [pendingPlanningCenterOperation, setPendingPlanningCenterOperation] = useState<
    "test" | "load-types" | "save" | "oauth-sign-in" | "oauth-disconnect" | null
  >(null);

  const [midi, setMidi] = useState<MidiInputsResponse | null>(null);
  const [midiMessages, setMidiMessages] = useState<MidiMonitorMessage[]>([]);
  const [midiError, setMidiError] = useState<string | null>(null);
  const [midiMessage, setMidiMessage] = useState<string | null>(null);
  const [pendingMidiOperation, setPendingMidiOperation] = useState<
    "refresh" | "connect" | "disconnect" | null
  >(null);
  const [pendingMidiCue, setPendingMidiCue] = useState<MidiCueName | null>(null);

  const [propresenter, setProPresenter] = useState<ProPresenterStatusResponse | null>(null);
  const [propresenterError, setProPresenterError] = useState<string | null>(null);
  const [propresenterMessage, setProPresenterMessage] = useState<string | null>(null);
  const [pendingProPresenterOperation, setPendingProPresenterOperation] = useState<
    "save" | "test" | "refresh" | null
  >(null);

  const [lights, setLights] = useState<LightsStatusResponse | null>(null);
  const [lightsError, setLightsError] = useState<string | null>(null);
  const [lightsMessage, setLightsMessage] = useState<string | null>(null);
  const [pendingLightsOperation, setPendingLightsOperation] = useState<
    "save" | "refresh" | "test" | "save-cues" | null
  >(null);

  const reconnectAttempts = useRef(0);
  const liveConnectionEstablished = useRef(false);
  const previousMidiStatus = useRef<ConnectionStatus | null>(null);
  const previousProPresenterStatus = useRef<ConnectionStatus | null>(null);
  const previousPlanningCenterStatus = useRef<ConnectionStatus | null>(null);

  const applyState = useCallback((nextState: ApplicationState) => {
    setState((currentState) =>
      currentState === null || nextState.revision >= currentState.revision
        ? nextState
        : currentState,
    );
  }, []);

  const loadMidiInputs = useCallback(async () => {
    if (!canConfigure) return;
    try {
      const response = await getMidiInputs();
      setMidi(response);
      setMidiError(null);
    } catch (cause) {
      setMidiError(describeError(cause, "MIDI inputs unavailable."));
    }
  }, [canConfigure]);

  const loadMidiMessages = useCallback(async () => {
    if (!canConfigure) return;
    try {
      setMidiMessages((await getMidiMessages()).messages);
    } catch {
      // Input selection and cue actions remain usable if the monitor refresh fails.
    }
  }, [canConfigure]);

  const loadProPresenter = useCallback(async () => {
    if (!canConfigure) return;
    try {
      setProPresenter(await getProPresenterStatus());
      setProPresenterError(null);
    } catch (cause) {
      setProPresenterError(
        describeError(cause, "Presentation status unavailable."),
      );
    }
  }, [canConfigure]);

  const loadSettings = useCallback(async () => {
    if (!canConfigure) return;
    try {
      setSettings(await getSettings());
      setSettingsError(null);
    } catch (cause) {
      setSettingsError(describeError(cause, "Configuration unavailable."));
    }
  }, [canConfigure]);

  const loadLights = useCallback(async () => {
    if (!canConfigure) return;
    try {
      setLights(await getLightsStatus());
      setLightsError(null);
    } catch (cause) {
      setLightsError(describeError(cause, "Lights output unavailable."));
    }
  }, [canConfigure]);

  const loadPlanningCenterStatus = useCallback(async () => {
    if (!canConfigure) return;
    try {
      setPlanningCenterStatus(await getPlanningCenterStatus());
      setPlanningCenterError(null);
    } catch (cause) {
      setPlanningCenterError(
        describeError(cause, "Services status unavailable."),
      );
    }
  }, [canConfigure]);

  useEffect(() => {
    let active = true;
    let socket: WebSocket | null = null;
    let reconnectTimer: number | undefined;

    const refresh = async () => {
      try {
        const [nextHealth, nextState] = await Promise.all([getHealth(), getState()]);
        if (!active) return;
        setHealth(nextHealth);
        applyState(nextState);
        setError(null);
        // On packaged macOS builds the first HTTP request can race the sidecar
        // becoming ready and WebKit reports that as "Load failed.".  Load
        // settings only after the health/state probe has proved the backend is
        // reachable so a stale startup error is not left in the setup panel.
        await loadSettings();
      } catch (cause) {
        if (active) {
          setError(describeError(cause, "Backend unavailable."));
        }
      }
    };

    const stopForAuth = () => {
      active = false;
      if (reconnectTimer !== undefined) window.clearTimeout(reconnectTimer);
      socket?.close();
      setLive(false);
      setState(null);
      setSettings(null);
      setMidi(null);
      setMidiMessages([]);
      setPlanningCenterStatus(null);
      setPlanningCenterServiceTypes([]);
      setProPresenter(null);
      setLights(null);
    };
    const unsubscribeAuth = onAccessInvalidated(stopForAuth);

    const connect = () => {
      if (!active) return;
      socket = new WebSocket(websocketUrl);
      socket.onopen = () => {
        if (!active) return;
        reconnectAttempts.current = 0;
        liveConnectionEstablished.current = true;
        setLive(true);
        setError(null);
        void refresh();
      };
      socket.onmessage = (message) => {
        if (!active) return;
        try {
          const envelope = JSON.parse(String(message.data)) as StateEnvelope;
          if (envelope.type === "state.snapshot") applyState(envelope.data);
        } catch {
          setError("Received an invalid live-state message.");
        }
      };
      socket.onerror = () => socket?.close();
      socket.onclose = async (event) => {
        if (!active) return;
        setLive(false);
        if (event?.code === 4401 || event?.code === 4403) {
          invalidateAccess();
          return;
        }
        setError(
          liveConnectionEstablished.current
            ? "Live connection interrupted; reconnecting."
            : access.mode === "remote" ? "Waiting for remote StagePilot." : "Waiting for local StagePilot.",
        );
        // Browsers report an HTTP-rejected WS handshake as opaque code 1006.
        // Ask the server before reconnecting; network failures still use backoff.
        if (event?.code === 1006) {
          const generation = accessGeneration();
          try {
            const nextAccess = await getAccess();
            if (!active) return;
            if (!nextAccess.authenticated || !nextAccess.capabilities.canRead
              || nextAccess.capabilities.canConfigure !== canConfigure
              || nextAccess.capabilities.canOperate !== canOperate) {
              invalidateAccess(generation);
              return;
            }
          } catch {
            // Transport/storage unavailability is not evidence of a logged-out session.
          }
        }
        if (!active) return;
        const delay = Math.min(
          1000 * 2 ** reconnectAttempts.current,
          MAX_RECONNECT_DELAY,
        );
        reconnectAttempts.current += 1;
        reconnectTimer = window.setTimeout(connect, delay);
      };
    };

    void refresh();
    void loadMidiInputs();
    void loadMidiMessages();
    void loadProPresenter();
    void loadLights();
    void loadPlanningCenterStatus();
    connect();

    return () => {
      active = false;
      unsubscribeAuth();
      if (reconnectTimer !== undefined) window.clearTimeout(reconnectTimer);
      socket?.close();
    };
  }, [
    access.mode,
    canConfigure,
    canOperate,
    applyState,
    loadMidiInputs,
    loadMidiMessages,
    loadPlanningCenterStatus,
    loadProPresenter,
    loadLights,
    loadSettings,
  ]);

  useEffect(() => {
    if (!midi?.enabled) return;
    const timer = window.setInterval(() => {
      void loadMidiMessages();
    }, MIDI_MONITOR_INTERVAL);
    return () => window.clearInterval(timer);
  }, [loadMidiMessages, midi?.enabled]);

  useEffect(() => {
    if (!propresenter?.enabled) return;
    const timer = window.setInterval(() => {
      void loadProPresenter();
    }, PROPRESENTER_MONITOR_INTERVAL);
    return () => window.clearInterval(timer);
  }, [loadProPresenter, propresenter?.enabled]);

  useEffect(() => {
    if (!lights?.enabled) return;
    const timer = window.setInterval(() => {
      void loadLights();
    }, LIGHTS_MONITOR_INTERVAL);
    return () => window.clearInterval(timer);
  }, [lights?.enabled, loadLights]);

  useEffect(() => {
    const status = state?.midi_status;
    if (!status) return;
    if (previousMidiStatus.current === null) {
      previousMidiStatus.current = status;
      return;
    }
    if (previousMidiStatus.current === status) return;
    previousMidiStatus.current = status;
    void loadMidiInputs();
  }, [loadMidiInputs, state?.midi_status]);

  useEffect(() => {
    const status = state?.propresenter_status;
    if (!status) return;
    if (previousProPresenterStatus.current === null) {
      previousProPresenterStatus.current = status;
      return;
    }
    if (previousProPresenterStatus.current === status) return;
    previousProPresenterStatus.current = status;
    void loadProPresenter();
  }, [loadProPresenter, state?.propresenter_status]);

  useEffect(() => {
    const status = state?.planning_center_status;
    if (!status) return;
    if (previousPlanningCenterStatus.current === null) {
      previousPlanningCenterStatus.current = status;
      return;
    }
    if (previousPlanningCenterStatus.current === status) return;
    previousPlanningCenterStatus.current = status;
    void loadPlanningCenterStatus();
  }, [loadPlanningCenterStatus, state?.planning_center_status]);

  // Retry for a failed panel load: every panel reads its own error, so reload them all and recover without a page reload.
  const retryLoads = useCallback(() => {
    void loadSettings();
    void loadMidiInputs();
    void loadProPresenter();
    void loadLights();
    void loadPlanningCenterStatus();
  }, [loadSettings, loadMidiInputs, loadProPresenter, loadLights, loadPlanningCenterStatus]);

  const dispatch = useCallback(
    async (action: ActionName) => {
    if (!canOperate) return;
      setPendingAction(action);
      setActionMessage(null);
      try {
        const response = await performAction(action);
        applyState(response.state);
        setActionMessage(response.message);
        if (!response.accepted) setError(response.message);
        else setError(null);
      } catch (cause) {
        setError(describeError(cause, "Action failed."));
      } finally {
        setPendingAction(null);
      }
    },
    [canOperate, applyState],
  );

  const selectPlan = useCallback(
    async (planId: string) => {
    if (!canOperate) return;
      setPendingPlanId(planId);
      setActionMessage(null);
      try {
        const response = await selectPlanningCenterPlan(planId);
        applyState(response.state);
        setActionMessage(response.message);
        setError(response.accepted ? null : response.message);
      } catch (cause) {
        setError(describeError(cause, "Service plan selection failed."));
      } finally {
        setPendingPlanId(null);
      }
    },
    [canOperate, applyState],
  );

  const saveGeneralSettings = useCallback(
    async (input: GeneralSettingsInput) => {
    if (!canConfigure) return;
      if (!settings) return;
      setPendingSettingsOperation(true);
      setSettingsError(null);
      setSettingsMessage(null);
      try {
        const {
          web_dashboard_pin,
          web_dashboard_pin_enabled,
          ...generalSettings
        } = input;
        const response = await updateSettings({
          ...settings.settings,
          ...generalSettings,
          ...(web_dashboard_pin_enabled !== undefined
            ? { web_dashboard_pin_enabled }
            : {}),
          onboarding: { general_completed: true },
        });
        const accessResponse = await updateDashboardAccess(
          web_dashboard_pin_enabled
            ?? settings.settings.web_dashboard_pin_enabled
            ?? true,
          web_dashboard_pin,
        );
        setSettings(accessResponse);
        rememberServerPort(response.settings.server_port);
        setSettingsMessage(
          "General configuration saved. Restart StagePilot and reload the dashboard to apply startup changes.",
        );
      } catch (cause) {
        setSettingsError(describeError(cause, "Configuration update failed."));
      } finally {
        setPendingSettingsOperation(false);
      }
    },
    [canConfigure, settings],
  );

  const saveMidiSettings = useCallback(
    async (input: MidiSettingsInput) => {
    if (!canConfigure) return false;
      if (!settings) return false;
      setPendingSettingsOperation(true);
      setSettingsError(null);
      setSettingsMessage(null);
      try {
        const response = await updateSettings({
          ...settings.settings,
          integration_modes: {
            ...settings.settings.integration_modes,
            midi_source: "real",
          },
          midi: { ...input, enabled: true },
        });
        setSettings(response);
        if (response.restart_required && await restartDesktopBackend()) {
          window.location.reload();
          return true;
        }
        setSettingsMessage(
          response.restart_required
            ? "MIDI configuration saved. Restart StagePilot to apply the hardware input configuration."
            : "MIDI configuration saved and applied to the running Playback input.",
        );
        return true;
      } catch (cause) {
        setSettingsError(describeError(cause, "MIDI configuration update failed."));
        return false;
      } finally {
        setPendingSettingsOperation(false);
      }
    },
    [canConfigure, settings],
  );

  const testPlanningCenterConnection = useCallback(
    async (input: PlanningCenterTestInput) => {
    if (!canConfigure) return;
      setPendingPlanningCenterOperation("test");
      setPlanningCenterError(null);
      setPlanningCenterMessage(null);
      try {
        const response = await testPlanningCenter(input);
        setPlanningCenterServiceTypes(response.service_types);
        setPlanningCenterMessage(response.message);
      } catch (cause) {
        setPlanningCenterError(
          describeError(cause, "Services connection test failed."),
        );
      } finally {
        setPendingPlanningCenterOperation(null);
      }
    },
    [canConfigure],
  );

  const loadPlanningCenterServiceTypes = useCallback(async () => {
    if (!canConfigure) return;
    setPendingPlanningCenterOperation("load-types");
    setPlanningCenterError(null);
    setPlanningCenterMessage(null);
    try {
      const serviceTypes = await getPlanningCenterServiceTypes();
      setPlanningCenterServiceTypes(serviceTypes);
      setPlanningCenterMessage(`Loaded ${serviceTypes.length} service types.`);
    } catch (cause) {
      setPlanningCenterError(
        describeError(cause, "Service types could not be loaded."),
      );
    } finally {
      setPendingPlanningCenterOperation(null);
    }
  }, [canConfigure]);

  const savePlanningCenter = useCallback(
    async (
      input: PlanningCenterSettingsInput,
      timezone: string,
    ) => {
    if (!canConfigure) return;
      setPendingPlanningCenterOperation("save");
      setPlanningCenterError(null);
      setPlanningCenterMessage(null);
      try {
        const planningCenterResponse = await updatePlanningCenterSettings(input);
        const response = await updateSettings({
          ...planningCenterResponse.settings,
          timezone,
          integration_modes: {
            ...planningCenterResponse.settings.integration_modes,
            service_source: "planning_center",
          },
        });
        setSettings(response);
        if (response.restart_required && await restartDesktopBackend()) {
          window.location.reload();
          return;
        }
        setPlanningCenterStatus(await getPlanningCenterStatus());
        if (response.restart_required) {
          if (response.warning) {
            setPlanningCenterError(response.warning);
          }
          setPlanningCenterMessage(
            "Services configuration saved securely. Restart StagePilot to apply the service source.",
          );
        } else {
          setPlanningCenterMessage(
            "Services configuration saved securely and applied to the running service source.",
          );
        }
      } catch (cause) {
        setPlanningCenterError(
          describeError(cause, "Services configuration could not be saved."),
        );
      } finally {
        setPendingPlanningCenterOperation(null);
      }
    },
    [canConfigure],
  );

  const signInPlanningCenterOAuth = useCallback(async () => {
    if (!canConfigure) return;
    setPendingPlanningCenterOperation("oauth-sign-in");
    setPlanningCenterError(null);
    setPlanningCenterMessage(null);
    try {
      const { authorize_url: authorizeUrlPrefix, state } = await startPlanningCenterOAuth();
      const { code, redirect_uri: redirectUri } = await signInWithPlanningCenter(
        authorizeUrlPrefix,
        state,
      );
      await completePlanningCenterOAuth(state, code, redirectUri);
      setPlanningCenterStatus(await getPlanningCenterStatus());
      setPlanningCenterMessage("Connected to Planning Center.");
    } catch (cause) {
      setPlanningCenterError(
        describeError(cause, "Planning Center sign-in failed."),
      );
    } finally {
      setPendingPlanningCenterOperation(null);
    }
  }, [canConfigure]);

  const disconnectPlanningCenter = useCallback(async () => {
    if (!canConfigure) return;
    setPendingPlanningCenterOperation("oauth-disconnect");
    setPlanningCenterError(null);
    setPlanningCenterMessage(null);
    try {
      await disconnectPlanningCenterOAuth();
      setPlanningCenterStatus(await getPlanningCenterStatus());
      setPlanningCenterMessage("Disconnected from Planning Center.");
    } catch (cause) {
      setPlanningCenterError(
        describeError(cause, "Could not disconnect from Planning Center."),
      );
    } finally {
      setPendingPlanningCenterOperation(null);
    }
  }, [canConfigure]);

  const refreshMidi = useCallback(async () => {
    if (!canConfigure) return;
    setPendingMidiOperation("refresh");
    setMidiError(null);
    setMidiMessage(null);
    try {
      setMidi(await refreshMidiInputs());
      setMidiMessage("MIDI input list refreshed.");
    } catch (cause) {
      setMidiError(describeError(cause, "MIDI refresh failed."));
    } finally {
      setPendingMidiOperation(null);
    }
  }, [canConfigure]);

  const selectMidi = useCallback(async (inputId: string | null) => {
    if (!canConfigure) return;
    setPendingMidiOperation(inputId === null ? "disconnect" : "connect");
    setMidiError(null);
    setMidiMessage(null);
    try {
      const response = await selectMidiInput(inputId);
      setMidi(response.midi);
      setMidiMessage(response.message);
      if (!response.accepted) setMidiError(response.message);
    } catch (cause) {
      setMidiError(describeError(cause, "MIDI input selection failed."));
    } finally {
      setPendingMidiOperation(null);
    }
  }, [canConfigure]);

  const simulateMidi = useCallback(
    async (cue: MidiCueName) => {
    if (!canOperate) return;
      setPendingMidiCue(cue);
      setMidiError(null);
      setMidiMessage(null);
      try {
        const response = await simulateMidiCue(cue);
        applyState(response.state);
        await loadMidiMessages();
        setMidiMessage(response.message);
        if (!response.accepted) setMidiError(response.message);
      } catch (cause) {
        setMidiError(describeError(cause, "MIDI cue simulation failed."));
      } finally {
        setPendingMidiCue(null);
      }
    },
    [canOperate, applyState, loadMidiMessages],
  );

  const saveProPresenter = useCallback(async (input: ProPresenterSettingsInput) => {
    if (!canConfigure) return;
    if (!settings) return;
    setPendingProPresenterOperation("save");
    setProPresenterError(null);
    setProPresenterMessage(null);
    try {
      const activated = await updateSettings({
        ...settings.settings,
        integration_modes: {
          ...settings.settings.integration_modes,
          timer_output: "propresenter",
        },
        propresenter: {
          ...settings.settings.propresenter,
          ...input,
          enabled: true,
        },
      });
      setSettings(activated);
      const response = await updateProPresenterSettings(input);
      setProPresenter(response.propresenter);
      setProPresenterMessage(response.message);
      if (!response.accepted) setProPresenterError(response.message);
    } catch (cause) {
      setProPresenterError(
        describeError(cause, "Presentation configuration update failed."),
      );
    } finally {
      setPendingProPresenterOperation(null);
    }
  }, [canConfigure, settings]);

  const runProPresenterTest = useCallback(async () => {
    if (!canConfigure) return;
    setPendingProPresenterOperation("test");
    setProPresenterError(null);
    setProPresenterMessage(null);
    try {
      const response = await testProPresenter();
      setProPresenter(response.propresenter);
      setProPresenterMessage(response.message);
      if (!response.accepted) setProPresenterError(response.message);
    } catch (cause) {
      setProPresenterError(
        describeError(cause, "Presentation connection test failed."),
      );
    } finally {
      setPendingProPresenterOperation(null);
    }
  }, [canConfigure]);

  const refreshProPresenter = useCallback(async () => {
    if (!canConfigure) return;
    setPendingProPresenterOperation("refresh");
    setProPresenterError(null);
    setProPresenterMessage(null);
    try {
      const response = await refreshProPresenterTimers();
      setProPresenter(response.propresenter);
      setProPresenterMessage(response.message);
      if (!response.accepted) setProPresenterError(response.message);
    } catch (cause) {
      setProPresenterError(
        describeError(cause, "Presentation timer refresh failed."),
      );
    } finally {
      setPendingProPresenterOperation(null);
    }
  }, [canConfigure]);

  const saveLights = useCallback(async (input: LightsSettingsInput) => {
    if (!canConfigure) return;
    setPendingLightsOperation("save");
    setLightsError(null);
    setLightsMessage(null);
    try {
      const response = await updateLightsSettings(input);
      setLights(response.lights);
      setSettings(await getSettings());
      setLightsMessage(response.message);
      if (!response.accepted) setLightsError(response.message);
    } catch (cause) {
      setLightsError(describeError(cause, "Lights configuration failed."));
    } finally {
      setPendingLightsOperation(null);
    }
  }, [canConfigure]);

  const refreshLights = useCallback(async () => {
    if (!canConfigure) return;
    setPendingLightsOperation("refresh");
    setLightsError(null);
    setLightsMessage(null);
    try {
      setLights(await refreshLightingOutputs());
      setLightsMessage("Lights MIDI outputs refreshed.");
    } catch (cause) {
      setLightsError(describeError(cause, "Lights output refresh failed."));
    } finally {
      setPendingLightsOperation(null);
    }
  }, [canConfigure]);

  const sendLightingTest = useCallback(async (note: number, velocity: number) => {
    if (!canOperate) return;
    setPendingLightsOperation("test");
    setLightsError(null);
    setLightsMessage(null);
    try {
      const response = await testLightingCue(note, velocity);
      setLights(response.lights);
      setLightsMessage(response.message);
      if (!response.accepted) setLightsError(response.message);
    } catch (cause) {
      setLightsError(describeError(cause, "Lights test cue failed."));
    } finally {
      setPendingLightsOperation(null);
    }
  }, [canOperate]);

  const saveLightingCues = useCallback(async (song: Song, cues: LightingCue[]) => {
    if (!canConfigure) return;
    setPendingLightsOperation("save-cues");
    setLightsError(null);
    setLightsMessage(null);
    const cueMap: SongLightingCueMap = {
      song_key: song.source_song_id ?? song.id,
      song_title: song.title,
      cues,
    };
    try {
      const response = await updateLightingCueMap(cueMap);
      setLights(response.lights);
      setSettings(await getSettings());
      setLightsMessage(response.message);
    } catch (cause) {
      setLightsError(describeError(cause, "Lighting cue map failed to save."));
    } finally {
      setPendingLightsOperation(null);
    }
  }, [canConfigure]);

  const clearAllLightingCues = useCallback(async (songs: Song[]) => {
    if (!canConfigure) return;
    setPendingLightsOperation("save-cues");
    setLightsError(null);
    setLightsMessage(null);
    try {
      let latestLights: LightsStatusResponse | null = null;
      for (const song of songs) {
        const response = await updateLightingCueMap({
          song_key: song.source_song_id ?? song.id,
          song_title: song.title,
          cues: [],
        });
        latestLights = response.lights;
      }
      if (latestLights) setLights(latestLights);
      setSettings(await getSettings());
      setLightsMessage(`Cleared lighting cues for all ${songs.length} songs in the service plan.`);
    } catch (cause) {
      setLightsError(
        describeError(cause, "Lighting cue maps failed to clear."),
      );
    } finally {
      setPendingLightsOperation(null);
    }
  }, [canConfigure]);

  const activateConfiguredServices = useCallback(async () => {
    if (!canActivateServices) return;
    if (!settings) return;
    const saved = settings.settings;

    await refreshMidi();

    if (
      saved.integration_modes.timer_output === "propresenter"
      && saved.propresenter.enabled
    ) {
      await saveProPresenter({
        host: saved.propresenter.host,
        port: saved.propresenter.port,
        timer_name: saved.propresenter.timer_name,
        look_id: saved.propresenter.look_id ?? null,
        request_timeout_seconds: saved.propresenter.request_timeout_seconds,
      });
    }

    if (saved.lights.enabled && saved.lights.output_name) {
      await saveLights({
        enabled: true,
        output_name: saved.lights.output_name,
        channel: saved.lights.channel,
        pulse_ms: saved.lights.pulse_ms,
      });
    }

    const planningCenterAuthConfigured = (
      planningCenterStatus?.connection_method === "oauth"
        ? planningCenterStatus.oauth_connected
        : Boolean(settings.planning_center_secret_saved && saved.planning_center.app_id)
    );
    if (
      saved.integration_modes.service_source === "planning_center"
      && planningCenterAuthConfigured
      && saved.planning_center.service_type_id
    ) {
      await savePlanningCenter(saved.planning_center, saved.timezone);
    }
  }, [canActivateServices,
    refreshMidi,
    planningCenterStatus,
    saveLights,
    savePlanningCenter,
    saveProPresenter,
    settings,
  ]);

  return {
    playback,
    state,
    health,
    live,
    error,
    actionMessage,
    pendingAction,
    pendingPlanId,
    settings,
    settingsError,
    settingsMessage,
    pendingSettingsOperation,
    planningCenterStatus,
    planningCenterServiceTypes,
    planningCenterError,
    planningCenterMessage,
    pendingPlanningCenterOperation,
    midi,
    midiMessages,
    midiError,
    midiMessage,
    pendingMidiOperation,
    pendingMidiCue,
    propresenter,
    propresenterError,
    propresenterMessage,
    pendingProPresenterOperation,
    lights,
    lightsError,
    lightsMessage,
    pendingLightsOperation,
    dispatch,
    retryLoads,
    selectPlan,
    saveGeneralSettings,
    saveMidiSettings,
    testPlanningCenterConnection,
    loadPlanningCenterServiceTypes,
    savePlanningCenter,
    signInPlanningCenterOAuth,
    disconnectPlanningCenter,
    refreshMidi,
    selectMidi,
    simulateMidi,
    saveProPresenter,
    runProPresenterTest,
    refreshProPresenter,
    saveLights,
    refreshLights,
    sendLightingTest,
    saveLightingCues,
    clearAllLightingCues,
    activateConfiguredServices,
  };
}
