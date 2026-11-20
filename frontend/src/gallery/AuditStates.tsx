import { EventRow } from "../components/dashboard/EventRow";
import { ReadinessRows } from "../components/dashboard/ReadinessRows";
import type { ReadinessCheck } from "../components/dashboard/dashboardReadiness";
import { ActionButton, ActionRow } from "../components/sections";
import { Panel } from "../components/ui/Panel";
import { SaveBar } from "../components/ui/SaveBar";
import { Field } from "../components/ui/Field";
import { Input } from "../components/ui/Input";
import { describeStream } from "../lib/eventWords";
import { timeWords } from "../lib/dateWords";
import type { EventSummary } from "../types";
import { PlaybackApiPanel } from "../components/PlaybackApiPanel";
import { playbackController, playbackOf } from "./fixtures";
import { Fresh } from "./Scripted";
import type { SampleState } from "./StateCard";

const noop = () => undefined;

// ---- Disabled controls say why -------------------------------------------------------------------------------------------------------------
const reasoned = [
  { id: "reasons-services", label: "Switched-off buttons say why: Services", look: "Test connection and Load service types are dimmed. Hover (or focus) each: the sentence says what to do first. A screen reader reads the same sentence.", render: () => (
    <Fresh><Panel tone="services" variant="setup"><ActionRow>
      <ActionButton label="Test connection" onClick={noop} reason="Enter the Planning Center secret first." tone="services" />
      <ActionButton label="Load service types" onClick={noop} reason="Sign in to Planning Center first." tone="services" />
      <ActionButton label="Load today's service plan" onClick={noop} reason="Load service types first." tone="services" />
    </ActionRow></Panel></Fresh>
  ) },
  { id: "reasons-midi", label: "Switched-off buttons say why: MIDI and Lighting", look: "Connect, Disconnect and Undo last capture, each with its own reason; the one that can be pressed has none.", render: () => (
    <Fresh><Panel tone="midi" variant="setup"><ActionRow>
      <ActionButton label="Refresh inputs" onClick={noop} tone="midi" />
      <ActionButton label="Connect" onClick={noop} reason="Choose an input first." tone="midi" />
      <ActionButton label="Disconnect" onClick={noop} reason="No input is connected." tone="midi" />
      <ActionButton label="Undo last capture" onClick={noop} reason="Nothing to undo." tone="lighting" />
    </ActionRow></Panel></Fresh>
  ) },
];

// ---- Event stream in plain words --------------------------------------------------------------------------------------------------------------
const at = (n: number): string => `2026-11-19T14:${String(31 - n).padStart(2, "0")}:42`;
const EVENTS: EventSummary[] = [
  { id: "e1", type: "connection.changed", source: "playback_connection", timestamp: at(0) },
  { id: "e2", type: "connection.changed", source: "propresenter", timestamp: at(1) },
  { id: "e3", type: "plugin.status_changed", source: "lights", timestamp: at(2) },
  { id: "e4", type: "service_plan.loaded", source: "planning_center", timestamp: at(3) },
  { id: "e5", type: "timer.stopped", source: "timer", timestamp: at(4) },
  { id: "e6", type: "song.started", source: "playback", timestamp: at(5) },
  { id: "e7", type: "some.new_kind_of_event", source: "lights", timestamp: at(6) },
];

const eventStream = () => {
  const shown = EVENTS.filter((event) => event.type !== "plugin.status_changed");
  const words = describeStream(shown, { playback_connection: true, propresenter: true });
  return (
    <div className="max-w-xl overflow-hidden rounded-ds-xl border border-edge bg-surface-panel">
      <p className="border-b border-edge-subtle px-4 py-2 text-xs font-bold uppercase text-ink-muted">Event stream</p>
      {shown.map((event, index) => <EventRow key={event.id} text={words[index]!.text} time={timeWords(event.timestamp, { seconds: true }) ?? ""} title={words[index]!.title} />)}
    </div>
  );
};

// ---- The header's check list ---------------------------------------------------------------------------------------------------------------------
const CHECKS: ReadinessCheck[] = [
  { id: "pc", label: "Services connected", passed: true, required: true, severity: "info", status: "connected" },
  { id: "pp", label: "Presentation not connected", passed: false, required: true, severity: "blocking", status: "disconnected", detail: "ProPresenter is not answering." },
  { id: "pb", label: "Playback not connected", passed: false, required: true, severity: "blocking", status: "disconnected" },
  { id: "lib", label: "Playback library not ready", passed: false, required: false, severity: "warning", status: "disconnected", detail: "The Library Link is optional." },
];

// ---- Save bar -------------------------------------------------------------------------------------------------------------------------------------
const bar = (kind: "held" | "auto") => (
  <Fresh><Panel tone="presentation" variant="setup">
    <div className="grid gap-2 pb-3">
      {["Host", "Port"].map((label, i) => <Field key={label} label={label}><Input defaultValue={i ? "8080" : "10.30.0.141"} /></Field>)}
    </div>
    <SaveBar onDiscard={noop} onSave={noop} state={kind === "held" ? { kind: "held", count: 2 } : { kind: "auto", at: "2026-11-19T14:31:42" }} />
  </Panel></Fresh>
);

export const auditStates: SampleState[] = [
  ...reasoned,
  { id: "event-stream", label: "Event stream in plain words", look: "Playback connected, Presentation connected, Service plan loaded, Timer stopped, Song started; no plugin.status_changed row; an event nobody named reads as plain words (Some new kind of event). Hover a row: the raw name is its hover text.", render: eventStream },
  { id: "check-list", label: "The header's check list", look: "A check that blocks a service and is not ready: red, with a ×. An optional one that is not ready: grey, with a !. One that is fine: green, with a ✓. The mark says it, not only the colour.", render: () => <div className="max-w-sm"><ReadinessRows checks={CHECKS} /></div> },
  { id: "save-bar-held", label: "Save bar with held changes (look at 360 px)", look: "A held change already has a solid fill. The tinted states (below) are the ones that were see-through on a phone.", render: () => bar("held") },
  { id: "save-bar-auto", label: "Save bar after an automatic save (look at 360 px)", look: "Below 1024 px the bar is the opaque panel colour with a heavier top border, so cards that scroll under it never show through; from 1024 px up it keeps its tint. Switch the width to compare.", render: () => bar("auto") },
  { id: "song-order-learning", label: "Song order: learning", look: "The Song order header says it once, with a neutral chip: Learning order (1 of 4). No red sentence under the songs.", render: () => <PlaybackApiPanel playback={playbackController(playbackOf({ order_mode: "learned", order_source: "learned", song_order: [], song_count: 4, plan_song_count: 4, learned_songs: [{ song_id: 101, plan_position: 1, confirmed: true, doubtful: false }] }))} /> },
];
