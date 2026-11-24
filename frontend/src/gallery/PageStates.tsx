import { BackendSetupPanel } from "../components/BackendSetupPanel";
import { LightingSetupPanel } from "../components/LightingSetupPanel";
import { MidiSetupPanel } from "../components/MidiSetupPanel";
import { PlanningCenterSetupPanel } from "../components/PlanningCenterSetupPanel";
import { ProPresenterSetupPanel } from "../components/ProPresenterSetupPanel";
import type { LightkeyCleanGlitchesResponse, OscLightingCue, ProPresenterStatusResponse, Song } from "../types";
import { libraryController, libraryOf, linkController, linkOf, playbackController, playbackOf, presenterApi } from "./fixtures";
import { madeUpHealth, madeUpLights, madeUpMidi, madeUpPlanningCenter, madeUpServiceTypes, madeUpSettings, madeUpState } from "./panelFixtures";
import { PresenterApiContext } from "../hooks/presenterApi";
import { madeUpPresenterApi, PLAN_NAMES, withoutPlayback } from "./presenterLinkData";
import { Fresh, Scripted, type Step } from "./Scripted";
import { galleryServiceState } from "./servicePlanSample";
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
  { id: "clean", label: "Presentation: clean", look: "Order: Live Link block, Timer and Look on ONE line (stacked on a phone), the Refresh button and its note right under the Timer box (in its column, no blank band, whatever the height of the Look hint), the Presenter Link section (the plan's names), the Save bar, the summary. Each box shows the current choice: 'Song Countdown (current)', 'Worship (current)'. No Timeout box, no 'Current Look' option.", render: () => page([], presentation()) },
  { id: "held", label: "Presentation: held changes", look: "Another timer and another Look picked: 'Not saved yet' beside both captions with the warning border; '(current)' stays on the saved timer and on the Look ProPresenter shows; the Save bar counts 2 changes.", render: () => page([{ pick: ["Timer", "Video Timer"] }, { pick: ["Look", "l2"] }], presentation()) },
  { id: "look-chosen", label: "Presentation: another Look saved", look: "The saved Look (Message) is the selected one; 'Worship (current)' is the Look ProPresenter shows now and is still listed.", render: () => page([], presentation({ api: ppApi({ look_id: "l2" }) })) },
  { id: "unavailable", label: "Presentation: saved timer and Look no longer there", look: "Both boxes show the saved name with '(unavailable)' and select it; the other choices are still listed.", render: () => page([], presentation({ api: ppApi({ timer_name: "Old Timer", timer_found: false, look_id: "l9" }) })) },
  { id: "timer-none", label: "Presentation: no timer chosen yet", look: "No option is marked '(current)'; the Timer box shows the muted hint 'Choose a timer' (not a choice in the list).", render: () => page([], presentation({ api: ppApi({ timer_name: "", timer_found: false }) })) },
  { id: "not-connected", label: "Presentation: ProPresenter not connected", look: "No Looks known: the Look box is dimmed and disabled and its hint says 'Connect to ProPresenter first'; the saved timer reads '(unavailable)'.", render: () => page([], presentation({ api: ppApi({ connection_status: "disconnected", timers: [], looks: [], current_look_id: null, timer_found: false, look_found: false }) })) },
  { id: "error", label: "Presentation: error after Save", look: "The red sentence under the field (role alert), the Save bar's blocked reason (a line, not a box).", render: () => page([{ type: ["Host", ""] }, { press: "Save configuration" }], presentation()) },
  { id: "busy-refresh", label: "Presentation: Refresh running", look: "Every field is disabled by the section; the button says Refreshing…", render: () => page([], presentation({ pending: "refresh" })) },
  { id: "busy-save", label: "Presentation: saving", look: "Fields disabled; the info line 'Connecting to Presentation…' under the header; the Save bar saving.", render: () => page([], presentation({ pending: "save" })) },
  { id: "disabled", label: "Presentation: not enabled yet", look: "The Refresh button is off (a disabled button); the summary says Not found in warning words, never colour alone.", render: () => page([], presentation({ api: ppApi({ enabled: false, connection_status: "disconnected", timer_found: false, last_checked_at: null }) })) },
  { id: "failed", label: "Presentation: a failure and a message", look: "The danger notice in the alert lines (role alert) with Try again when nothing loaded.", render: () => page([], presentation({ api: null, error: "Could not reach StagePilot." })) },
  { id: "loading", label: "Presentation: loading", look: "The polite info line while the settings load.", render: () => page([], presentation({ api: null })) },
];

// ---- Services (Planning Center) ---------------------------------------------------------------------------------------

const services = (over: { status?: ReturnType<typeof madeUpPlanningCenter>; error?: string | null; message?: string | null; pending?: "test" | "load-types" | "save" | null; types?: boolean; state?: ReturnType<typeof madeUpState>; picker?: boolean } = {}) => (
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
    servicePlanChoice={over.picker ? { pending: false, error: null, announcement: "", pick: nothing, back: nothing } : undefined}
    serviceTypes={over.types === false ? [] : madeUpServiceTypes}
    settings={madeUpSettings()}
    state={over.state ?? madeUpState()}
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
  { id: "past-calendar", label: "Services: a past service open in the calendar", look: "A service from the past 20 days is loaded for review: its date with the 'Past service' tag, the 'Current Upcoming' button and the 'Current upcoming service not selected' pill beside the Calendar button. The calendar shows that day selected, the other past days with a service muted (not dimmed: they can be picked), the upcoming days as before, and, when the 20 days reach into the month before, the back arrow goes there. The arrows beside the date step only among the services from today on.", render: () => <Fresh><Scripted patient steps={[{ press: "Calendar" }]}>{services({ picker: true, state: galleryServiceState("g-past-9") })}</Scripted></Fresh> },
  { id: "upcoming-calendar", label: "Services: the upcoming service open in the calendar", look: "The same calendar with the upcoming service loaded: the past days with a service are muted next to the ones to come.", render: () => <Fresh><Scripted patient steps={[{ press: "Calendar" }]}>{services({ picker: true, state: galleryServiceState("g-next-4") })}</Scripted></Fresh> },
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
  { id: "disconnected", label: "StagePilot: not live", look: "The WebSocket card says Not connected in danger words.", render: () => page([], backend({ live: false })) },
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

// Clean up glitches: a song with captured rows, and a made-up answer from the backend (312 flashes on 16 lights; 8,420 rows become 6,100).
const cleanSong: Song = { id: "item-1", source_song_id: "song-1", title: "Holy Forever", duration_seconds: 240, order: 1, is_generic: false };
const cleanRows: OscLightingCue[] = [4, 30, 60, 90].map((at, i) => ({
  id: `row-${i}`, at_seconds: at, address: "", args: [], label: `Look - ${i + 1} fixture${i ? "s" : ""}`, source: "captured",
  messages: [{ address: `/fixture/P${i + 1}/overrides/dimmer`, args: [{ type: "float", value: 0.5 }] }],
}));
const cleanAnswer = (over: Partial<LightkeyCleanGlitchesResponse>): LightkeyCleanGlitchesResponse => ({
  accepted: true, applied: false, message: "", flashes: 312, lights: 16, flashes_after: 3, rows_before: 8420, rows_after: 6100, messages_before: 9800, messages_after: 7000, loops: 3, ...over,
});
const cleaning = (message: string | null = null) => (
  // The dialog covers the sample, not the whole page: a transformed box is the containing block of its fixed children.
  <div className="relative min-h-[34rem]" style={{ transform: "translateZ(0)" }}>
    <LightingSetupPanel
      error={null}
      lights={madeUpLights({ method: "osc" })}
      message={message}
      onCleanGlitches={async (_songKey, apply) => cleanAnswer(apply ? { applied: true } : {})}
      onRefresh={nothing}
      onSaveCues={nothing}
      onSaveSettings={nothing}
      onTest={nothing}
      pendingOperation={null}
      settings={madeUpSettings({ lights: { method: "osc", osc_host: "192.0.2.10", osc_port: 21600, cue_maps: { "song-1": { song_key: "song-1", song_title: "Holy Forever", cues: [], osc_cues: cleanRows } } } })}
      state={madeUpState({ plan: { id: "p", title: "Sunday", songs: [cleanSong] } as never, current_song: cleanSong })}
    />
  </div>
);

export const lightingPages: SampleState[] = [
  { id: "osc", label: "Lighting: OSC", look: "The Live Link block first, then the songs, the Song lighting timeline (a feature section, remembered), the Save bar.", render: () => page([], lighting("osc")) },
  { id: "midi", label: "Lighting: MIDI", look: "Output, channel and note length on one line (lead column), the note, Refresh outputs, the Send a test cue section, then the same timeline.", render: () => page([], lighting("midi")) },
  { id: "midi-held", label: "Lighting: MIDI, held and error", look: "'Not saved yet' on the channel; the error sentence on the note length after a Save.", render: () => page([{ pick: ["MIDI channel", "5"] }, { type: ["Note length", "5"] }, { press: "Save configuration" }], lighting("midi")) },
  { id: "midi-busy", label: "Lighting: MIDI, Refresh running", look: "The Refresh button says Refreshing….", render: () => page([], lighting("midi", "refresh")) },
  { id: "clean-button", label: "Lighting: a song with captured rows has Clean up glitches", look: "ONE tinted button, Clean up glitches…, in the timeline toolbar after Clear song… (and after Merge rows): only a song that has captured rows shows it. Nothing runs by itself.", render: () => page([], cleaning()) },
  { id: "clean-ask", label: "Lighting: Clean up glitches asks first", look: "The in-app dialog after the button is pressed: Clean up glitches?, what it found (312 flashes on 16 lights in this song), Remove them? You can Undo. Keep as is has the focus; Remove flashes does the repair.", render: () => <Fresh><Scripted patient steps={[{ press: "Clean up glitches" }]}>{cleaning()}</Scripted></Fresh> },
  { id: "clean-done", label: "Lighting: after Clean up glitches", look: "The line says what it did (Removed 312 flashes; 8,420 rows became 6,100.) and the toolbar has one Undo clean-up button, which puts the old song back.", render: () => <Fresh><Scripted patient steps={[{ press: "Clean up glitches" }, { press: "Remove flashes" }]}>{cleaning("Removed 312 flashes; 8,420 rows became 6,100.")}</Scripted></Fresh> },
  { id: "timeline-closed", label: "Lighting: timeline closed (remembered)", look: "The feature section folds to its two-line header; its detail line stays.", render: () => page([{ press: "Song lighting timeline" }], lighting("midi")) },
];
