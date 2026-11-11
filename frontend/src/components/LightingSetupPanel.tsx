import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";

import type {
  ApplicationState,
  LightingCaptureResponse,
  LightingCue,
  LightingEffect,
  LightingMethod,
  LightingPad,
  LightkeyStatus,
  OscArgument,
  OscLightingCue,
  LightsSettingsInput,
  LightsStatusResponse,
  ConnectionStatus,
  SettingsResponse,
  Song,
} from "../types";
import {
  clearAllRequest,
  clearLoudnessRequest,
  clearSongRequest,
  countCueMap,
  sumCounts,
  type ClearCuesRequest,
} from "./lightingCueConfirmations";
import { ClearCuesDialog } from "./ClearCuesDialog";
import { SetupPanelHeader } from "./SetupPanelHeader";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { PanelAlert } from "./PanelAlert";
import { Input } from "./ui/Input";
import { Panel } from "./ui/Panel";
import { SaveBar, type SaveBarState } from "./ui/SaveBar";
import { useSaveAttempt } from "../hooks/useSaveAttempt";
import { Disclosure } from "./ui/Disclosure";
import { accentClasses } from "./ui/accent";
import { ScrollRegion } from "./ui/ScrollRegion";
import { Select } from "./ui/Select";
import { ConnectionMethod } from "./ui/ConnectionMethod";
import { EffectRow, LookRow, OscArgumentRows, OscConnectionFields, OscCueRow } from "./LightingOscParts";
import { Dialog } from "./ui/Dialog";
import { fadeOf, isRamp, mergeBlockReason, rampDescription, RowValueIndex } from "./lightingSmooth";
import {
  capturedCount, formatCount, hasEditedCaptured, replayBusyNote, isCaptured, isDraftRow, isLook, lookLabel, markEdited, mergeRows, rowMessages,
  removeFixtureFromRows,
  captureLabelOf,
} from "./lightingLooks";
import { projectMismatchText } from "./lightkeyProject";
import { usePersistedFlag } from "../hooks/usePersistedFlag";
import { LightingCueBoard, type CaptureMode } from "./LightingCueBoard";
import { CueTypeBadge } from "./CueTypeParts";
import { useHiddenCueTypes } from "./useHiddenCueTypes";
import type { CueTypeId } from "./lightingCueTypes";
import { classifyMidiCue, classifyOscCue, midiNoteName, midiSpokenLabel, typeSpokenLabel } from "./lightingCueTypes";
import { LightingTimeline } from "./LightingTimeline";
import { LightkeyStrip } from "./LightkeyStrip";
import { SettingsNoticeBanner } from "./SettingsNoticeBanner";
import { SongsStrip, type SongChip } from "./SongsStrip";
import { CONTROL_34 } from "./compactControl";
import { CONTROL_40, shortLightingMessage } from "./liveLink";
import { confirmMergeChangesValues, confirmRemoveFixture } from "./lightingCueConfirmations";
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
  /** Removes the selected song's loudness envelope (nothing else). */
  onClearLoudness?: (songKey: string) => Promise<boolean>;
  onSavePads: (pads: LightingPad[]) => Promise<boolean>;
}

export const LIGHTING_TIMELINE_OPEN_KEY = "stagepilot.lighting.timeline-open";
export const LIGHTING_CUE_BOARD_OPEN_KEY = "stagepilot.lighting.cue-board-open";

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
  onClearLoudness,
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
  onSaveCues: (song: Song, cues: LightingCue[] | OscLightingCue[], method: LightingMethod, options?: { removeEffectIds?: string[] }) => void;
  onClearAllCues?: (songs: Song[], method: LightingMethod) => void;
} & Partial<LightingPanelActions>) {
  const testReasonId = useId();
  const songs = useMemo(() => state.plan?.songs ?? [], [state.plan?.songs]);
  // Only a song that is actually playing moves the follow. When a song ends, current_song may advance to the next one;
  // that must not drop a hand pick (the last playing song is remembered below).
  const liveSong = state.playing_song;
  const liveKey = liveSong ? songKey(liveSong) : "";
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
  // The timeline follows the live song, or the last one that played. A hand pick holds until the next song starts.
  const [lastLiveKey, setLastLiveKey] = useState(liveKey);
  const [manualSongKey, setManualSongKey] = useState<string | null>(null);
  useEffect(() => {
    if (!liveKey || liveKey === lastLiveKey) return;
    setLastLiveKey(liveKey);
    setManualSongKey(null);
  }, [liveKey, lastLiveKey]);
  const hasSong = (key: string | null): key is string => !!key && songs.some((song) => songKey(song) === key);
  const followKey = hasSong(lastLiveKey) ? lastLiveKey : null;
  const selectedSongKey = hasSong(manualSongKey)
    ? manualSongKey
    : (followKey ?? (songs[0] ? songKey(songs[0]) : ""));
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

  const selectedSong = songs.find((song) => songKey(song) === selectedSongKey) ?? null;
  const savedCueMap = selectedSongKey
    ? settings?.settings.lights.cue_maps[selectedSongKey]
    : undefined;

  // Rows still being typed (no address yet) are kept across a refresh of the saved map and across the follow moving to
  // another song; each song keeps its own (F-03).
  const draftsBySong = useRef<Record<string, OscLightingCue[]>>({});
  const shownSongKey = useRef(selectedSongKey);
  const oscCuesNow = useRef<OscLightingCue[]>([]);
  oscCuesNow.current = oscCues;
  useEffect(() => {
    setCues(savedCueMap?.cues.map((cue) => ({ ...cue })) ?? []);
    const saved = savedCueMap?.osc_cues?.map((cue) => ({ ...cue, args: cue.args.map((arg) => ({ ...arg })) })) ?? [];
    let drafts = oscCuesNow.current.filter(isDraftRow);
    if (shownSongKey.current !== selectedSongKey) {
      draftsBySong.current[shownSongKey.current] = drafts;
      shownSongKey.current = selectedSongKey;
      drafts = draftsBySong.current[selectedSongKey] ?? [];
      delete draftsBySong.current[selectedSongKey];
    }
    const draftIds = new Set(drafts.map((cue) => cue.id));
    setOscCues([...saved.filter((cue) => !draftIds.has(cue.id)), ...drafts]);
  }, [savedCueMap, selectedSongKey]);

  // ---- Cue board, live capture and timeline ----
  const pads = settings?.settings.lights.pads ?? [];
  const [captureMode, setCaptureMode] = useState<CaptureMode>("capture");
  // The Lightkey project that is open now (the strip reads it); the song's timeline compares it with the project the song was captured under.
  const [openProject, setOpenProject] = useState<string | null>(null);
  const onLightkeyStatus = useCallback((status: LightkeyStatus) => setOpenProject(status.project ?? null), []);
  const [captureStatus, setCaptureStatus] = useState<string | null>(null);
  const [lastCapture, setLastCapture] = useState<{ songKey: string; cueId: string } | null>(null);
  const [boardBusy, setBoardBusy] = useState(false);
  const boardBusyRef = useRef(false);
  const [clock, setClock] = useState(() => Date.now());
  const playingSong = state.playing_song ?? state.current_song;
  const timerRunning = state.timer?.status === "running" && Boolean(state.timer?.started_at);
  // The whole song-timeline group (song picker, cue board, timeline, cue list) folds away for people who do not use it. It starts
  // open and then stays however the user leaves it, across restarts (owner, 2026-10-15).
  const [timelineOpen, setTimelineOpen] = usePersistedFlag(LIGHTING_TIMELINE_OPEN_KEY, true);
  // The Cue board folds away on its own and is remembered on this device; it starts open.
  const [cueBoardOpen, setCueBoardOpen] = usePersistedFlag(LIGHTING_CUE_BOARD_OPEN_KEY, true);
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

  // One step of undo for merge, simplify and remove-fixture. Any other edit forgets it.
  const [undo, setUndo] = useState<{ cues: OscLightingCue[]; text: string } | null>(null);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  useEffect(() => { setUndo(null); setSelectedIds([]); setPage(0); setFocusId(null); setPendingRemoveEffect(null); }, [selectedSongKey]);
  useEffect(() => {
    setSelectedIds((ids) => {
      if (!ids.length) return ids;
      const present = new Set(oscCues.map((cue) => cue.id));
      return ids.every((id) => present.has(id)) ? ids : ids.filter((id) => present.has(id));
    });
  }, [oscCues]);

  const persistOscCues = (nextCues: OscLightingCue[], keepUndo = false, options?: { removeEffectIds?: string[] }) => {
    if (!keepUndo) setUndo(null);
    const sortedCues = [...nextCues].sort((left, right) => left.at_seconds - right.at_seconds);
    setOscCues(sortedCues);
    if (selectedSong) { if (options) onSaveCues(selectedSong, sortedCues, "osc", options); else onSaveCues(selectedSong, sortedCues, "osc"); }
  };

  // The song's loops come from the saved map only: rows never carry them, and a row save never changes them.
  const effects: LightingEffect[] = savedCueMap?.effects ?? [];
  const [pendingRemoveEffect, setPendingRemoveEffect] = useState<string | null>(null);
  const effectToRemove = effects.find((effect) => effect.id === pendingRemoveEffect) ?? null;
  const removeEffect = (id: string) => {
    persistOscCues(oscCues, true, { removeEffectIds: [id] });
    setPendingRemoveEffect(null);
    setFocusId((current) => (current === id ? null : current));
  };

  // Timeline edits go through the same save as the cue rows, so the two stay in sync.
  const moveCue = (id: string, at: number) => {
    if (method === "osc") persistOscCues(oscCues.map((cue) => cue.id === id ? markEdited({ ...cue, at_seconds: at }) : cue));
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

  // A look row changed by hand. A new message has no address yet, so it is only shown until the address is valid.
  const commitLook = (next: OscLightingCue) => {
    const messages = next.messages ?? [];
    if (!messages.length) { persistOscCues(oscCues.filter((cue) => cue.id !== next.id)); return; }
    const edited = markEdited(next);
    if (messages.some((message) => message.address === "")) setOscCues(oscCues.map((cue) => cue.id === next.id ? edited : cue));
    else persistOscCues(oscCues.map((cue) => cue.id === next.id ? edited : cue));
  };
  const mergeBlocked = mergeBlockReason(oscCues, selectedIds);
  const mergeSelected = (ids: string[]) => {
    if (mergeBlockReason(oscCues, ids)) return;
    const result = mergeRows(oscCues, ids);
    if (!result) return;
    if (result.changedAddresses.length && !confirmMergeChangesValues(result.changedAddresses)) return;
    const before = oscCues;
    persistOscCues(result.cues, true);
    setUndo({ cues: before, text: `Undo merge of ${ids.length} rows` });
    setSelectedIds([]);
  };
  const removeFixture = (fixture: string) => {
    if (!selectedSong || !confirmRemoveFixture(fixture, selectedSong.title)) return;
    const before = oscCues;
    persistOscCues(removeFixtureFromRows(oscCues, fixture), true);
    setUndo({ cues: before, text: `Undo removing ${fixture}` });
  };
  const undoLast = () => {
    if (!undo) return;
    persistOscCues(undo.cues, true);
    setUndo(null);
  };
  // The follow-along re-renders this panel ten times a second; anything that walks every row is kept until the rows change.
  const looks = useMemo(() => ({
    label: captureLabelOf(savedCueMap ? { ...savedCueMap, osc_cues: oscCues } : { osc_cues: oscCues }),
    count: capturedCount(oscCues),
    loops: effects.length,
    edited: hasEditedCaptured(oscCues) || Boolean(savedCueMap?.captured_edited),
    simplified: savedCueMap?.capture?.simplified ?? null,
    startLights: savedCueMap?.capture?.start_lights ?? null,
  }), [savedCueMap, oscCues, effects.length]);
  const rowIndex = useMemo(() => new RowValueIndex(oscCues), [oscCues]);
  // Warn only: replay is never held or skipped for this.
  const projectNotice = projectMismatchText(savedCueMap?.capture?.project, openProject);
  const PAGE_SIZE = 40;

  const addOscCue = () => {
    persistOscCuesLocal([...oscCues, newOscCue(newCueId())]);
  };
  // A new OSC cue has no address yet, so it is only shown; it saves once its address is valid.
  const persistOscCuesLocal = (next: OscLightingCue[]) => setOscCues(next);

  // Every Clear waits here for the in-app permanent-erase dialog (never window.confirm).
  const [pendingClear, setPendingClear] = useState<ClearCuesRequest | null>(null);
  const cueMaps = settings?.settings.lights.cue_maps;

  // Clearing a song's OSC cues clears its loops too; the MIDI clear sends nothing about loops.
  const saveEmpty = (song: Song, forMethod: LightingMethod, loops: LightingEffect[] = []) => {
    if (forMethod === "osc") onSaveCues(song, [], forMethod, { removeEffectIds: loops.map((effect) => effect.id) });
    else onSaveCues(song, [], forMethod);
  };

  const clearSelectedSongCues = () => {
    if (!selectedSong || !shownCues.length) return;
    setPendingClear(clearSongRequest(selectedSong.title, countCueMap(savedCueMap, method), () => {
      if (method === "osc") persistOscCues([], false, { removeEffectIds: effects.map((effect) => effect.id) });
      else persistCues([]);
    }));
  };

  const clearEverySongCues = () => {
    if (!songs.length) return;
    const counts = sumCounts(songs.map((song) => countCueMap(cueMaps?.[songKey(song)], method)));
    setPendingClear(clearAllRequest(songs.map((song) => song.title), state.plan?.title ?? null, counts, () => {
      if (method === "osc") setOscCues([]);
      else setCues([]);
      if (onClearAllCues) onClearAllCues(songs, method);
      else songs.forEach((song) => saveEmpty(song, method, cueMaps?.[songKey(song)]?.effects));
    }));
  };

  // The x on a song chip: the same permanent-erase dialog, for that song only.
  const clearSongByKey = (key: string) => {
    const song = songs.find((item) => songKey(item) === key);
    if (!song) return;
    if (key === selectedSongKey && shownCues.length) { clearSelectedSongCues(); return; }
    const counts = countCueMap(cueMaps?.[key], method);
    if (counts.looks + counts.handmade + (counts.loops ?? 0) === 0) return;
    setPendingClear(clearSongRequest(song.title, counts, () => saveEmpty(song, method, cueMaps?.[key]?.effects)));
  };
  const songChips: SongChip[] = useMemo(() => songs.map((song) => {
    const map = cueMaps?.[songKey(song)];
    const counts = countCueMap(map, method);
    return { key: songKey(song), title: song.title, order: song.order, looks: counts.looks, loops: counts.loops ?? 0, handmade: counts.handmade, hasLoudness: Boolean(map?.loudness), label: captureLabelOf(map) };
  }), [songs, cueMaps, method]);

  const askClearLoudness = () => {
    if (!onClearLoudness || !selectedSongKey) return;
    const key = selectedSongKey;
    setPendingClear(clearLoudnessRequest(selectedSong?.title ?? savedCueMap?.song_title ?? "this song", () => void onClearLoudness(key)));
  };

  // Hidden types only change what is drawn and listed; the cue maps and replay never see them.
  const typeFilter = useHiddenCueTypes();
  const keepRows = new Set([...selectedIds, ...(focusId ? [focusId] : [])]);
  const rowVisible = (id: string, type: CueTypeId) => !typeFilter.hidden.has(type) || keepRows.has(id);
  const keepKey = [...keepRows].join("|");
  const listedOsc = useMemo(
    () => oscCues.filter((cue) => rowVisible(cue.id, classifyOscCue(cue))),
    [oscCues, typeFilter.hidden, keepKey], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const listedEffects = effects.filter((effect) => rowVisible(effect.id, "effect-loop"));
  const listedMidi = cues.filter((cue) => rowVisible(cue.id, classifyMidiCue(cue).type));
  const pageCount = Math.max(1, Math.ceil(listedOsc.length / PAGE_SIZE));
  const safePage = Math.min(page, pageCount - 1);
  const shownPage = listedOsc.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE);
  useEffect(() => {
    if (!focusId) return;
    const index = listedOsc.findIndex((cue) => cue.id === focusId);
    if (index >= 0) setPage(Math.floor(index / PAGE_SIZE));
    document.getElementById(`look-row-${focusId}`)?.scrollIntoView?.({ block: "nearest" });
  }, [focusId]); // eslint-disable-line react-hooks/exhaustive-deps
  const shownCues = method === "osc" ? [...listedEffects, ...listedOsc] : listedMidi;
  const songsWithCues = Object.values(settings?.settings.lights.cue_maps ?? {}).filter(
    (map) => (method === "osc" ? (map.osc_cues?.length ?? 0) : (map.cues?.length ?? 0)) > 0,
  ).length;
  const cueTotal = Object.values(settings?.settings.lights.cue_maps ?? {}).reduce(
    (total, map) => total + (method === "osc" ? (map.osc_cues?.length ?? 0) : (map.cues?.length ?? 0)),
    0,
  );
  const activePads = pads.filter((pad) => pad.method === method).length;
  // The line under the title, shown open or closed: what is in here, so the closed card still tells the story.
  const timelineDetail = [
    cueTotal === 0 ? "No cues yet" : `${formatCount(cueTotal)} cue${cueTotal === 1 ? "" : "s"} on ${songsWithCues} of ${songs.length} song${songs.length === 1 ? "" : "s"}`,
    `${activePads} pad${activePads === 1 ? "" : "s"}`,
    selectedSong && selectedSong.duration_seconds != null ? `${selectedSong.title} ${formatElapsed(selectedSong.duration_seconds)}` : null,
  ].filter(Boolean).join(" · ");
  // Only the method on screen is drawn; cues of the other method stay saved.
  const timelineMarkers: TimelineMarker[] = useMemo(() => method === "osc"
    ? oscCues.filter((cue) => cue.address !== "" || isLook(cue)).map((cue) => {
      const type = classifyOscCue(cue);
      const label = isLook(cue) ? lookLabel(cue) : cue.label || cue.address;
      const ramp = isRamp(cue);
      return {
        id: cue.id, at: cue.at_seconds, label, color: cue.color ?? null, captured: isCaptured(cue), type, spokenLabel: typeSpokenLabel(type, label),
        ...(ramp ? { fade: fadeOf(cue), description: rampDescription(cue, rowIndex) } : {}),
        weight: rowMessages(cue).length,
      };
    })
    : cues.map((cue) => {
      const { type, sharp } = classifyMidiCue(cue);
      const text = cue.label || `Note ${cue.note}`;
      return {
        id: cue.id, at: cue.at_seconds, color: cue.color ?? null, type, sharp,
        label: noteDisplayMode === "music" ? `${midiNoteName(cue.note)} ${text}` : `${cue.note} ${text}`,
        spokenLabel: midiSpokenLabel(cue.note, text),
      };
    }), [method, oscCues, cues, rowIndex, noteDisplayMode]);
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
  // The controls wear the chosen method (OSC keeps the lighting colour, MIDI the Playback MIDI blue-violet); the title and the
  // panel's own top border and wash stay lighting (owner, 2026-10-16), the same split the MIDI / Playback panel uses.
  const accent = method === "midi" ? "midi" : "lighting";
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

  const loudnessProp = {
    present: Boolean(savedCueMap?.loudness),
    disabled: pendingOperation !== null,
    onClear: onClearLoudness && selectedSongKey ? askClearLoudness : undefined,
  };
  const testReasonText = method === "osc" ? "Save the OSC settings first. Test cue sends with the saved settings." : "Save the new output first. Test cue sends to the saved output.";

  // The OSC test cue lives in the Live Link block's strip ("Send a test cue"), folded away until it is wanted. Same fields, checks and sending as before.
  const oscTestCue = (
    <div aria-label="OSC test cue" className="lighting-osc-test grid gap-2" role="group">
      <div className="flex flex-wrap items-end gap-x-2.5 gap-y-2">
        <Field className="live-link-host" label="OSC address">
          <Input aria-describedby={oscTestAddress && oscTestAddressError ? "osc-test-cue-hint osc-test-cue-error" : "osc-test-cue-hint"} autoCapitalize="none" autoCorrect="off" className={`font-mono ${CONTROL_40}`} error={Boolean(oscTestAddress && oscTestAddressError)} onChange={(event) => setOscTestAddress(event.target.value)} placeholder="/live/*/cue/Stars/activate" spellCheck={false} value={oscTestAddress} />
        </Field>
        <OscArgumentRows drafts={oscTestArgs} inline label="Test argument" onChange={setOscTestArgs} />
        <Button
          aria-describedby={draftDiffersFromSaved ? testReasonId : undefined}
          className={CONTROL_40}
          disabled={pendingOperation !== null || draftDiffersFromSaved || savedMethod !== "osc" || Boolean(oscTestAddressError) || Boolean(oscTestParsed.error)}
          onClick={() => onTestOsc?.(oscTestAddress.trim(), oscTestParsed.args)}
          variant="tinted"
        >
          {pendingOperation === "test" ? "Sending…" : "Send test cue"}
        </Button>
      </div>
      <p className="text-type-body-sm text-ink-muted" id="osc-test-cue-hint">The cue&apos;s OSC path, for example /live/*/cue/Stars/activate. The host is set above.</p>
      {oscTestAddress && oscTestAddressError && <p className="text-type-body-sm text-danger-text" id="osc-test-cue-error" role="alert">{oscTestAddressError}</p>}
      {oscTestParsed.error && <p className="text-type-body-sm text-danger-text" role="alert">{oscTestParsed.error}</p>}
      {draftDiffersFromSaved && <p className="text-type-body-sm text-ink-muted">Save the changes above first, then send a test cue.</p>}
    </div>
  );
  const midiTestCue = (
    <Disclosure className="mt-4" tone="midi" title="Send a test cue" variant="advanced">
      <div aria-label="Test cue" className="mt-3 flex flex-wrap items-end gap-x-3 gap-y-2" role="group">
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
        <Button className="flex-none" aria-describedby={draftDiffersFromSaved ? testReasonId : undefined} disabled={lights?.connection_status !== "connected" || pendingOperation !== null || draftDiffersFromSaved} onClick={() => onTest(Number(testNote), Number(testVelocity))} variant="tinted">{pendingOperation === "test" ? "Sending…" : "Send test cue"}</Button>
        {draftDiffersFromSaved && <p className="w-full text-type-body-sm text-ink-muted">Save the changes above first, then send a test cue.</p>}
      </div>
    </Disclosure>
  );
  // Under the Live Link block's strip. The "ready" state is NOT repeated here (the banner above and the header pill say it once): only what
  // is new (an error, the last cue sent) or what has no other home.
  const oscStatusLine = savedMethod === "osc" && lights?.connection_status === "error"
    ? lights.detail
    : savedMethod === "osc" && lights?.last_osc_cue
      ? `Last sent: ${lights.last_osc_cue.address}${lights.last_cue_at ? ` at ${new Date(lights.last_cue_at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit", second: "2-digit" })}` : ""}. UDP does not confirm delivery.`
      : savedMethod === "osc" && lights?.connection_status === "ready"
        ? null
        : savedMethod === "osc" ? null : "OSC is sent over UDP, so StagePilot cannot confirm a cue arrived. Use Send test cue to check.";
  const replayBusy = replayBusyNote(lights?.replay_load);
  const oscFooter = (
    <>
      {oscStatusLine && <p className={`mt-2 text-type-body-sm ${savedMethod === "osc" && lights?.connection_status === "error" ? "text-danger-text" : "text-ink-muted"}`} data-testid="osc-status">{oscStatusLine}</p>}
      {replayBusy && <p className="mt-2 text-type-body-sm text-ink-muted" data-testid="replay-busy">{replayBusy}</p>}
      {heldMethod && <p className="mt-2 text-type-body-sm text-ink-muted">MIDI cues stay saved and send nothing while OSC is the saved method.</p>}
    </>
  );

  // One toolbar row when wide, wrapping when narrow: every control is 34 px (44 on touch).
  const toolbarNode = (
    <>
      <div className="flex min-w-0 flex-wrap items-end gap-x-3 gap-y-2">
        <Field className="w-full sm:w-64" label="Song">
          <Select className={CONTROL_34} onChange={(event) => setManualSongKey(event.target.value)} value={selectedSongKey}>
            {!songs.length && <option value="">Load a service plan first</option>}
            {songs.map((song) => <option key={songKey(song)} value={songKey(song)}>{song.order}. {song.title}</option>)}
          </Select>
        </Field>
        {followKey && (
          <p className="flex min-h-[34px] items-center gap-2 text-type-body-sm text-ink-muted" data-testid="lighting-follow-status">
            {selectedSongKey === followKey ? "Following live song" : `Showing ${selectedSong?.title ?? "another song"}`}
            {selectedSongKey !== followKey && (
              <Button className={CONTROL_34} onClick={() => setManualSongKey(null)} variant="secondary">Follow live</Button>
            )}
          </p>
        )}
      </div>
      {method === "midi" && (
        <div className="flex min-h-[34px] items-center gap-2" aria-label="MIDI note display">
          <span aria-hidden="true" className={noteDisplayMode === "music" ? "text-integration-playback-midi-text" : "text-ink-muted"}>♪</span>
          <button
            aria-checked={noteDisplayMode === "numeric"}
            aria-label={`MIDI note display: ${noteDisplayMode === "music" ? "music notation" : "numeric value"}`}
            className={`relative h-7 w-12 shrink-0 cursor-pointer touch-manipulation rounded-ds-full border border-integration-playback-midi-soft/30 transition-colors duration-[140ms] motion-reduce:transition-none can-hover:border-integration-playback-midi-soft/60 coarse:after:absolute coarse:after:inset-x-0 coarse:after:-inset-y-[9px] coarse:after:content-[''] ${noteDisplayMode === "numeric" ? "bg-integration-playback-midi-soft/20" : "bg-surface-900"}`}
            onClick={() => setNoteDisplayMode((current) => current === "music" ? "numeric" : "music")}
            role="switch"
            type="button"
          >
            <span
              aria-hidden="true"
              className={`absolute left-0 top-1 h-4 w-4 rounded-ds-full bg-integration-playback-midi-soft shadow-sm transition-transform duration-200 motion-reduce:transition-none ${noteDisplayMode === "numeric" ? "translate-x-7" : "translate-x-1"}`}
            />
          </button>
          <span aria-hidden="true" className={noteDisplayMode === "numeric" ? "text-integration-playback-midi-text" : "text-ink-muted"}>#</span>
        </div>
      )}
      <Button className={CONTROL_34} disabled={!selectedSong || pendingOperation !== null} onClick={method === "osc" ? addOscCue : addCue} variant="tinted">Add cue</Button>
      <Button className={CONTROL_34} disabled={!shownCues.length || pendingOperation !== null || !selectedSong} onClick={clearSelectedSongCues} variant="destructive">{pendingOperation === "save-cues" ? "Saving cues…" : "Clear song..."}</Button>
      {method === "osc" && (
        <>
          <Button
            className={CONTROL_34}
            disabled={selectedIds.length < 2 || pendingOperation !== null || Boolean(mergeBlocked)}
            onClick={() => mergeSelected(selectedIds)}
            title={mergeBlocked ?? "Tick two or more rows, or Shift-click markers, to merge them."}
            variant="tinted"
          >
            {selectedIds.length > 1 ? `Merge ${selectedIds.length} rows` : "Merge rows"}
          </Button>
          {undo && <Button className={CONTROL_34} onClick={undoLast} variant="secondary">{undo.text}</Button>}
        </>
      )}
    </>
  );

  return (
    <Panel accent={accent} className="mt-5" id="lighting-configuration" tone="lighting" variant="setup">
      <SetupPanelHeader
        closeLabel="Close Lighting configuration"
        description="Sends timed cues to your lighting software."
        onClose={onClose}
        status={connectionStatus ?? lights?.connection_status ?? "loading"}
        title="Lighting"
        titleTone="lighting"
        method={<ConnectionMethod
          placement="header"
        disabled={!lights && !settings}
        heldChange={heldMethod}
        onChange={(value) => setMethod(value === "midi" ? "midi" : "osc")}
        options={[
          { value: "osc", label: "OSC", role: "primary", chip: "border-integration-lighting-osc bg-integration-lighting-osc/45" },
          { value: "midi", label: "MIDI", role: "alternate", chip: "border-integration-playback-midi bg-integration-playback-midi/45" },
        ]}
        value={method}
        />}
      />


      <SettingsNoticeBanner settings={settings} where="lighting" />

      <PanelAlert className="mt-4 !py-1.5" dot error={error} message={message ? shortLightingMessage(message) : message} messageTone="info" neutral={Boolean(message && /^OSC /.test(message))} onRetry={!lights ? onRetryLoad : undefined} />

      {method === "osc" ? (
        savedMethod === "osc" ? (
          <LightkeyStrip
            connection={{ host: oscHost, onHost: setOscHost, oscPort, onPort: setOscPort, savedOscHost: savedLighting?.osc_host ?? "127.0.0.1", attempted: attempt.attempted, held: { host: heldHost, port: heldPort } }}
            lightingEnabled={Boolean(lights?.enabled)}
            looks={looks}
            onStatus={onLightkeyStatus}
            loudness={loudnessProp}
            footer={oscFooter}
            testCue={oscTestCue}
          />
        ) : (
          <OscConnectionFields
            attempted={attempt.attempted}
            held={{ host: heldHost, port: heldPort }}
            host={oscHost}
            onHost={setOscHost}
            onPort={setOscPort}
            port={oscPort}
            footer={oscFooter}
            testCue={oscTestCue}
          />
        )
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
        <Button disabled={pendingOperation !== null} onClick={onRefresh} variant="tinted">{pendingOperation === "refresh" ? "Refreshing…" : "Refresh outputs"}</Button>
      </div>
      {midiTestCue}
        </>
      )}
      {method === "midi" && savedMethod === "osc" && <LightkeyStrip
        lightingEnabled={Boolean(lights?.enabled)}
        looks={looks}
            onStatus={onLightkeyStatus}
        loudness={loudnessProp}
      />}
      <SongsStrip
        chips={songChips}
        liveKey={hasSong(liveKey) ? liveKey : ""}
        noun={method === "osc" ? "looks" : "cues"}
        onClear={clearSongByKey}
        onClearAll={clearEverySongCues}
        onSelect={setManualSongKey}
        selectedKey={selectedSongKey}
      />
      <Disclosure
        aside={(
          <span className={`rounded-ds-full border px-2.5 py-0.5 text-xs font-bold uppercase tracking-wide ${accentClasses[accent].banner}`}>
            {method === "osc" ? "OSC" : "MIDI"}
          </span>
        )}
        className="mt-6"
        detail={timelineDetail}
        onOpenChange={setTimelineOpen}
        open={timelineOpen}
        title="Song lighting timeline"
        tone={accent}
        variant="feature"
      >
        {!selectedSong && <div aria-label="Timeline tools" className="flex flex-wrap items-end gap-x-3 gap-y-2" role="toolbar">{toolbarNode}</div>}
        {draftDiffersFromSaved && <p className="mt-2 text-sm text-ink-muted" id={testReasonId}>{testReasonText}</p>}
        {selectedSong && projectNotice && <p className="mt-2 text-type-body-sm text-warning-text" data-testid="lighting-project-notice" role="status">{projectNotice}</p>}
        <p className="mt-2 text-type-body-sm text-ink-muted">Cue times are elapsed from song start. Cues for the other method stay saved. Changes save automatically. The timeline begins only after the Presentation countdown confirms it started.</p>

        {onSavePads && (
          <Disclosure className="mt-4" onOpenChange={setCueBoardOpen} open={cueBoardOpen} title="Cue board" tone={accent} variant="advanced">
          <div className="mt-3">
          <LightingCueBoard
            embedded
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
          </div>
          </Disclosure>
        )}
        {selectedSong && (
          <LightingTimeline
            toolbar={toolbarNode}
            disabled={pendingOperation !== null}
            markers={timelineMarkers}
            onMerge={mergeSelected}
            onMove={moveCue}
            onRemove={removeCue}
            onSelect={(id) => setFocusId(id)}
            onSelectionChange={setSelectedIds}
            selectedIds={selectedIds}
            playhead={elapsed}
            loudness={savedCueMap?.loudness ?? null}
            effects={method === "osc" ? effects : []}
            hiddenTypes={typeFilter.hidden}
            mergeBlockedReason={mergeBlocked}
            method={method}
            onRemoveEffect={(id) => setPendingRemoveEffect(id)}
            onShowAllTypes={typeFilter.showAll}
            onToggleType={typeFilter.toggle}
            songLength={songLength}
          />
        )}

        {/* The list scrolls once it is taller than 32rem, so it is drawn as its own bordered box (only while it has cues) with a fade
            over any edge that has more to see, so the user can tell where the scrolling area starts and ends. */}
        <ScrollRegion
          aria-label={shownCues.length > 0 ? "Lighting cues, scrollable" : undefined}
          className="grid max-h-[32rem] gap-2 overflow-y-auto overscroll-contain p-3"
          data-testid="lighting-cue-list"
          frameClassName="mt-3 rounded-ds-lg"
          framed={shownCues.length > 0}
          role={shownCues.length > 0 ? "region" : undefined}
          tabIndex={shownCues.length > 0 ? 0 : undefined}
        >
          {method === "osc" && listedEffects.map((effect) => (
            <EffectRow
              disabled={pendingOperation !== null}
              effect={effect}
              highlight={effect.id === focusId}
              key={effect.id}
              onRemove={() => setPendingRemoveEffect(effect.id)}
            />
          ))}
          {method === "osc" && shownPage.map((cue) => isLook(cue) ? (
            <LookRow
              allRows={rowIndex}
              cue={cue}
              disabled={pendingOperation !== null}
              highlight={cue.id === focusId}
              key={cue.id}
              onCommit={commitLook}
              onRemove={() => persistOscCues(oscCues.filter((item) => item.id !== cue.id))}
              onRemoveFixture={removeFixture}
              onToggleSelect={() => setSelectedIds((ids) => ids.includes(cue.id) ? ids.filter((id) => id !== cue.id) : [...ids, cue.id])}
              selected={selectedIds.includes(cue.id)}
              songDuration={selectedSong?.duration_seconds ?? null}
            />
          ) : (
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
          {listedMidi.map((cue) => (
            <div className="grid gap-2 border-t border-edge-subtle pt-3 first:border-t-0 first:pt-0 md:grid-cols-[auto_7rem_minmax(11rem,1fr)_7rem_minmax(9rem,1fr)_auto_auto]" data-testid="midi-cue-row" key={cue.id}>
              <span className="self-end pb-2"><CueTypeBadge sharp={classifyMidiCue(cue).sharp} type={classifyMidiCue(cue).type} /></span>
              <Field label="Elapsed time">
                <Input aria-label="Elapsed time" className="font-mono tabular-nums" defaultValue={formatElapsed(cue.at_seconds)} key={`${cue.id}-${cue.at_seconds}`} disabled={pendingOperation !== null} onBlur={(event) => { const parsed = parseElapsed(event.target.value); if (parsed !== null && (selectedSong?.duration_seconds == null || parsed <= selectedSong.duration_seconds)) updateCue(cue.id, { at_seconds: parsed }, true); else event.target.value = formatElapsed(cue.at_seconds); }} placeholder="For example, 01:30…" />
              </Field>
              <Field label="Lighting cue note">
                <Select aria-label="Lighting cue note" disabled={pendingOperation !== null} onChange={(event) => updateCue(cue.id, { note: Number(event.target.value) }, true)} value={cue.note}>
                {displayedNoteOptions.map((option) => <option key={option.note} value={option.note}>{option.label}</option>)}
              </Select>
              </Field>
              <Field label="Lighting cue velocity">
                <Input aria-label="Lighting cue velocity" className="tabular-nums" disabled={pendingOperation !== null} max={127} min={1} onBlur={() => cue.velocity >= 1 && cue.velocity <= 127 && persistCues(cues)} onChange={(event) => updateCue(cue.id, { velocity: Number(event.target.value) })} type="number" value={cue.velocity} />
              </Field>
              <Field label="Cue label">
                <Input aria-label="Cue label" disabled={pendingOperation !== null} maxLength={120} onBlur={() => persistCues(cues)} onChange={(event) => updateCue(cue.id, { label: event.target.value })} placeholder="For example, Verse, chorus, blackout…" value={cue.label} />
              </Field>
              <Button className="self-end" aria-describedby={draftDiffersFromSaved ? testReasonId : undefined} disabled={lights?.connection_status !== "connected" || pendingOperation !== null || draftDiffersFromSaved} onClick={() => onTest(cue.note, cue.velocity)} variant="tinted">Test</Button>
              <Button className="self-end" aria-label={`Remove cue at ${formatElapsed(cue.at_seconds)}`} disabled={pendingOperation !== null} onClick={() => persistCues(cues.filter((item) => item.id !== cue.id))} tone="danger" variant="tinted">Remove</Button>
            </div>
          ))}
          </>
          )}
          {!shownCues.length && <p className="px-4 py-5 text-center text-type-body-sm text-ink-muted">{method === "osc" ? "No OSC cues saved for this song." : "No lighting cues saved for this song."}</p>}
        </ScrollRegion>
        {method === "osc" && pageCount > 1 && (
          <nav aria-label="Cue pages" className="mt-2 flex items-center gap-2" data-testid="cue-pages">
            <Button disabled={safePage === 0} onClick={() => setPage(safePage - 1)} variant="tinted">Previous</Button>
            <span className="text-type-body-sm tabular-nums text-ink-muted">Rows {safePage * PAGE_SIZE + 1}–{Math.min(listedOsc.length, (safePage + 1) * PAGE_SIZE)} of {listedOsc.length}</span>
            <Button disabled={safePage >= pageCount - 1} onClick={() => setPage(safePage + 1)} variant="tinted">Next</Button>
          </nav>
        )}
        {selectedSong?.duration_seconds != null && <p className="mt-2 text-type-body-sm text-ink-muted">Scheduled song length: {formatElapsed(selectedSong.duration_seconds)}. Cues must fall within this duration.</p>}
      </Disclosure>

      {blockedReason && <p className="mt-4 text-sm text-ink-body" id="lighting-save-blocked">{blockedReason}</p>}
      <SaveBar className="mt-4" onDiscard={discard} onRetry={save} onSave={save} state={saveState} />
      <ClearCuesDialog onClose={() => setPendingClear(null)} request={pendingClear} />
      <Dialog
        actions={effectToRemove && (
          <>
            <Button autoFocus onClick={() => setPendingRemoveEffect(null)} size="touch" variant="secondary">Keep loop</Button>
            <Button onClick={() => removeEffect(effectToRemove.id)} size="touch" variant="destructive">Remove loop</Button>
          </>
        )}
        description={effectToRemove && "The motion it plays is lost until you record again."}
        onClose={() => setPendingRemoveEffect(null)}
        open={effectToRemove !== null}
        title="Remove this loop?"
        tone="danger"
      />
    </Panel>
  );
}
