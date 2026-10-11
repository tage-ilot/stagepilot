import { useEffect, useId, useMemo, useRef, useState } from "react";

import type {
  ApplicationState,
  LightingCue,
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
import { InlineAlert } from "./ui/InlineAlert";
import { Input } from "./ui/Input";
import { Panel } from "./ui/Panel";
import { SaveBar, type SaveBarState } from "./ui/SaveBar";
import { Select } from "./ui/Select";

const midiNoteNames = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"] as const;
const midiNoteName = (note: number) =>
  `${midiNoteNames[note % midiNoteNames.length]}${Math.floor(note / 12) - 2}`;

type NoteDisplayMode = "music" | "numeric";

const midiNoteOptions = (mode: NoteDisplayMode) =>
  Array.from({ length: 128 }, (_, note) => ({
    note,
    label: mode === "music" ? midiNoteName(note) : String(note),
  }));

const formatElapsed = (seconds: number) =>
  `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;

const parseElapsed = (value: string) => {
  const match = /^(\d{1,3}):([0-5]\d)$/.exec(value.trim());
  return match ? Number(match[1]) * 60 + Number(match[2]) : null;
};

const newCueId = () => {
  if (typeof globalThis.crypto?.randomUUID === "function") return globalThis.crypto.randomUUID();
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (value) => {
    const random = Math.floor(Math.random() * 16);
    const nibble = value === "x" ? random : (random & 0x3) | 0x8;
    return nibble.toString(16);
  });
};

const songKey = (song: Song) => song.source_song_id ?? song.id;

export function LightsSetupPanel({
  connectionStatus,
  lights,
  settings,
  state,
  error,
  message,
  pendingOperation,
  onClose,
  onRefresh,
  onSaveSettings,
  onTest,
  onSaveCues,
  onClearAllCues,
}: {
  connectionStatus?: ConnectionStatus;
  lights: LightsStatusResponse | null;
  settings: SettingsResponse | null;
  state: ApplicationState;
  error: string | null;
  message: string | null;
  pendingOperation: "save" | "refresh" | "test" | "save-cues" | null;
  onClose?: () => void;
  onRefresh: () => void;
  onSaveSettings: (settings: LightsSettingsInput) => void;
  onTest: (note: number, velocity: number) => void;
  onSaveCues: (song: Song, cues: LightingCue[]) => void;
  onClearAllCues?: (songs: Song[]) => void;
}) {
  const testReasonId = useId();
  const songs = useMemo(() => state.plan?.songs ?? [], [state.plan?.songs]);
  const preferredSongKey = state.current_song ? songKey(state.current_song) : songs[0] ? songKey(songs[0]) : "";
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
  }, [savedCueMap, selectedSongKey]);

  const parsedSettings = useMemo<LightsSettingsInput | null>(() => {
    const parsedChannel = Number(channel);
    const parsedPulse = Number(pulseMs);
    if (!outputName) return null;
    if (!Number.isInteger(parsedChannel) || parsedChannel < 1 || parsedChannel > 15) return null;
    if (!Number.isInteger(parsedPulse) || parsedPulse < 10 || parsedPulse > 2_000) return null;
    return {
      enabled: true,
      output_name: outputName,
      channel: parsedChannel,
      pulse_ms: parsedPulse,
    };
  }, [channel, outputName, pulseMs]);

  const persistCues = (nextCues: LightingCue[]) => {
    const sortedCues = [...nextCues].sort((left, right) => left.at_seconds - right.at_seconds);
    setCues(sortedCues);
    if (selectedSong) onSaveCues(selectedSong, sortedCues);
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

  const clearSelectedSongCues = () => {
    if (!selectedSong || !cues.length) return;
    if (confirmClearSelectedLightingCues(selectedSong.title)) {
      persistCues([]);
    }
  };

  const clearEverySongCues = () => {
    if (!songs.length) return;
    if (confirmClearAllLightingCues(songs.map((song) => song.title), state.plan?.title ?? null)) {
      setCues([]);
      if (onClearAllCues) onClearAllCues(songs);
      else songs.forEach((song) => onSaveCues(song, []));
    }
  };

  const savedLights = settings?.settings.lights ?? lights;
  const savedOutputName = savedLights?.output_name ?? null;
  // Test cue goes out through the connected (saved) port, so it is only offered
  // while the draft output and channel equal what is saved.
  const draftDiffersFromSaved = savedLights != null
    && (savedOutputName === null
      || outputName !== savedOutputName
      || channel !== String(savedLights.channel));
  // Held changes: only the output and the channel are listed (DESIGN.md). Note pulse saves
  // immediately (on blur or Enter, so a half-typed number never reconfigures the MIDI output)
  // and sends the saved output and channel, never the held drafts.
  const heldOutput = savedLights != null && outputName !== (savedOutputName ?? "");
  const heldChannel = savedLights != null && channel !== String(savedLights.channel);
  const changeCount = Number(heldOutput) + Number(heldChannel);
  const pulseDiffers = savedLights != null && pulseMs !== String(savedLights.pulse_ms);
  const pulseValid = /^\d+$/.test(pulseMs) && Number(pulseMs) >= 10 && Number(pulseMs) <= 2_000;
  const [pulseSaving, setPulseSaving] = useState(false);
  const [pulseSaveFailed, setPulseSaveFailed] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [autoSavedAt, setAutoSavedAt] = useState<string | null>(null);
  const previousOperation = useRef<typeof pendingOperation>(null);
  useEffect(() => {
    const previous = previousOperation.current;
    previousOperation.current = pendingOperation;
    if (previous === "save" && pendingOperation !== "save") {
      if (pulseSaving) {
        setPulseSaving(false);
        setPulseSaveFailed(Boolean(error));
        if (!error) setAutoSavedAt(new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
      } else {
        setSaveFailed(Boolean(error));
      }
    }
    if (previous === "save-cues" && pendingOperation !== "save-cues" && !error) {
      setAutoSavedAt(new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
    }
  }, [pendingOperation, error, pulseSaving]);
  const saveState: SaveBarState = pendingOperation === "save" && !pulseSaving
    ? { kind: "saving" }
    : saveFailed && changeCount > 0
      ? { kind: "error", message: "Couldn't save" }
      : changeCount > 0
        ? { kind: "held", count: changeCount }
        : autoSavedAt
          ? { kind: "auto", at: autoSavedAt }
          : { kind: "clean" };
  const blockedReason = changeCount > 0 && parsedSettings === null
    ? "Choose an output, a channel from 1 to 15 and a note length from 10 to 2000 ms before saving."
    : null;

  const discard = () => {
    if (!savedLights) return;
    setOutputName(savedLights.output_name ?? "");
    setChannel(String(savedLights.channel));
    setPulseMs(String(savedLights.pulse_ms));
    setSaveFailed(false);
    setPulseSaveFailed(false);
  };

  const savePulse = () => {
    if (!savedLights || !pulseDiffers || !pulseValid || pulseSaving || pendingOperation !== null) return;
    setPulseSaving(true);
    setPulseSaveFailed(false);
    setAutoSavedAt(null);
    onSaveSettings({
      enabled: savedLights.enabled,
      output_name: savedOutputName,
      channel: savedLights.channel,
      pulse_ms: Number(pulseMs),
    });
  };

  const save = () => {
    // A clean Save is a no-op once Lights is enabled; the note pulse edit alone still saves.
    if (!parsedSettings || pendingOperation !== null) return;
    if (changeCount === 0 && !pulseDiffers && savedLights?.enabled) return;
    setSaveFailed(false);
    setAutoSavedAt(null);
    onSaveSettings(parsedSettings);
  };
  const currentOutputMissing = outputName
    && !lights?.outputs.some((output) => output.name === outputName);

  return (
    <Panel className="mt-5" id="lights-configuration" variant="setup">
      <SetupPanelHeader
        closeLabel="Close Lights configuration"
        description="Send elapsed-time MIDI Note On/Off cues to Lights."
        onClose={onClose}
        status={connectionStatus ?? lights?.connection_status ?? "loading"}
        title="Lights"
      />

      {(error || message) && (
        <InlineAlert aria-live="polite" className="mt-4" tone={error ? "danger" : "info"}>
          {error ?? message}
        </InlineAlert>
      )}

      <h3 className="mt-5 font-semibold text-ink">Basic</h3>
      <p className="mt-2 text-sm text-ink-body">Sends cues by MIDI</p>
      <div className="mt-3 grid gap-4 lg:grid-cols-4">
        <Field className="lg:col-span-2" heldChange={heldOutput} label="MIDI Output to Lighting Controller">
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
        <Field error={pulseSaveFailed ? "Couldn't save the note length" : undefined} hint={pulseSaving && pendingOperation === "save" ? "Saving…" : undefined} label="Note length (ms)">
          <Input className="tabular-nums" max={2000} min={10} onBlur={savePulse} onChange={(event) => { setPulseMs(event.target.value); setPulseSaveFailed(false); }} onKeyDown={(event) => { if (event.key === "Enter") savePulse(); }} type="number" value={pulseMs} />
        </Field>
      </div>
      <p className="mt-2 text-type-body-sm text-ink-muted">Choose this network session under Settings → External Control → Input. Channels 1–15 are supported here.</p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button disabled={pendingOperation !== null} onClick={onRefresh} tone="lights" variant="tinted">{pendingOperation === "refresh" ? "Refreshing…" : "Refresh outputs"}</Button>
      </div>
      {blockedReason && <p className="mt-4 text-sm text-ink-body" id="lights-save-blocked">{blockedReason}</p>}
      <SaveBar className="mt-4" onDiscard={discard} onRetry={save} onSave={save} state={saveState} />

      <div className="mt-6 border-t border-edge-subtle pt-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h3 className="font-semibold text-ink">Test cue</h3>
          <div className="flex items-center gap-2" aria-label="MIDI note display">
            <span aria-hidden="true" className={noteDisplayMode === "music" ? "text-info-text" : "text-ink-muted"}>♪</span>
            <button
              aria-checked={noteDisplayMode === "numeric"}
              aria-label={`MIDI note display: ${noteDisplayMode === "music" ? "music notation" : "numeric value"}`}
              className={`relative h-7 w-12 shrink-0 cursor-pointer touch-manipulation rounded-ds-full border border-integration-lights/30 transition-colors duration-[140ms] motion-reduce:transition-none can-hover:border-integration-lights/60 coarse:after:absolute coarse:after:inset-x-0 coarse:after:-inset-y-2 coarse:after:content-[''] ${noteDisplayMode === "numeric" ? "bg-integration-lights/20" : "bg-surface-900"}`}
              onClick={() => setNoteDisplayMode((current) => current === "music" ? "numeric" : "music")}
              role="switch"
              type="button"
            >
              <span
                aria-hidden="true"
                className={`absolute left-0 top-1 h-4 w-4 rounded-ds-full bg-integration-lights shadow-sm transition-transform duration-200 motion-reduce:transition-none ${noteDisplayMode === "numeric" ? "translate-x-7" : "translate-x-1"}`}
              />
            </button>
            <span aria-hidden="true" className={noteDisplayMode === "numeric" ? "text-info-text" : "text-ink-muted"}>#</span>
          </div>
        </div>
        <div className="mt-3 grid gap-3 sm:grid-cols-[minmax(0,1fr)_8rem_auto]">
          <Field label="Test lighting note">
            <Select aria-label="Test lighting note" onChange={(event) => setTestNote(event.target.value)} value={testNote}>
              {displayedNoteOptions.map((option) => <option key={option.note} value={option.note}>{option.label}</option>)}
            </Select>
          </Field>
          <Field label="Test lighting velocity">
            <Input aria-label="Test lighting velocity" className="tabular-nums" max={127} min={1} onChange={(event) => setTestVelocity(event.target.value)} type="number" value={testVelocity} />
          </Field>
          <Button className="self-end" aria-describedby={draftDiffersFromSaved ? testReasonId : undefined} disabled={lights?.connection_status !== "connected" || pendingOperation !== null || draftDiffersFromSaved} onClick={() => onTest(Number(testNote), Number(testVelocity))} tone="lights" variant="tinted">{pendingOperation === "test" ? "Sending…" : "Send test cue"}</Button>
        </div>
        {draftDiffersFromSaved && <p className="mt-2 text-sm text-ink-muted" id={testReasonId}>Save the new output first. Test cue sends to the saved output.</p>}
      </div>

      <div className="mt-6 border-t border-edge-subtle pt-5">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <Field className="min-w-64" label="Song lighting timeline">
            <Select onChange={(event) => setSelectedSongKey(event.target.value)} value={selectedSongKey}>
              {!songs.length && <option value="">Load a service plan first</option>}
              {songs.map((song) => <option key={songKey(song)} value={songKey(song)}>{song.order}. {song.title}</option>)}
            </Select>
          </Field>
          <Button disabled={!selectedSong || pendingOperation !== null} onClick={addCue} tone="lights" variant="tinted">Add lighting cue</Button>
        </div>
        <p className="mt-2 text-type-body-sm text-ink-muted">Cue times are elapsed from song start. Changes save automatically. The timeline begins only after the Presentation countdown confirms it started.</p>

        <div className="mt-3 grid gap-2">
          {cues.map((cue) => (
            <div className="grid gap-2 border-t border-edge-subtle pt-3 first:border-t-0 first:pt-0 md:grid-cols-[7rem_minmax(11rem,1fr)_7rem_minmax(9rem,1fr)_auto_auto]" key={cue.id}>
              <Field label="Cue elapsed time">
                <Input aria-label="Cue elapsed time" className="font-mono tabular-nums" defaultValue={formatElapsed(cue.at_seconds)} disabled={pendingOperation !== null} onBlur={(event) => { const parsed = parseElapsed(event.target.value); if (parsed !== null && (selectedSong?.duration_seconds == null || parsed <= selectedSong.duration_seconds)) updateCue(cue.id, { at_seconds: parsed }, true); else event.target.value = formatElapsed(cue.at_seconds); }} placeholder="00:00" />
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
                <Input aria-label="Lighting cue label" disabled={pendingOperation !== null} maxLength={120} onBlur={() => persistCues(cues)} onChange={(event) => updateCue(cue.id, { label: event.target.value })} placeholder="Verse, chorus, blackout…" value={cue.label} />
              </Field>
              <Button className="self-end" disabled={lights?.connection_status !== "connected" || pendingOperation !== null} onClick={() => onTest(cue.note, cue.velocity)} tone="lights" variant="tinted">Test</Button>
              <Button className="self-end" aria-label={`Remove cue at ${formatElapsed(cue.at_seconds)}`} disabled={pendingOperation !== null} onClick={() => persistCues(cues.filter((item) => item.id !== cue.id))} tone="danger" variant="tinted">Remove</Button>
            </div>
          ))}
          {!cues.length && <p className="px-4 py-5 text-center text-type-body-sm text-ink-muted">No lighting cues saved for this song.</p>}
        </div>
        {selectedSong?.duration_seconds != null && <p className="mt-2 text-type-body-sm text-ink-muted">Scheduled song length: {formatElapsed(selectedSong.duration_seconds)}. Cues must fall within this duration.</p>}
        <div className="mt-4 flex flex-wrap gap-2">
          <Button disabled={!cues.length || pendingOperation !== null || !selectedSong} onClick={clearSelectedSongCues} variant="destructive">{pendingOperation === "save-cues" ? "Saving cues…" : "Clear"}</Button>
          <Button disabled={!songs.length || pendingOperation !== null} onClick={clearEverySongCues} variant="destructive">{pendingOperation === "save-cues" ? "Saving cues…" : "Clear all"}</Button>
        </div>
      </div>
    </Panel>
  );
}
