import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ApplicationState, LightingCue, LightsStatusResponse, OscLightingCue, SettingsResponse, Song, SongLightingCueMap } from "../types";
import { cuesInMap, serviceCueText } from "./lightingCueCount";
import { LightingSetupPanel } from "./LightingSetupPanel";

const fmt = (count: number) => count.toLocaleString("en-US");
const midiCues = (count: number): LightingCue[] => Array.from({ length: count }, (_, index) => ({ id: `m${index}`, at_seconds: index, note: 60, velocity: 100, label: `Cue ${index}` }));
const oscCues = (count: number): OscLightingCue[] => Array.from({ length: count }, (_, index) => ({ id: `o${index}`, at_seconds: index, address: `/cue/${index}`, args: [], label: `Cue ${index}` }) as OscLightingCue);
const map = (key: string, midi: number, osc = 0): SongLightingCueMap => ({ song_key: key, song_title: key, cues: midiCues(midi), osc_cues: oscCues(osc) });
const maps = (...list: SongLightingCueMap[]) => Object.fromEntries(list.map((item) => [item.song_key, item]));

describe("serviceCueText: the Lighting timeline header counts only this service's songs", () => {
  // The real iMac: six older songs hold 78,839 cues, none of the four songs on screen has any.
  const older = maps(map("old-1", 20_000), map("old-2", 18_000), map("old-3", 15_000), map("old-4", 12_000), map("old-5", 8_000), map("old-6", 5_839));

  it("says 'No cues yet' when six saved maps belong to other services and the four songs on screen have none", () => {
    expect(Object.keys(older)).toHaveLength(6);
    const total = Object.values(older).reduce((sum, item) => sum + item.cues.length, 0);
    expect(total).toBe(78_839);
    expect(serviceCueText(["a", "b", "c", "d"], older, "midi", fmt)).toBe("No cues yet");
  });

  it("counts only the cues of the service songs: 2 of 4 songs", () => {
    const all = { ...older, ...maps(map("a", 120), map("c", 30)) };
    expect(serviceCueText(["a", "b", "c", "d"], all, "midi", fmt)).toBe("150 cues on 2 of 4 songs");
  });

  it("uses the singular for one cue and for one song, and a thousands separator", () => {
    expect(serviceCueText(["a"], maps(map("a", 1)), "midi", fmt)).toBe("1 cue on 1 of 1 song");
    expect(serviceCueText(["a"], maps(map("a", 12_000)), "midi", fmt)).toBe("12,000 cues on 1 of 1 song");
    expect(serviceCueText(["a", "b"], maps(map("a", 3)), "midi", fmt)).toBe("3 cues on 1 of 2 songs");
  });

  it("counts `cues` for MIDI and `osc_cues` for OSC, as before", () => {
    const both = maps(map("a", 5, 9), map("b", 0, 4), map("c", 7, 0));
    expect(cuesInMap(both.a, "midi")).toBe(5);
    expect(cuesInMap(both.a, "osc")).toBe(9);
    expect(serviceCueText(["a", "b", "c"], both, "midi", fmt)).toBe("12 cues on 2 of 3 songs");
    expect(serviceCueText(["a", "b", "c"], both, "osc", fmt)).toBe("13 cues on 2 of 3 songs");
  });

  it("treats a map without osc_cues, a missing map and missing settings as no cues", () => {
    const old = { a: { song_key: "a", song_title: "a", cues: midiCues(2) } } as Record<string, SongLightingCueMap>;
    expect(serviceCueText(["a"], old, "osc", fmt)).toBe("No cues yet");
    expect(serviceCueText(["a"], undefined, "midi", fmt)).toBe("No cues yet");
    expect(serviceCueText([], older, "midi", fmt)).toBe("No cues yet");
  });

  it("never says more songs with cues than songs on screen, however many maps are saved", () => {
    const saved = maps(...Array.from({ length: 12 }, (_, index) => map(`s${index}`, index + 1)));
    for (let songs = 1; songs <= 14; songs += 1) {
      const keys = Array.from({ length: songs }, (_, index) => `s${index}`);
      for (const method of ["midi", "osc"] as const) {
        const text = serviceCueText(keys, saved, method, fmt);
        const found = /on (\d+) of (\d+) songs?$/.exec(text);
        if (found) expect(Number(found[1])).toBeLessThanOrEqual(Number(found[2]));
      }
    }
    // The same song listed twice is one saved map: counted once, and still not above the list length.
    expect(serviceCueText(["s1", "s1"], saved, "midi", fmt)).toBe("2 cues on 1 of 2 songs");
  });
});

describe("the timeline header on the Lighting panel", () => {
  const mkSong = (order: number): Song => ({ id: `item-${order}`, source_song_id: `song-${order}`, title: `Song ${order}`, duration_seconds: 300, order, is_generic: false });
  const songs = [1, 2, 3, 4].map(mkSong);
  const lights = { enabled: true, method: "midi", osc_host: "127.0.0.1", osc_port: 21600, output_name: "StagePilot to Lightkey", channel: 3, pulse_ms: 100, connection_status: "connected", detail: "Connected", outputs: [], last_cue: null, last_cue_at: null } as LightsStatusResponse;
  const state = { revision: 1, plan: { id: "plan-1", title: "Sunday", songs }, current_song: songs[0], next_song: null, current_song_index: 0, playing_song: null, playing_song_index: null, up_next_song: null, plugins: {}, recent_events: [], recent_errors: [], timer: { status: "idle" } } as unknown as ApplicationState;
  const settingsWith = (cueMaps: Record<string, SongLightingCueMap>) => ({ planning_center_secret_saved: true, warning: null, restart_required: false, settings: { lights: { ...lights, cue_maps: cueMaps } } }) as unknown as SettingsResponse;
  const headerDetail = (cueMaps: Record<string, SongLightingCueMap>) => {
    window.localStorage.setItem("stagepilot.lighting.timeline-open", "false");
    const { container, unmount } = render(<LightingSetupPanel error={null} lights={lights} message={null} onRefresh={vi.fn()} onSaveCues={vi.fn()} onSaveSettings={vi.fn()} onTest={vi.fn()} pendingOperation={null} settings={settingsWith(cueMaps)} state={state} />);
    const text = container.querySelector("[data-part=detail]")!.textContent;
    unmount();
    return text;
  };

  it("the iMac case: 6 saved maps of older songs, 4 songs on screen, none captured -> No cues yet, not '6 of 4 songs'", () => {
    const older = maps(...Array.from({ length: 6 }, (_, index) => map(`older-${index}`, 13_000)));
    const text = headerDetail(older);
    expect(text).toMatch(/^No cues yet · 0 pads/);
    expect(text).not.toMatch(/of 4 songs/);
  });

  it("counts only the songs on screen: 2 of 4 songs", () => {
    const text = headerDetail({ ...maps(map("older-1", 50_000), map("older-2", 28_839)), ...maps(map("song-2", 40), map("song-4", 2)) });
    expect(text).toMatch(/^42 cues on 2 of 4 songs · 0 pads/);
  });
});
