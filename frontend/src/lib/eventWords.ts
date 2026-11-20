import type { EventSummary } from "../types";

/**
 * The Event stream in plain words (decided 2026-11-18, design audit). The backend's events are named for developers
 * (`connection.changed · playback_connection`); an operator reads `Playback connected`. The raw name stays in the row's hover text
 * for support. Nothing here changes what is recorded.
 */

/** What each event says, by its type. `connection.changed` is worded with the source's name and state (below). */
export const EVENT_WORDS: Readonly<Record<string, string>> = {
  "application.started": "StagePilot started",
  "application.stopping": "StagePilot is stopping",
  "action.requested": "Control pressed",
  "service.reload_requested": "Service plan reload requested",
  "service.plan_selection_requested": "Service chosen",
  "service.load_changed": "Service plan loading",
  "service.loaded": "Service plan loaded",
  "song.selected": "Song selected",
  "song.started": "Song started",
  "song.restarted": "Song restarted",
  "midi.note_received": "MIDI note received",
  "timer.stop_requested": "Timer stop requested",
  "timer.reset_requested": "Timer reset requested",
  "timer.started": "Timer started",
  "timer.resumed": "Timer resumed",
  "timer.seeked": "Timer moved",
  "timer.stopped": "Timer stopped",
  "timer.failed": "Timer failed",
  "plugin.failed": "A connection failed",
};

/** Events that are bookkeeping, not news: they are left out of the stream (and do not use up its rows). */
export const HIDDEN_EVENTS: ReadonlySet<string> = new Set(["plugin.status_changed"]);

/** The name an operator knows each connection by, for the sources that publish `connection.changed`. */
export const SOURCE_NAMES: Readonly<Record<string, string>> = {
  planning_center: "Services",
  playback_connection: "Playback",
  midi_playback: "Playback MIDI",
  propresenter: "Presentation",
  lights: "Lighting",
};

/** `plugin_manager` becomes `Plugin manager`, `timer.seeked` becomes `Timer seeked`: readable, never dotted. */
export function readable(raw: string): string {
  const words = raw.replace(/[._]+/g, " ").replace(/\s+/g, " ").trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : "Something happened";
}

export const sourceName = (source: string): string => SOURCE_NAMES[source] ?? readable(source);

export const isHiddenEvent = (event: Pick<EventSummary, "type">): boolean => HIDDEN_EVENTS.has(event.type);

/** What a source is doing now, by source (`true` connected). The stream words an old connection event with it (see `describeStream`). */
export type ConnectedNow = Readonly<Record<string, boolean | undefined>>;

export interface PlainEvent {
  id: string;
  /** The words shown. */
  text: string;
  /** The raw `type · source`, for support. */
  title: string;
}

/**
 * Words for a newest-first list of events. A connection event does not say which way it went, so it is read off the source's state now:
 * the newest one of a source is how the source is now, the one before it the other way (the backend keeps one entry per way and source).
 * With no state known it says `<Name> changed`. Rows from Playback's own song events and the pinned-error row already carry a sentence as
 * their `source`; that sentence is the text.
 */
export function describeStream(events: readonly EventSummary[], connectedNow: ConnectedNow = {}): PlainEvent[] {
  const seen = new Map<string, number>();
  return events.filter((event) => !isHiddenEvent(event)).map((event) => {
    const title = `${event.type} · ${event.source}`;
    if (event.type.startsWith("playback.")) return { id: event.id, text: event.source, title };
    // The pinned error's own row (`<Name> error`) is already words.
    if (event.type.includes(" ")) return { id: event.id, text: event.type, title };
    if (event.type === "connection.changed") {
      const name = sourceName(event.source);
      const count = seen.get(event.source) ?? 0;
      seen.set(event.source, count + 1);
      const now = connectedNow[event.source];
      if (now === undefined) return { id: event.id, text: `${name} changed`, title };
      return { id: event.id, text: `${name} ${(count % 2 === 0 ? now : !now) ? "connected" : "not connected"}`, title };
    }
    return { id: event.id, text: EVENT_WORDS[event.type] ?? readable(event.type), title };
  });
}
