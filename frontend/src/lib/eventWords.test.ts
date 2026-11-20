import { describe, expect, it } from "vitest";

import type { EventSummary } from "../types";
import { describeStream, EVENT_WORDS, HIDDEN_EVENTS, isHiddenEvent, readable, sourceName } from "./eventWords";

const event = (type: string, source = "demo", id = `${type}-${source}`): EventSummary => ({ id, type, source, timestamp: "2026-11-19T14:31:00Z" });

describe("the Event stream in plain words", () => {
  it.each(Object.entries(EVENT_WORDS))("%s reads %s, and keeps the raw name for support", (type, words) => {
    const [row] = describeStream([event(type, "state_service")]);
    expect(row).toMatchObject({ text: words, title: `${type} · state_service` });
    expect(row!.text).not.toMatch(/[._]/);
  });

  it.each([
    ["connection.changed", "playback_connection", true, "Playback connected"],
    ["connection.changed", "playback_connection", false, "Playback not connected"],
    ["connection.changed", "propresenter", true, "Presentation connected"],
    ["connection.changed", "planning_center", true, "Services connected"],
    ["connection.changed", "lights", false, "Lighting not connected"],
    ["connection.changed", "midi_playback", true, "Playback MIDI connected"],
  ])("%s from %s while it is connected=%s reads %s", (type, source, now, words) => {
    expect(describeStream([event(type, source)], { [source]: now })[0]!.text).toBe(words);
  });

  it("words the older connection entry of a source the other way (the backend keeps one of each)", () => {
    const rows = describeStream([event("connection.changed", "propresenter", "newer"), event("timer.stopped"), event("connection.changed", "propresenter", "older")], { propresenter: true });
    expect(rows.map((row) => row.text)).toEqual(["Presentation connected", "Timer stopped", "Presentation not connected"]);
  });

  it("says which source changed when its state is not known", () => {
    expect(describeStream([event("connection.changed", "lights")])[0]!.text).toBe("Lighting changed");
  });

  it("drops plugin.status_changed rows", () => {
    expect(HIDDEN_EVENTS.has("plugin.status_changed")).toBe(true);
    expect(isHiddenEvent(event("plugin.status_changed", "plugin_manager"))).toBe(true);
    expect(describeStream([event("plugin.status_changed", "plugin_manager"), event("service.loaded")]).map((row) => row.text)).toEqual(["Service plan loaded"]);
  });

  it.each([
    ["plugin.something_new", "Plugin something new"],
    ["timer.seeked_forward", "Timer seeked forward"],
    ["weird", "Weird"],
    ["a.b.c", "A b c"],
    ["", "Something happened"],
  ])("an unknown type (%j) falls back to the plain form %j, never the dotted name", (type, words) => {
    const [row] = describeStream([event(type)]);
    expect(row!.text).toBe(words);
    expect(row!.text).not.toMatch(/\./);
    expect(row!.title).toBe(`${type} · demo`);
  });

  it("keeps the sentence of Playback's own rows and of the pinned error's row", () => {
    expect(describeStream([{ id: "p", type: "playback.song.paused", source: "Paused (while scanning Playback songs)", timestamp: "2026-11-19T14:31:00Z" }])[0]!.text).toBe("Paused (while scanning Playback songs)");
    expect(describeStream([{ id: "e", type: "Presentation error", source: "Evicted", timestamp: "2026-11-19T14:31:00Z" }])[0]!.text).toBe("Presentation error");
  });

  it("names sources the way operators do, and any other source readably", () => {
    expect(sourceName("playback_connection")).toBe("Playback");
    expect(sourceName("plugin_manager")).toBe("Plugin manager");
    expect(readable("state_service")).toBe("State service");
  });
});
