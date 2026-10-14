import { useCallback, useEffect, useMemo, useState } from "react";

import { ConnectionMethod } from "./ui/ConnectionMethod";
import { Disclosure } from "./ui/Disclosure";
import type {
  MidiCueName,
  MidiInputsResponse,
  MidiMonitorMessage,
  MidiSettingsInput,
  SettingsResponse,
  Song,
} from "../types";
import { SetupPanelHeader } from "./SetupPanelHeader";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { PlaybackApiPanel } from "./PlaybackApiPanel";
import { PlaybackSaveBar, usePlaybackDraft, type PlaybackMethod } from "./PlaybackSaveFooter";
import { usePlaybackSave } from "./usePlaybackSave";
import { playbackMidiNote } from "./playbackStatus";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { InlineAlert } from "./ui/InlineAlert";
import { PanelAlert } from "./PanelAlert";
import { Input } from "./ui/Input";
import { Panel } from "./ui/Panel";
import { Select } from "./ui/Select";

const cues: ReadonlyArray<readonly [MidiCueName, string]> = [
  ["start_next", "Start next"],
  ["restart_current", "Restart current"],
  ["previous", "Previous"],
  ["next", "Next"],
  ["reload_plan", "Reload service plan"],
  ["stop_timer", "Stop timer"],
];

const midiNoteNames = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"] as const;

const midiNoteName = (note: number) =>
  `${midiNoteNames[note % midiNoteNames.length]}${Math.floor(note / 12) - 2}`;

const midiNoteOptions = Array.from({ length: 128 }, (_, note) => ({
  note,
  label: `${midiNoteName(note)} (MIDI ${note})`,
}));

const monitorTone = (message: MidiMonitorMessage) => {
  if (message.disposition === "dispatched") return "text-integration-playback-text";
  if (message.disposition === "note_release") return "text-ink-muted";
  return "text-warning-text";
};

export function MidiSetupPanel({
  midi,
  messages,
  error,
  onRetryLoad,
  message,
  pendingOperation,
  onRefresh,
  onSelect,
  onClose,
  onDirtyChange,
  settings,
  pendingSettingsSave = false,
  settingsError = null,
  settingsMessage = null,
  onSaveSettings,
  songs = [],
  playback,
}: {
  midi: MidiInputsResponse | null;
  messages: MidiMonitorMessage[];
  error: string | null;
  onRetryLoad?: () => void;
  message: string | null;
  pendingOperation: "refresh" | "connect" | "disconnect" | null;
  onRefresh: () => void;
  onSelect: (inputId: string | null) => void;
  onClose?: () => void;
  onDirtyChange?: (dirty: boolean) => void;
  settings: SettingsResponse | null;
  pendingSettingsSave?: boolean;
  settingsError?: string | null;
  settingsMessage?: string | null;
  /** May resolve false when the save failed (the one Save configuration then stops and shows Couldn't save). */
  onSaveSettings: (settings: MidiSettingsInput) => void | boolean | Promise<boolean | void>;
  songs?: Song[];
  playback?: PlaybackController;
}) {
  // An install saved with the old simulated MIDI source opens on Playback API; the option no longer exists.
  const rawMethod = settings?.settings.integration_modes.midi_source;
  const savedMethod: PlaybackMethod = rawMethod === "real" ? "real" : "playback_api";
  const draft = usePlaybackDraft(playback, savedMethod);
  const [candidateId, setCandidateId] = useState("");
  const [channel, setChannel] = useState("1");
  const [note, setNote] = useState("112");
  const [debounce, setDebounce] = useState("250");
  const [velocities, setVelocities] = useState<Record<MidiCueName, string>>({
    start_next: "100",
    restart_current: "101",
    previous: "102",
    next: "103",
    reload_plan: "104",
    stop_timer: "105",
  });

  useEffect(() => {
    setCandidateId(midi?.inputs.find((input) => input.selected)?.id ?? "");
  }, [midi]);

  useEffect(() => {
    if (!settings) return;
    const midiSettings = settings.settings.midi;
    setChannel(String(midiSettings.channel));
    setNote(String(midiSettings.note));
    setDebounce(String(midiSettings.debounce_ms));
    setVelocities(
      Object.fromEntries(
        cues.map(([cue]) => [cue, String(midiSettings.mappings[cue] ?? "")]),
      ) as Record<MidiCueName, string>,
    );
  }, [settings]);

  const parsedSettings = useMemo<MidiSettingsInput | null>(() => {
    if (!settings) return null;
    const parsedChannel = Number(channel);
    const parsedNote = Number(note);
    const parsedDebounce = Number(debounce);
    const parsedVelocities = Object.fromEntries(
      cues.map(([cue]) => [cue, Number(velocities[cue])]),
    ) as Record<MidiCueName, number>;
    if (!Number.isInteger(parsedChannel) || parsedChannel < 1 || parsedChannel > 16) return null;
    if (!Number.isInteger(parsedNote) || parsedNote < 0 || parsedNote > 127) return null;
    if (!Number.isInteger(parsedDebounce) || parsedDebounce < 0 || parsedDebounce > 2000) return null;
    if (Object.values(parsedVelocities).some((value) => !Number.isInteger(value) || value < 100 || value > 127)) return null;
    if (new Set(Object.values(parsedVelocities)).size !== cues.length) return null;
    return {
      ...settings.settings.midi,
      enabled: true,
      channel: parsedChannel,
      note: parsedNote,
      debounce_ms: parsedDebounce,
      mappings: parsedVelocities,
    };
  }, [channel, debounce, note, settings, velocities]);

  const midiView = draft.method !== "playback_api";
  const savedMidi = settings?.settings.midi;
  const midiCount = !savedMidi ? 0
    : Number(channel !== String(savedMidi.channel))
      + Number(note !== String(savedMidi.note))
      + Number(debounce !== String(savedMidi.debounce_ms))
      + cues.filter(([cue]) => velocities[cue] !== String(savedMidi.mappings[cue] ?? "")).length;
  const midiDirty = midiCount > 0;
  const midiHeld = midiView ? midiDirty : false;
  const heldVelocity = (cue: MidiCueName) => midiView && Boolean(savedMidi) && velocities[cue] !== String(savedMidi!.mappings[cue] ?? "");
  const revertMidi = useCallback(() => {
    if (!settings) return;
    const saved = settings.settings.midi;
    setChannel(String(saved.channel));
    setNote(String(saved.note));
    setDebounce(String(saved.debounce_ms));
    setVelocities(Object.fromEntries(cues.map(([cue]) => [cue, String(saved.mappings[cue] ?? "")])) as Record<MidiCueName, string>);
  }, [settings]);
  const applyMidi = useCallback(async () => {
    if (!parsedSettings) return false;
    return (await onSaveSettings(parsedSettings)) !== false;
  }, [onSaveSettings, parsedSettings]);
  const save = usePlaybackSave(draft, playback, {
    // Choosing MIDI while the saved MIDI input is off also writes the MIDI settings (enabled), as the old Save MIDI settings did after selectSource.
    pending: midiView && (midiDirty || (draft.methodDirty && savedMidi?.enabled === false && parsedSettings !== null)),
    count: midiHeld ? midiCount : 0,
    valid: parsedSettings !== null,
    snapshot: midiView ? JSON.stringify(parsedSettings) : "",
    apply: applyMidi,
    revert: revertMidi,
  });
  useEffect(() => { onDirtyChange?.(save.dirty); }, [save.dirty, onDirtyChange]);
  const enabled = midi?.enabled ?? false;
  const selectedInput = midi?.inputs.find((input) => input.selected) ?? null;
  const connectedInput = midi?.inputs.find((input) => input.connected) ?? null;
  const controlsPending = pendingOperation !== null;
  const connectionStatus = playback?.status ? playback.status.connected ? "connected" : "disconnected"
    : playback ? "loading" : connectedInput ? "connected" : midi ? "disconnected" : "loading";

  return (
    <Panel
      aria-busy={controlsPending}
      aria-labelledby="midi-setup-heading"
      className="mt-5"
      id="midi-configuration"
      tone="playback"
      variant="setup"
    >
      <SetupPanelHeader
        closeLabel="Close MIDI / Playback configuration"
        description="Receives song and action cues from Playback to control StagePilot."
        headingId="midi-setup-heading"
        onClose={onClose}
        status={connectionStatus}
        statusDetail={savedMethod === "real" ? "MIDI · Alternate" : undefined}
        title="Playback"
      />

      <ConnectionMethod
        disabled={!playback || Boolean(playback.pending) || playback.status?.discovery === "running"}
        heldChange={draft.methodDirty}
        onChange={(value) => draft.setMethod(value === "real" ? "real" : "playback_api")}
        options={[
          { value: "playback_api", label: "Playback API", role: "primary" },
          { value: "real", label: "MIDI", role: "alternate" },
        ]}
        value={draft.method}
      />
      {draft.method === "playback_api" && <PlaybackApiPanel playback={playback} draft={draft} />}
      {draft.method !== "playback_api" && <div id="playback-midi-method">
        {playback?.status && <p className="mt-2 text-sm text-ink-body">{playbackMidiNote(playback.status)}</p>}

      <div className="mt-4">
        <Disclosure summary="Song order and velocity map" title="Playback song velocities" tone="playback" variant="advanced">
        <div className="mt-2 rounded-ds-lg bg-integration-playback-soft/10 p-3">
          <p className="mt-1 text-xs text-ink-body">
            Set each song-start cue to a velocity corresponding with its position in the service plan. Velocities 1–99 correspond to the songs in order; 100–127 are reserved for action cues.
          </p>
          {songs.length > 0 ? (
            <>
              <ol className="mt-3 grid gap-1.5 sm:grid-cols-2">
                {songs.slice(0, 99).map((song, index) => (
                  <li className="flex items-center gap-2 text-xs text-ink-body" key={`${song.id}-${index}`}>
                    <span className="inline-flex min-w-8 justify-center rounded-ds-lg bg-integration-playback-soft/20 px-1.5 py-0.5 font-mono font-bold tabular-nums text-integration-playback-text">
                      {index + 1}
                    </span>
                    <span className="truncate">{song.title}</span>
                  </li>
                ))}
              </ol>
              {songs.length > 99 && (
                <p className="mt-3 text-xs text-danger-text">
                  This service plan has more than 99 songs. Positions after 99 cannot be addressed by MIDI velocity.
                </p>
              )}
            </>
          ) : (
            <p className="mt-2 text-xs text-warning-text">Load a service plan to see its song-to-velocity map.</p>
          )}
        </div>
        <div className="grid gap-3 sm:grid-cols-3">
          <Field heldChange={midiView && Boolean(savedMidi) && channel !== String(savedMidi!.channel)} label="MIDI channel">
            <Input className="tabular-nums" disabled={pendingSettingsSave} inputMode="numeric" onChange={(event) => setChannel(event.target.value)} value={channel} />
          </Field>
          <Field heldChange={midiView && Boolean(savedMidi) && note !== String(savedMidi!.note)} label="Fixed note">
            <Select
              disabled={pendingSettingsSave}
              onChange={(event) => setNote(event.target.value)}
              value={note}
            >
              {midiNoteOptions.map((option) => (
                <option key={option.note} value={option.note}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Field>
          <Field heldChange={midiView && Boolean(savedMidi) && debounce !== String(savedMidi!.debounce_ms)} label="Debounce (ms)">
            <Input className="tabular-nums" disabled={pendingSettingsSave} inputMode="numeric" onChange={(event) => setDebounce(event.target.value)} value={debounce} />
          </Field>
        </div>
        <div className="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {cues.map(([cue, label]) => (
            <Field heldChange={heldVelocity(cue)} key={cue} label={`${label} velocity`}>
              <Input
                aria-label={`${label} velocity`}
                className="tabular-nums"
                disabled={pendingSettingsSave}
                inputMode="numeric"
                min={100}
                max={127}
                onChange={(event) => setVelocities((current) => ({ ...current, [cue]: event.target.value }))}
                value={velocities[cue]}
              />
            </Field>
          ))}
        </div>
        <p className="mt-3 text-xs text-ink-muted">Saving enables real MIDI input. Action velocities must be unique and between 100 and 127.</p>
        {midi && !enabled && (
          <InlineAlert className="mt-3" tone="warning">
            Save the MIDI settings above, then restart StagePilot to discover and connect the input device.
          </InlineAlert>
        )}
        </Disclosure>
      </div>

      {(settingsError || settingsMessage) && (
        <InlineAlert className="mt-3" tone={settingsError ? "danger" : "info"}>
          {settingsError ?? settingsMessage}
        </InlineAlert>
      )}

      <div className="mt-4">
        <div>
          {!midi && !error && (
            <p className="rounded-ds-lg bg-edge-subtle px-3 py-3 text-sm text-ink-muted">
              Loading MIDI configuration…
            </p>
          )}

          <label className="mt-3 block text-xs font-bold uppercase tracking-wider text-ink-muted" htmlFor="midi-input">
            Available input
          </label>
          <div className="mt-2 flex flex-col gap-2 sm:flex-row">
            <div className="min-w-0 flex-1">
            <Select
              disabled={!enabled || controlsPending || !midi?.inputs.length}
              id="midi-input"
              onChange={(event) => setCandidateId(event.target.value)}
              value={candidateId}
            >
              <option value="">Choose a MIDI input</option>
              {midi?.inputs.map((input) => (
                <option disabled={input.ambiguous} key={input.id} value={input.id}>
                  {input.name}{input.ambiguous ? " (duplicate name)" : input.connected ? " (connected)" : ""}
                </option>
              ))}
            </Select>
            </div>
            <Button disabled={controlsPending} onClick={onRefresh} tone="playback" variant="tinted">
              {pendingOperation === "refresh" ? "Refreshing…" : "Refresh inputs"}
            </Button>
            <Button
              disabled={!enabled || controlsPending || !candidateId || candidateId === selectedInput?.id}
              onClick={() => onSelect(candidateId)}
              variant="primary"
            >
              {pendingOperation === "connect" ? "Connecting…" : "Connect"}
            </Button>
            <Button
              disabled={!enabled || controlsPending || !midi?.selected_input_name}
              onClick={() => onSelect(null)}
              tone="playback"
              variant="tinted"
            >
              {pendingOperation === "disconnect" ? "Disconnecting…" : "Disconnect"}
            </Button>
          </div>

          <div className="mt-3 flex flex-wrap gap-2 text-xs">
            <span className={`rounded-ds-full px-2.5 py-1 font-bold ${connectedInput ? "bg-integration-playback-soft/20 text-integration-playback-text" : "bg-edge-subtle text-ink-muted"}`}>
              {playback?.status?.sources.midi.connected && playback.status.active_source === "playback_api" ? `Connected: ${connectedInput?.name ?? "MIDI"} — standing by` : connectedInput ? `Connected: ${connectedInput.name}` : "No input connected"}
            </span>
            {midi?.selected_input_name && !connectedInput && (
              <span className="rounded-ds-full bg-warning/10 px-2.5 py-1 font-bold text-warning-text">
                Waiting for {midi.selected_input_name}
              </span>
            )}
            {midi?.configured_input_name && (
              <span className="rounded-ds-full bg-edge-subtle px-2.5 py-1 text-ink-muted">
                Startup default: {midi.configured_input_name}
              </span>
            )}
          </div>

          <PanelAlert className="mt-3" error={error} message={message} messageTone="info" onRetry={!midi ? onRetryLoad : undefined} />
        </div>

      </div>

      <div className="mt-4 overflow-hidden rounded-ds-lg bg-surface-950/30">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-edge-subtle px-3 py-2.5">
          <div>
            <p className="text-xs font-bold uppercase tracking-wider text-ink-body">
              Live MIDI note monitor
            </p>
            <p className="mt-0.5 text-xs text-ink-muted">
              Shows note-on and note-off messages received from the selected input.
            </p>
          </div>
          <span className="rounded-ds-full bg-edge-subtle px-2.5 py-1 text-xs tabular-nums text-ink-muted">
            {messages.length} recent
          </span>
        </div>
        {messages.length === 0 ? (
          <p className="px-3 py-4 text-sm text-ink-muted">
            No MIDI notes received yet. Send a cue from Playback; an empty monitor points to port or routing configuration.
          </p>
        ) : (
          <div className="max-h-64 overflow-auto overscroll-contain">
            <table className="w-full min-w-[720px] text-left text-xs">
              <thead className="sticky top-0 bg-surface-950 text-ink-muted">
                <tr>
                  <th className="px-3 py-2 font-semibold">Time</th>
                  <th className="px-3 py-2 font-semibold">Input</th>
                  <th className="px-3 py-2 font-semibold">Message</th>
                  <th className="px-3 py-2 font-semibold">Channel</th>
                  <th className="px-3 py-2 font-semibold">Note</th>
                  <th className="px-3 py-2 font-semibold">Velocity</th>
                  <th className="px-3 py-2 font-semibold">Result</th>
                </tr>
              </thead>
              <tbody>
                {messages.map((item, index) => (
                  <tr
                    className="border-t border-edge-subtle text-ink-body"
                    key={`${item.timestamp}-${item.channel}-${item.note}-${index}`}
                  >
                    <td className="whitespace-nowrap px-3 py-2 font-mono tabular-nums text-ink-muted">
                      {new Date(item.timestamp).toLocaleTimeString()}
                    </td>
                    <td className="max-w-52 truncate px-3 py-2">
                      {item.input_name ?? "Unknown input"}
                    </td>
                    <td className="whitespace-nowrap px-3 py-2 font-mono">
                      {item.message_type}
                    </td>
                    <td className="px-3 py-2 font-mono tabular-nums">{item.channel}</td>
                    <td className="whitespace-nowrap px-3 py-2 font-mono">
                      {item.note_name} ({item.note})
                    </td>
                    <td className="px-3 py-2 font-mono tabular-nums">{item.velocity}</td>
                    <td className={`px-3 py-2 ${monitorTone(item)}`} title={item.detail}>
                      {item.disposition.replaceAll("_", " ")}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
      </div>}
      <PlaybackSaveBar save={save} />
    </Panel>
  );
}
