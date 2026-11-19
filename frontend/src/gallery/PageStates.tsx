import { BackendSetupPanel } from "../components/BackendSetupPanel";
import { LightingSetupPanel } from "../components/LightingSetupPanel";
import { MidiSetupPanel } from "../components/MidiSetupPanel";
import { PlanningCenterSetupPanel } from "../components/PlanningCenterSetupPanel";
import { ProPresenterSetupPanel } from "../components/ProPresenterSetupPanel";
import type { ProPresenterStatusResponse } from "../types";
import { libraryController, libraryOf, linkController, linkOf, playbackController, playbackOf, presenterApi } from "./fixtures";
import { madeUpHealth, madeUpLights, madeUpMidi, madeUpPlanningCenter, madeUpServiceTypes, madeUpSettings, madeUpState } from "./panelFixtures";
import { PresenterApiContext } from "../hooks/presenterApi";
import { madeUpPresenterApi, PLAN_NAMES, withoutPlayback } from "./presenterLinkData";
import { Fresh, Scripted, type Step } from "./Scripted";
import type { SampleState } from "./StateCard";

/** Every callback of a page does nothing here: the gallery asks no backend and saves nothing. */
const nothing = () => undefined;
const page = (steps: Step[], node: React.ReactNode, flags?: Record<string, boolean>) => <Fresh flags={flags}><Scripted steps={steps}>{node}</Scripted></Fresh>;

// ---- Presentation -----------------------------------------------------------------------------------------------------

const timers = [{ id: "t1", name: "Song Countdown", index: 0, is_countdown: true, state: null }, { id: "t2", name: "Video Timer", index: 1, is_countdown: false, state: null }];
const looks = [{ id: "l1", name: "Worship", index: 0 }, { id: "l2", name: "Message", index: 1 }];
const ppApi = (over: Partial<ProPresenterStatusResponse> = {}) => presenterApi({ timers, looks, current_look_id: "l1", last_checked_at: "2026-11-15T17:45:00Z", ...over });
const madeUpStagePilot = madeUpPresenterApi();
// With the Presenter Link ready the page also draws its section (the plan's Planning Center names, no Playback data), answered by a made-up StagePilot.
const presentation = (over: { api?: ProPresenterStatusResponse | null; error?: string | null; message?: string | null; pending?: "save" | "test" | "refresh" | null } = {}) => (
  <PresenterApiContext.Provider value={madeUpStagePilot}>
    <ProPresenterSetupPanel
      error={over.error ?? null}
      library={withoutPlayback()}
      link={linkController(linkOf("ready"))}
      message={over.message ?? null}
      onRefreshTimers={nothing}
      onSave={nothing}
      onTest={nothing}
      pendingOperation={over.pending ?? null}
      plan={{ id: "p", title: "Sunday", date: "2026-11-22", service_type: "Sunday", service_type_id: null, service_times: [], duration_source: "plan", songs: PLAN_NAMES.songs }}
      propresenter={over.api === undefined ? ppApi() : over.api}
    />
  </PresenterApiContext.Provider>
);

export const presentationPages: SampleState[] = [
  { id: "clean", label: "Presentation: clean", look: "Order: Live Link block, Detected timer + Timeout (a pair), Look (narrow, with its hint), the Refresh row, the Presenter Link section (the plan's names), the Save bar, the summary (API, Configured timer, Last checked).", render: () => page([], presentation()) },
  { id: "held", label: "Presentation: held changes", look: "'Not saved yet' beside each changed caption with the warning border; the Refresh row says it uses the saved settings; the Save bar counts the changes.", render: () => page([{ type: ["Timeout", "5"] }, { pick: ["Detected timer", "Video Timer"] }, { pick: ["ProPresenter Look", "l2"] }], presentation()) },
  { id: "error", label: "Presentation: error after Save", look: "The red sentence under the field (role alert), the Save bar's blocked reason (a line, not a box).", render: () => page([{ type: ["Timeout", "0"] }, { press: "Save configuration" }], presentation()) },
  { id: "busy-refresh", label: "Presentation: Refresh running", look: "Every field is disabled by the section; the button says Refreshing…", render: () => page([], presentation({ pending: "refresh" })) },
  { id: "busy-save", label: "Presentation: saving", look: "Fields disabled; the info line 'Connecting to Presentation…' under the header; the Save bar saving.", render: () => page([], presentation({ pending: "save" })) },
  { id: "disabled", label: "Presentation: not enabled yet", look: "The Refresh button is off (a disabled button); the summary says Not found in warning words, never colour alone.", render: () => page([], presentation({ api: ppApi({ enabled: false, connection_status: "disconnected", timer_found: false, last_checked_at: null }) })) },
  { id: "failed", label: "Presentation: a failure and a message", look: "The danger notice in the alert lines (role alert) with Try again when nothing loaded.", render: () => page([], presentation({ api: null, error: "Could not reach StagePilot." })) },
  { id: "loading", label: "Presentation: loading", look: "The polite info line while the settings load.", render: () => page([], presentation({ api: null })) },
];

// ---- Services (Planning Center) ---------------------------------------------------------------------------------------

const services = (over: { status?: ReturnType<typeof madeUpPlanningCenter>; error?: string | null; message?: string | null; pending?: "test" | "load-types" | "save" | null; types?: boolean } = {}) => (
  <PlanningCenterSetupPanel
    error={over.error ?? null}
    message={over.message ?? null}
    onLoadServiceTypes={nothing}
    onReload={nothing}
    onSave={nothing}
    onSelectPlan={nothing}
    onTest={nothing}
    onClose={nothing}
    pendingAction={null}
    pendingOperation={over.pending ?? null}
    pendingPlanId={null}
    serviceTypes={over.types === false ? [] : madeUpServiceTypes}
    settings={madeUpSettings()}
    state={madeUpState()}
    status={over.status ?? madeUpPlanningCenter()}
    onDisconnectOAuth={nothing}
    onSignInOAuth={nothing}
  />
);

export const servicesPages: SampleState[] = [
  { id: "signed-out", label: "Services: signed out", look: "The info notice with its sign-in button; Manual API Connection closed beside it; the four fields in one divided group; two actions; the Save bar; the summary of four.", render: () => page([], services({ status: madeUpPlanningCenter({ connection_status: "disconnected", planning_center_secret_saved: false, oauth_connected: false }) })) },
  { id: "signed-in", label: "Services: signed in", look: "The success notice with its title, its words and Disconnect.", render: () => page([], services({ status: madeUpPlanningCenter({ connection_method: "oauth", oauth_connected: true }) })) },
  { id: "expired", label: "Services: sign-in expired", look: "The warning notice (role alert) with Reconnect.", render: () => page([], services({ status: madeUpPlanningCenter({ connection_method: "oauth", oauth_connected: true, oauth_needs_reconnect: true, connection_status: "error" }) })) },
  { id: "manual-held", label: "Services: Manual connection open, held change", look: "The optional section open; its two fields in a pair; 'Not saved yet' on the secret; Test connection as an action row.", render: () => page([{ press: "Manual API Connection" }, { type: ["Secret", "new-secret"] }], services()) },
  { id: "busy", label: "Services: loading service types", look: "All fields disabled by the section; the button says Loading….", render: () => page([], services({ pending: "load-types" })) },
  { id: "no-types", label: "Services: no service types yet", look: "Service type is off on its own (no types to choose), not busy; the others are enabled.", render: () => page([], services({ types: false })) },
  { id: "error", label: "Services: a failure", look: "The danger notice, in the notices just above the Save bar.", render: () => page([], services({ error: "Planning Center did not answer. Check the Internet and try again." })) },
];

// ---- Backend (StagePilot) ---------------------------------------------------------------------------------------------

const backend = (over: { live?: boolean; error?: string | null; message?: string | null } = {}) => (
  <BackendSetupPanel
    error={over.error ?? null}
    health={madeUpHealth()}
    live={over.live ?? true}
    message={over.message ?? null}
    onClose={nothing}
    onSave={nothing}
    pending={false}
    settings={madeUpSettings()}
    state={madeUpState()}
  />
);

export const backendPages: SampleState[] = [
  { id: "clean", label: "StagePilot: clean", look: "The four status cards open the page (the summary at the top), then Advanced (closed, with its one-line summary), Connection Access, the Save bar, the log buttons under a divider.", render: () => page([], backend()) },
  { id: "advanced-open", label: "StagePilot: Advanced open, held port", look: "Three fields 1/2/4 across (inset columns), 'Not saved yet' and the restart hint on the port, the note under the group.", render: () => page([{ press: "Advanced" }, { type: ["Server port", "9000"] }], backend()) },
  { id: "error", label: "StagePilot: invalid port after Save", look: "Advanced opens by itself with the error sentence; the Save bar's blocked reason.", render: () => page([{ press: "Advanced" }, { type: ["Server port", "0"] }, { press: "Save configuration" }], backend()) },
  { id: "disconnected", label: "StagePilot: not live", look: "The WebSocket card says Disconnected in danger words.", render: () => page([], backend({ live: false })) },
  { id: "failure", label: "StagePilot: a failure", look: "The danger notice above the Save bar.", render: () => page([], backend({ error: "The settings could not be saved. Your changes are still here." })) },
];

// ---- Playback / MIDI --------------------------------------------------------------------------------------------------

const songOrderStatus = () => playbackOf({ song_order: [101, 202, 303], song_count: 3, plan_song_count: 3, captured_at: "2026-11-15T17:40:00Z", song_lengths: [], order_source: "scan" });
const playbackPage = (method: "playback_api" | "real", withStatus = true) => (
  <MidiSetupPanel
    library={libraryController(libraryOf("ready"))}
    message={null}
    messages={[]}
    midi={madeUpMidi()}
    error={null}
    onRefresh={nothing}
    onSaveSettings={nothing}
    onSelect={nothing}
    pendingOperation={null}
    playback={withStatus ? playbackController(songOrderStatus()) : undefined}
    settings={madeUpSettings({ midi_source: method })}
    songs={[]}
  />
);

export const playbackPages: SampleState[] = [
  { id: "api-open", label: "Playback: Song order open (remembered)", look: "The Live Link block first, then the Song order band: toggle (chevron, heading, one-line summary) and its button on one row; the Save bar under it.", render: () => page([], playbackPage("playback_api")) },
  { id: "api-closed", label: "Playback: Song order closed (remembered)", look: "Only the header row stays; the body is folded away and out of the tab order.", render: () => page([{ press: "Song order" }], playbackPage("playback_api")) },
  { id: "midi", label: "MIDI: clean", look: "The input row (Refresh inputs, Connect, Disconnect as an action row), the status chips, the velocities (advanced, closed), the monitor.", render: () => page([], playbackPage("real", false)) },
  { id: "midi-held", label: "MIDI: velocities open, held change", look: "Nine fields in three columns; 'Not saved yet' on the channel; the one note under them.", render: () => page([{ press: "Playback song velocities" }, { type: ["MIDI channel", "3"] }], playbackPage("real", false)) },
];

// ---- Lighting ---------------------------------------------------------------------------------------------------------

const lighting = (method: "osc" | "midi", pending: "refresh" | null = null) => (
  <LightingSetupPanel
    error={null}
    lights={madeUpLights({ method })}
    message={null}
    onRefresh={nothing}
    onSaveCues={nothing}
    onSaveSettings={nothing}
    onTest={nothing}
    pendingOperation={pending}
    settings={madeUpSettings({ lights: { method, osc_host: "192.0.2.10", osc_port: 21600 } })}
    state={madeUpState()}
  />
);

export const lightingPages: SampleState[] = [
  { id: "osc", label: "Lighting: OSC", look: "The Live Link block first, then the songs, the Song lighting timeline (a feature section, remembered), the Save bar.", render: () => page([], lighting("osc")) },
  { id: "midi", label: "Lighting: MIDI", look: "Output, channel and note length on one line (lead column), the note, Refresh outputs, the Send a test cue section, then the same timeline.", render: () => page([], lighting("midi")) },
  { id: "midi-held", label: "Lighting: MIDI, held and error", look: "'Not saved yet' on the channel; the error sentence on the note length after a Save.", render: () => page([{ pick: ["MIDI channel", "5"] }, { type: ["Note length", "5"] }, { press: "Save configuration" }], lighting("midi")) },
  { id: "midi-busy", label: "Lighting: MIDI, Refresh running", look: "The Refresh button says Refreshing….", render: () => page([], lighting("midi", "refresh")) },
  { id: "timeline-closed", label: "Lighting: timeline closed (remembered)", look: "The feature section folds to its two-line header; its detail line stays.", render: () => page([{ press: "Song lighting timeline" }], lighting("midi")) },
];
