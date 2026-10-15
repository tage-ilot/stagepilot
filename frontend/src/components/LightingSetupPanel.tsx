import { useEffect, useId, useMemo, useRef, useState } from "react";

import type {
  ApplicationState,
  LightingCaptureResponse,
  LightingCue,
  LightingMethod,
  LightingPad,
  OscArgument,
  OscLightingCue,
  LightsSettingsInput,
  LightsStatusResponse,
  ConnectionStatus,
  SettingsResponse,
  Song,
} from "../types";
import {
  confirmClearAllLightingCues,
  confirmClearSelectedLightingCues,
} from "./lightingCueConfirmations";
import { SetupPanelHeader } from "./SetupPanelHeader";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { PanelAlert } from "./PanelAlert";
import { Input } from "./ui/Input";
import { Panel } from "./ui/Panel";
import { SaveBar, type SaveBarState } from "./ui/SaveBar";
import { useSaveAttempt } from "../hooks/useSaveAttempt";
import { Select } from "./ui/Select";
import { ConnectionMethod } from "./ui/ConnectionMethod";
import { OscArgumentRows, OscConnectionFields, OscCueRow } from "./LightingOscParts";
import { LightingCueBoard, type CaptureMode } from "./LightingCueBoard";
import { LightingTimeline } from "./LightingTimeline";
import { clampCueTime, elapsedSince, snapTenths, type TimelineMarker } from "./lightingTimelineMath";
import {
  formatElapsed,
  parseElapsed,
  newOscCue,
  oscAddressError,
  oscHostError,
  oscPortError,
  parseOscArguments,
  type OscArgumentDraft,
} from "./lightingOsc";

const midiNoteNames = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"] as const;
const midiNoteName = (note: number) =>
  `${midiNoteNames[note % midiNoteNames.length]}${Math.floor(note / 12) - 2}`;

type NoteDisplayMode = "music" | "numeric";

const midiNoteOptions = (mode: NoteDisplayMode) =>
  Array.from({ length: 128 }, (_, note) => ({
    note,
    label: mode === "music" ? midiNoteName(note) : String(note),
  }));

const newCueId = () => {
  if (typeof globalThis.crypto?.randomUUID === "function") return globalThis.crypto.randomUUID();
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (value) => {
    const random = Math.floor(Math.random() * 16);
    const nibble = value === "x" ? random : (random & 0x3) | 0x8;
    return nibble.toString(16);
  });
};

const songKey = (song: Song) => song.source_song_id ?? song.id;

/** The cue-board actions the panel needs from the app (kept together so Dashboard stays small). */
export interface LightingPanelActions {
  onPressPad: (
    padId: string,
    capture?: { song_key: string; song_title: string; at_seconds: number },
  ) => Promise<LightingCaptureResponse | null>;
  onUndoCapture: (songKey: string, cueId: string) => Promise<boolean>;
  onSavePads: (pads: LightingPad[]) => Promise<boolean>;
}

export function LightingSetupPanel({
  connectionStatus,
  lights,
  settings,
  state,
  error,
  onRetryLoad,
  message,
  pendingOperation,
  onClose,
  onDirtyChange,
  onRefresh,
  onSaveSettings,
  onTest,
  onTestOsc,
  onSaveCues,
  onClearAllCues,
  onPressPad,
  onUndoCapture,
  onSavePads,
}: {
  connectionStatus?: ConnectionStatus;
  lights: LightsStatusResponse | null;
  settings: SettingsResponse | null;
  state: ApplicationState;
  error: string | null;
  onRetryLoad?: () => void;
  message: string | null;
  pendingOperation: "save" | "refresh" | "test" | "save-cues" | null;
  onClose?: () => void;
  onDirtyChange?: (dirty: boolean) => void;
  onRefresh: () => void;
  onSaveSettings: (settings: LightsSettingsInput) => void;
  onTest: (note: number, velocity: number) => void;
  onTestOsc?: (address: string, args: OscArgument[]) => void;
  onSaveCues: (song: Song, cues: LightingCue[] | OscLightingCue[], method: LightingMethod) => void;
  onClearAllCues?: (songs: Song[], method: LightingMethod) => void;
} & Partial<LightingPanelActions>) {
  const testReasonId = useId();
  const songs = useMemo(() => state.plan?.songs ?? [], [state.plan?.songs]);
  const preferredSongKey = state.current_song ? songKey(state.current_song) : songs[0] ? songKey(songs[0]) : "";
  const [method, setMethod] = useState<LightingMethod>("osc");
  const [oscHost, setOscHost] = useState("127.0.0.1");
  const [oscPort, setOscPort] = useState("21600");
  const [oscTestAddress, setOscTestAddress] = useState("");
  const [oscTestArgs, setOscTestArgs] = useState<OscArgumentDraft[]>([]);
  const [oscCues, setOscCues] = useState<OscLightingCue[]>([]);
  const [outputName, setOutputName] = useState("");
  const [channel, setChannel] = useState("1");
  const [pulseMs, setPulseMs] = useState("100");
  const [testNote, setTestNote] = useState("60");
  const [testVelocity, setTestVelocity] = useState("127");
  const [selectedSongKey, setSelectedSongKey] = useState(preferredSongKey);
  const [cues, setCues] = useState<LightingCue[]>([]);
  const [noteDisplayMode, setNoteDisplayMode] = useState<NoteDisplayMode>("music");
  const displayedNoteOptions = useMemo(
    () => midiNoteOptions(noteDisplayMode),
    [noteDisplayMode],
  );

  // Each draft follows its own saved value, so saving one field (or a cue) never overwrites
  // another field's unsaved draft.
  const syncSource = settings?.settings.lights ?? lights;
  const syncedOutput = syncSource ? (syncSource.output_name ?? "") : null;
  const syncedChannel = syncSource ? String(syncSource.channel) : null;
  const syncedPulse = syncSource ? String(syncSource.pulse_ms) : null;
  const syncedMethod = syncSource ? (syncSource.method ?? "midi") : null;
  const syncedHost = syncSource ? (syncSource.osc_host ?? "127.0.0.1") : null;
  const syncedPort = syncSource ? String(syncSource.osc_port ?? 21600) : null;
  useEffect(() => {
    if (syncedMethod !== null) setMethod(syncedMethod);
  }, [syncedMethod]);
  useEffect(() => {
    if (syncedHost !== null) setOscHost(syncedHost);
  }, [syncedHost]);
  useEffect(() => {
    if (syncedPort !== null) setOscPort(syncedPort);
  }, [syncedPort]);
  useEffect(() => {
    if (syncedOutput !== null) setOutputName(syncedOutput);
  }, [syncedOutput]);
  useEffect(() => {
    if (syncedChannel !== null) setChannel(syncedChannel);
  }, [syncedChannel]);
  useEffect(() => {
    if (syncedPulse !== null) setPulseMs(syncedPulse);
  }, [syncedPulse]);

  useEffect(() => {
    if (!songs.some((song) => songKey(song) === selectedSongKey)) {
      setSelectedSongKey(preferredSongKey);
    }
  }, [preferredSongKey, selectedSongKey, songs]);

  const selectedSong = songs.find((song) => songKey(song) === selectedSongKey) ?? null;
  const savedCueMap = selectedSongKey
    ? settings?.settings.lights.cue_maps[selectedSongKey]
    : undefined;

  useEffect(() => {
    setCues(savedCueMap?.cues.map((cue) => ({ ...cue })) ?? []);
    setOscCues(savedCueMap?.osc_cues?.map((cue) => ({ ...cue, args: cue.args.map((arg) => ({ ...arg })) })) ?? []);
  }, [savedCueMap, selectedSongKey]);

  // ---- Cue board, live capture and timeline ----
  const pads = settings?.settings.lights.pads ?? [];
  const [captureMode, setCaptureMode] = useState<CaptureMode>("capture");
  const [captureStatus, setCaptureStatus] = useState<string | null>(null);
  const [lastCapture, setLastCapture] = useState<{ songKey: string; cueId: string } | null>(null);
  const [boardBusy, setBoardBusy] = useState(false);
  const boardBusyRef = useRef(false);
  const [clock, setClock] = useState(() => Date.now());
  const playingSong = state.playing_song ?? state.current_song;
  const timerRunning = state.timer?.status === "running" && Boolean(state.timer?.started_at);
  const selectedSongRunning = timerRunning && selectedSong !== null
    && playingSong !== null && playingSong !== undefined && songKey(playingSong) === songKey(selectedSong);
  useEffect(() => {
    setClock(Date.now());
    if (!selectedSongRunning) return;
    const timer = window.setInterval(() => setClock(Date.now()), 100);
    return () => window.clearInterval(timer);
  }, [selectedSongRunning, state.timer?.started_at]);
  const elapsed = selectedSongRunning
    ? elapsedSince(state.timer?.started_at, state.timer?.position_seconds, clock)
    : null;
  const songLength = selectedSong?.duration_seconds ?? null;

  const pressPad = async (pad: LightingPad) => {
    if (!onPressPad || boardBusyRef.current) return;
    boardBusyRef.current = true;
    setBoardBusy(true);
    try {
      // Read the clock at the press, not at the last render tick.
      const now = Date.now();
      const live = selectedSongRunning && selectedSong
        ? elapsedSince(state.timer?.started_at, state.timer?.position_seconds, now)
        : null;
      const capturing = captureMode === "capture" && live !== null && selectedSong !== null;
      const at = capturing ? clampCueTime(snapTenths(live), songLength) : 0;
      const response = await onPressPad(
        pad.id,
        capturing && selectedSong
          ? { song_key: songKey(selectedSong), song_title: selectedSong.title, at_seconds: at }
          : undefined,
      );
      if (!response) { setCaptureStatus(null); return; }
      if (response.cue_id && selectedSong) {
        setLastCapture({ songKey: songKey(selectedSong), cueId: response.cue_id });
        setCaptureStatus(response.accepted
          ? `Added at ${formatElapsed(response.at_seconds ?? at)}`
          : `Added at ${formatElapsed(response.at_seconds ?? at)}, but the cue did not send: ${response.message}`);
      } else if (!response.accepted) {
        setCaptureStatus(response.message);
      } else if (captureMode === "preview") {
        setCaptureStatus("Fired in preview. Nothing was added to the song.");
      } else {
        setCaptureStatus("Fired. No song is running, so nothing was added.");
      }
    } finally {
      boardBusyRef.current = false;
      setBoardBusy(false);
    }
  };
  const undoCapture = async () => {
    if (!onUndoCapture || !lastCapture || boardBusyRef.current) return;
    boardBusyRef.current = true;
    setBoardBusy(true);
    try {
      const ok = await onUndoCapture(lastCapture.songKey, lastCapture.cueId);
      setCaptureStatus(ok ? "Last capture undone." : "That capture was already changed or removed.");
      setLastCapture(null);
    } finally {
      boardBusyRef.current = false;
      setBoardBusy(false);
    }
  };

  const parsedSettings = useMemo<LightsSettingsInput | null>(() => {
    const osc = {
      osc_host: oscHost.trim(),
      osc_port: Number(oscPort),
    };
    if (method === "osc") {
      if (oscHostError(oscHost) || oscPortError(oscPort)) return null;
      return {
        enabled: true,
        method,
        output_name: syncSource?.output_name ?? null,
        channel: syncSource?.channel ?? 1,
        pulse_ms: syncSource?.pulse_ms ?? 100,
        ...osc,
      };
    }
    const parsedChannel = Number(channel);
    const parsedPulse = Number(pulseMs);
    if (!outputName) return null;
    if (!Number.isInteger(parsedChannel) || parsedChannel < 1 || parsedChannel > 15) return null;
    if (!Number.isInteger(parsedPulse) || parsedPulse < 10 || parsedPulse > 2_000) return null;
    return {
      enabled: true,
      method,
      output_name: outputName,
      channel: parsedChannel,
      pulse_ms: parsedPulse,
      ...osc,
    };
  }, [channel, method, oscHost, oscPort, outputName, pulseMs, syncSource]);

  const persistCues = (nextCues: LightingCue[]) => {
    const sortedCues = [...nextCues].sort((left, right) => left.at_seconds - right.at_seconds);
    setCues(sortedCues);
    if (selectedSong) onSaveCues(selectedSong, sortedCues, "midi");
  };

  const persistOscCues = (nextCues: OscLightingCue[]) => {
    const sortedCues = [...nextCues].sort((left, right) => left.at_seconds - right.at_seconds);
    setOscCues(sortedCues);
    if (selectedSong) onSaveCues(selectedSong, sortedCues, "osc");
  };

  // Timeline edits go through the same save as the cue rows, so the two stay in sync.
  const moveCue = (id: string, at: number) => {
    if (method === "osc") persistOscCues(oscCues.map((cue) => cue.id === id ? { ...cue, at_seconds: at } : cue));
    else persistCues(cues.map((cue) => cue.id === id ? { ...cue, at_seconds: at } : cue));
  };
  const removeCue = (id: string) => {
    if (method === "osc") persistOscCues(oscCues.filter((cue) => cue.id !== id));
    else persistCues(cues.filter((cue) => cue.id !== id));
  };

  const updateCue = (id: string, update: Partial<LightingCue>, save = false) => {
    const nextCues = cues.map((cue) => cue.id === id ? { ...cue, ...update } : cue);
    if (save) persistCues(nextCues);
    else setCues(nextCues);
  };

  const addCue = () => {
    persistCues([
      ...cues,
      {
        id: newCueId(),
        at_seconds: 0,
        note: 60,
        velocity: 127,
        label: "",
      },
    ]);
  };

  const addOscCue = () => {
    persistOscCuesLocal([...oscCues, newOscCue(newCueId())]);
  };
  // A new OSC cue has no address yet, so it is only shown; it saves once its address is valid.
  const persistOscCuesLocal = (next: OscLightingCue[]) => setOscCues(next);

  const clearSelectedSongCues = () => {
    if (!selectedSong || !shownCues.length) return;
    if (confirmClearSelectedLightingCues(selectedSong.title)) {
      if (method === "osc") persistOscCues([]);
      else persistCues([]);
    }
  };

  const clearEverySongCues = () => {
    if (!songs.length) return;
    if (confirmClearAllLightingCues(songs.map((song) => song.title), state.plan?.title ?? null)) {
      if (method === "osc") setOscCues([]);
      else setCues([]);
      if (onClearAllCues) onClearAllCues(songs, method);
      else songs.forEach((song) => onSaveCues(song, [], method));
    }
  };

  const shownCues = method === "osc" ? oscCues : cues;
  // Only the method on screen is drawn; cues of the other method stay saved.
  const timelineMarkers: TimelineMarker[] = method === "osc"
    ? oscCues.filter((cue) => cue.address !== "").map((cue) => ({ id: cue.id, at: cue.at_seconds, label: cue.label || cue.address, color: cue.color ?? null }))
    : cues.map((cue) => ({ id: cue.id, at: cue.at_seconds, label: cue.label || `Note ${cue.note}`, color: cue.color ?? null }));
  const savedLighting = settings?.settings.lights ?? lights;
  const savedMethod: LightingMethod = savedLighting?.method ?? "midi";
  const savedOutputName = savedLighting?.output_name ?? null;
  // Test cue goes out through the connected (saved) port, so it is only offered
  // while the draft output and channel equal what is saved.
  const heldMethod = savedLighting != null && method !== savedMethod;
  const heldHost = savedLighting != null && oscHost !== (savedLighting.osc_host ?? "127.0.0.1");
  const heldPort = savedLighting != null && oscPort !== String(savedLighting.osc_port ?? 21600);
  const oscDraftDiffers = heldMethod || heldHost || heldPort;
  const draftDiffersFromSaved = method === "osc"
    ? oscDraftDiffers
    : savedLighting != null
      && (heldMethod
        || savedOutputName === null
        || outputName !== savedOutputName
        || channel !== String(savedLighting.channel));
  // Held changes: output, channel and note pulse. Any Lighting save reconfigures the plugin
  // (cancels the running timeline, reopens the MIDI port), so none of them saves on its own.
  const heldOutput = savedLighting != null && outputName !== (savedOutputName ?? "");
  const heldChannel = savedLighting != null && channel !== String(savedLighting.channel);
  const heldPulse = savedLighting != null && pulseMs !== String(savedLighting.pulse_ms);
  const changeCount = method === "osc"
    ? Number(heldMethod) + Number(heldHost) + Number(heldPort)
    : Number(heldMethod) + Number(heldOutput) + Number(heldChannel) + Number(heldPulse);
  useEffect(() => { onDirtyChange?.(changeCount > 0); return () => onDirtyChange?.(false); }, [changeCount, onDirtyChange]);
  const [saveFailed, setSaveFailed] = useState(false);
  const attempt = useSaveAttempt("lighting-configuration");
  const pulseNumber = Number(pulseMs);
  const errOutput = method === "midi" && attempt.attempted && !outputName ? "Choose an output." : undefined;
  const errPulse = attempt.attempted && !(Number.isInteger(pulseNumber) && pulseNumber >= 10 && pulseNumber <= 2000) ? "Enter a note length from 10 to 2000 ms." : undefined;
  const [autoSavedAt, setAutoSavedAt] = useState<string | null>(null);
  const previousOperation = useRef<typeof pendingOperation>(null);
  useEffect(() => {
    const previous = previousOperation.current;
    previousOperation.current = pendingOperation;
    if (previous === "save" && pendingOperation !== "save") {
      setSaveFailed(Boolean(error));
    }
    if (previous === "save-cues" && pendingOperation !== "save-cues" && !error) {
      setAutoSavedAt(new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
    }
  }, [pendingOperation, error]);
  const saveState: SaveBarState = pendingOperation === "save"
    ? { kind: "saving" }
    : saveFailed && changeCount > 0
      ? { kind: "error", message: "Couldn't save" }
      : changeCount > 0
        ? { kind: "held", count: changeCount }
        : autoSavedAt
          ? { kind: "auto", at: autoSavedAt }
          : { kind: "clean" };
  const blockedReason = changeCount > 0 && parsedSettings === null
    ? method === "osc"
      ? "Enter a host and a UDP port from 1 to 65535 before saving."
      : "Choose an output, a channel from 1 to 15 and a note length from 10 to 2000 ms before saving."
    : null;

  const discard = () => {
    if (!savedLighting) return;
    setMethod(savedLighting.method ?? "midi");
    setOscHost(savedLighting.osc_host ?? "127.0.0.1");
    setOscPort(String(savedLighting.osc_port ?? 21600));
    setOutputName(savedLighting.output_name ?? "");
    setChannel(String(savedLighting.channel));
    setPulseMs(String(savedLighting.pulse_ms));
    setSaveFailed(false);
    attempt.reset();
  };

  const save = () => {
    // A clean Save is a no-op once Lighting is enabled.
    if (pendingOperation !== null) return;
    if (changeCount === 0 && savedLighting?.enabled) return;
    if (!parsedSettings && method === "osc") { attempt.refuse(); return; }
    if (!parsedSettings) { attempt.refuse(); return; }
    setSaveFailed(false);
    setAutoSavedAt(null);
    onSaveSettings(parsedSettings);
  };
  const oscTestParsed = parseOscArguments(oscTestArgs);
  const oscTestAddressError = oscAddressError(oscTestAddress);
  const currentOutputMissing = outputName
    && !lights?.outputs.some((output) => output.name === outputName);

  return (
    <Panel className="mt-5" id="lighting-configuration" tone="lighting" variant="setup">
      <SetupPanelHeader
        closeLabel="Close Lighting configuration"
        description="Sends timed cues to your lighting software."
        onClose={onClose}
        status={connectionStatus ?? lights?.connection_status ?? "loading"}
        statusDetail={savedMethod === "midi" ? "MIDI · Alternate" : undefined}
        title="Lighting"
      />

      <ConnectionMethod
        disabled={!lights && !settings}
        heldChange={heldMethod}
        onChange={(value) => setMethod(value === "midi" ? "midi" : "osc")}
        options={[
          { value: "osc", label: "OSC", role: "primary", chip: "border-integration-lighting-osc bg-integration-lighting-osc/45" },
          { value: "midi", label: "MIDI", role: "alternate", chip: "border-integration-playback-midi bg-integration-playback-midi/45" },
        ]}
        value={method}
      />

      <PanelAlert className="mt-4" dot error={error} message={message} messageTone="info" onRetry={!lights ? onRetryLoad : undefined} />

      {method === "osc" ? (
        <>
          <OscConnectionFields
            attempted={attempt.attempted}
            held={{ host: heldHost, port: heldPort }}
            host={oscHost}
            onHost={setOscHost}
            onPort={setOscPort}
            port={oscPort}
          />
          <p className="mt-2 text-type-body-sm text-ink-muted" data-testid="osc-status">
            {savedMethod === "osc" && lights?.connection_status === "error"
              ? lights.detail
              : savedMethod === "osc" && lights?.last_osc_cue
                ? `Last sent: ${lights.last_osc_cue.address}. UDP does not confirm delivery.`
                : "OSC is sent over UDP, so StagePilot cannot confirm a cue arrived. Use Send test cue to check."}
          </p>
          {heldMethod && <p className="mt-2 text-type-body-sm text-ink-muted">MIDI cues stay saved and send nothing while OSC is the saved method.</p>}
        </>
      ) : (
        <>
      <div className="lighting-output mt-4">
      <div className="lighting-output-fields grid gap-4">
        <Field error={errOutput} heldChange={heldOutput} label="MIDI Output">
          <Select onChange={(event) => setOutputName(event.target.value)} value={outputName}>
            <option value="">Choose an output</option>
            {currentOutputMissing && <option value={outputName}>{outputName} (currently unavailable)</option>}
            {lights?.outputs.map((output, index) => (
              <option disabled={output.ambiguous} key={`${output.name}-${index}`} value={output.name}>
                {output.name}{output.connected ? " (connected)" : output.ambiguous ? " (ambiguous)" : ""}
              </option>
            ))}
          </Select>
        </Field>
        <Field heldChange={heldChannel} label="MIDI channel">
          <Select onChange={(event) => setChannel(event.target.value)} value={channel}>
            {Array.from({ length: 15 }, (_, index) => index + 1).map((value) => <option key={value} value={value}>{value}</option>)}
          </Select>
        </Field>
        <Field error={errPulse} heldChange={heldPulse} label="Note length (ms)">
          <Input className="tabular-nums" max={2000} min={10} onChange={(event) => setPulseMs(event.target.value)} type="number" value={pulseMs} />
        </Field>
      </div>
      </div>
      <p className="mt-2 text-type-body-sm text-ink-muted">Choose this network session under Settings → External Control → Input. Channels 1–15 are supported here.</p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button disabled={pendingOperation !== null} onClick={onRefresh} tone="lighting" variant="tinted">{pendingOperation === "refresh" ? "Refreshing…" : "Refresh outputs"}</Button>
      </div>
        </>
      )}
      <div className="mt-5 border-t border-edge-subtle pt-5">
        <div className="flex flex-wrap-reverse items-end gap-x-6 gap-y-4">
          <div className="flex min-w-0 flex-wrap items-end gap-x-4 gap-y-3">
          <Field className="w-full sm:w-80" label="Song lighting timeline">
            <Select onChange={(event) => setSelectedSongKey(event.target.value)} value={selectedSongKey}>
              {!songs.length && <option value="">Load a service plan first</option>}
              {songs.map((song) => <option key={songKey(song)} value={songKey(song)}>{song.order}. {song.title}</option>)}
            </Select>
          </Field>
          {method === "midi" && (
          <div className="flex items-center gap-2 pb-2" aria-label="MIDI note display">
            <span aria-hidden="true" className={noteDisplayMode === "music" ? "text-integration-lighting-text" : "text-ink-muted"}>♪</span>
            <button
              aria-checked={noteDisplayMode === "numeric"}
              aria-label={`MIDI note display: ${noteDisplayMode === "music" ? "music notation" : "numeric value"}`}
              className={`relative h-7 w-12 shrink-0 cursor-pointer touch-manipulation rounded-ds-full border border-integration-lighting-soft/30 transition-colors duration-[140ms] motion-reduce:transition-none can-hover:border-integration-lighting-soft/60 coarse:after:absolute coarse:after:inset-x-0 coarse:after:-inset-y-[9px] coarse:after:content-[''] ${noteDisplayMode === "numeric" ? "bg-integration-lighting-soft/20" : "bg-surface-900"}`}
              onClick={() => setNoteDisplayMode((current) => current === "music" ? "numeric" : "music")}
              role="switch"
              type="button"
            >
              <span
                aria-hidden="true"
                className={`absolute left-0 top-1 h-4 w-4 rounded-ds-full bg-integration-lighting-soft shadow-sm transition-transform duration-200 motion-reduce:transition-none ${noteDisplayMode === "numeric" ? "translate-x-7" : "translate-x-1"}`}
              />
            </button>
            <span aria-hidden="true" className={noteDisplayMode === "numeric" ? "text-integration-lighting-text" : "text-ink-muted"}>#</span>
          </div>
          )}
          <Button disabled={!selectedSong || pendingOperation !== null} onClick={method === "osc" ? addOscCue : addCue} tone="lighting" variant="tinted">Add cue</Button>
          </div>
          {method === "midi" && (
          <div aria-label="Test cue" className="lighting-test flex flex-wrap items-end gap-x-3 gap-y-2" role="group">
            <h3 className="pb-2.5 font-semibold text-ink">Test cue</h3>
            <div className="w-28 flex-none">
              <Field label="Note">
                <Select aria-label="Note" onChange={(event) => setTestNote(event.target.value)} value={testNote}>
                  {displayedNoteOptions.map((option) => <option key={option.note} value={option.note}>{option.label}</option>)}
                </Select>
              </Field>
            </div>
            <div className="w-28 flex-none">
              <Field label="Velocity">
                <Input aria-label="Velocity" className="tabular-nums" max={127} min={1} onChange={(event) => setTestVelocity(event.target.value)} type="number" value={testVelocity} />
              </Field>
            </div>
            <Button className="flex-none" aria-describedby={draftDiffersFromSaved ? testReasonId : undefined} disabled={lights?.connection_status !== "connected" || pendingOperation !== null || draftDiffersFromSaved} onClick={() => onTest(Number(testNote), Number(testVelocity))} tone="lighting" variant="tinted">{pendingOperation === "test" ? "Sending…" : "Send test cue"}</Button>
          </div>
          )}
        </div>
        {method === "osc" && (
          <div aria-label="OSC test cue" className="lighting-osc-test mt-4 grid gap-3" role="group">
            <h3 className="font-semibold text-ink">Test cue</h3>
            <Field error={oscTestAddress && oscTestAddressError ? oscTestAddressError : undefined} label="OSC address">
              <Input autoCapitalize="none" autoCorrect="off" className="font-mono" onChange={(event) => setOscTestAddress(event.target.value)} spellCheck={false} value={oscTestAddress} />
            </Field>
            <OscArgumentRows drafts={oscTestArgs} label="Test argument" onChange={setOscTestArgs} />
            {oscTestParsed.error && <p className="text-type-body-sm text-danger-text" role="alert">{oscTestParsed.error}</p>}
            <div>
              <Button
                aria-describedby={draftDiffersFromSaved ? testReasonId : undefined}
                disabled={pendingOperation !== null || draftDiffersFromSaved || savedMethod !== "osc" || Boolean(oscTestAddressError) || Boolean(oscTestParsed.error)}
                onClick={() => onTestOsc?.(oscTestAddress.trim(), oscTestParsed.args)}
                tone="lighting"
                variant="tinted"
              >
                {pendingOperation === "test" ? "Sending…" : "Send test cue"}
              </Button>
            </div>
          </div>
        )}
        {draftDiffersFromSaved && <p className="mt-2 text-sm text-ink-muted" id={testReasonId}>{method === "osc" ? "Save the OSC settings first. Test cue sends with the saved settings." : "Save the new output first. Test cue sends to the saved output."}</p>}
        <p className="mt-2 text-type-body-sm text-ink-muted">Cue times are elapsed from song start. Cues for the other method stay saved. Changes save automatically. The timeline begins only after the Presentation countdown confirms it started.</p>

        {onSavePads && (
          <LightingCueBoard
            busy={boardBusy || pendingOperation !== null}
            canUndo={lastCapture !== null}
            captureMode={captureMode}
            onCaptureMode={setCaptureMode}
            onPress={(pad) => void pressPad(pad)}
            onSavePads={onSavePads}
            onUndo={() => void undoCapture()}
            pads={pads}
            running={selectedSongRunning}
            savedMethod={savedMethod}
            status={captureStatus}
            viewMethod={method}
          />
        )}
        {selectedSong && (
          <LightingTimeline
            disabled={pendingOperation !== null}
            markers={timelineMarkers}
            onMove={moveCue}
            onRemove={removeCue}
            playhead={elapsed}
            songLength={songLength}
          />
        )}

        {/* The list scrolls once it is taller than 32rem, so it is drawn as its own bordered box (only while it has cues) and the
            user can see where the scrolling area starts and ends. */}
        <div
          aria-label={(method === "osc" ? oscCues.length : cues.length) > 0 ? "Lighting cues, scrollable" : undefined}
          className={`mt-3 grid max-h-[32rem] gap-2 overflow-y-auto overscroll-contain ${(method === "osc" ? oscCues.length : cues.length) > 0 ? "scroll-region scroll-region-y rounded-ds-lg p-3" : ""}`}
          data-testid="lighting-cue-list"
          role={(method === "osc" ? oscCues.length : cues.length) > 0 ? "region" : undefined}
          tabIndex={(method === "osc" ? oscCues.length : cues.length) > 0 ? 0 : undefined}
        >
          {method === "osc" && oscCues.map((cue) => (
            <OscCueRow
              canTest={savedMethod === "osc" && !draftDiffersFromSaved}
              cue={cue}
              disabled={pendingOperation !== null}
              key={`${cue.id}-${cue.at_seconds}`}
              onCommit={(next) => persistOscCues(oscCues.map((item) => item.id === next.id ? next : item))}
              onRemove={() => persistOscCues(oscCues.filter((item) => item.id !== cue.id))}
              onTest={(address, args) => onTestOsc?.(address, args)}
              songDuration={selectedSong?.duration_seconds ?? null}
              testDisabledReason={draftDiffersFromSaved}
              testReasonId={testReasonId}
            />
          ))}
          {method === "midi" && (
          <>
          {cues.map((cue) => (
            <div className="grid gap-2 border-t border-edge-subtle pt-3 first:border-t-0 first:pt-0 md:grid-cols-[7rem_minmax(11rem,1fr)_7rem_minmax(9rem,1fr)_auto_auto]" key={cue.id}>
              <Field label="Cue elapsed time">
                <Input aria-label="Cue elapsed time" className="font-mono tabular-nums" defaultValue={formatElapsed(cue.at_seconds)} key={`${cue.id}-${cue.at_seconds}`} disabled={pendingOperation !== null} onBlur={(event) => { const parsed = parseElapsed(event.target.value); if (parsed !== null && (selectedSong?.duration_seconds == null || parsed <= selectedSong.duration_seconds)) updateCue(cue.id, { at_seconds: parsed }, true); else event.target.value = formatElapsed(cue.at_seconds); }} placeholder="For example, 01:30…" />
              </Field>
              <Field label="Lighting cue note">
                <Select aria-label="Lighting cue note" disabled={pendingOperation !== null} onChange={(event) => updateCue(cue.id, { note: Number(event.target.value) }, true)} value={cue.note}>
                {displayedNoteOptions.map((option) => <option key={option.note} value={option.note}>{option.label}</option>)}
              </Select>
              </Field>
              <Field label="Lighting cue velocity">
                <Input aria-label="Lighting cue velocity" className="tabular-nums" disabled={pendingOperation !== null} max={127} min={1} onBlur={() => cue.velocity >= 1 && cue.velocity <= 127 && persistCues(cues)} onChange={(event) => updateCue(cue.id, { velocity: Number(event.target.value) })} type="number" value={cue.velocity} />
              </Field>
              <Field label="Lighting cue label">
                <Input aria-label="Lighting cue label" disabled={pendingOperation !== null} maxLength={120} onBlur={() => persistCues(cues)} onChange={(event) => updateCue(cue.id, { label: event.target.value })} placeholder="For example, Verse, chorus, blackout…" value={cue.label} />
              </Field>
              <Button className="self-end" aria-describedby={draftDiffersFromSaved ? testReasonId : undefined} disabled={lights?.connection_status !== "connected" || pendingOperation !== null || draftDiffersFromSaved} onClick={() => onTest(cue.note, cue.velocity)} tone="lighting" variant="tinted">Test</Button>
              <Button className="self-end" aria-label={`Remove cue at ${formatElapsed(cue.at_seconds)}`} disabled={pendingOperation !== null} onClick={() => persistCues(cues.filter((item) => item.id !== cue.id))} tone="danger" variant="tinted">Remove</Button>
            </div>
          ))}
          </>
          )}
          {!shownCues.length && <p className="px-4 py-5 text-center text-type-body-sm text-ink-muted">{method === "osc" ? "No OSC cues saved for this song." : "No lighting cues saved for this song."}</p>}
        </div>
        {selectedSong?.duration_seconds != null && <p className="mt-2 text-type-body-sm text-ink-muted">Scheduled song length: {formatElapsed(selectedSong.duration_seconds)}. Cues must fall within this duration.</p>}
        <div className="mt-4 flex flex-wrap gap-2">
          <Button disabled={!shownCues.length || pendingOperation !== null || !selectedSong} onClick={clearSelectedSongCues} variant="destructive">{pendingOperation === "save-cues" ? "Saving cues…" : "Clear"}</Button>
          <Button disabled={!songs.length || pendingOperation !== null} onClick={clearEverySongCues} variant="destructive">{pendingOperation === "save-cues" ? "Saving cues…" : "Clear all"}</Button>
        </div>
      </div>

      {blockedReason && <p className="mt-4 text-sm text-ink-body" id="lighting-save-blocked">{blockedReason}</p>}
      <SaveBar className="mt-4" onDiscard={discard} onRetry={save} onSave={save} state={saveState} />
    </Panel>
  );
}
