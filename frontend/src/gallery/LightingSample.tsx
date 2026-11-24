import { useState } from "react";

import { LightkeyStrip } from "../components/LightkeyStrip";
import { EffectRow } from "../components/LightingOscParts";
import { LightingTimeline } from "../components/LightingTimeline";
import type { TimelineMarker } from "../components/lightingTimelineMath";
import type { EffectPatch, LightingEffect, LightkeyStatus, LibrarySection, SongLoudness, SongWaveform } from "../types";

/** The Lighting strip, reading its status from the page (made-up) instead of the backend; its host and port boxes are typed into. */
export function Lighting({ status, attempted = false, held = false, host = "192.0.2.10" }: { status: LightkeyStatus; attempted?: boolean; held?: boolean; host?: string }) {
  const [typed, setTyped] = useState(host);
  const [port, setPort] = useState("21600");
  return (
    <LightkeyStrip
      connection={{ host: typed, onHost: setTyped, oscPort: port, onPort: setPort, savedOscHost: "192.0.2.10", attempted, held: { host: held, port: held } }}
      lightingEnabled
      readStatus={async () => status}
      testCue={<p className="text-sm text-ink-body">The test cue's address and values go here.</p>}
    />
  );
}

const LENGTH = 240;
const SECTIONS: LibrarySection[] = [
  { name: "Count Off", start: 0, end: 4 }, { name: "Verse 1", start: 4, end: 44 }, { name: "Chorus", start: 44, end: 84 }, { name: "Verse 2", start: 84, end: 124 },
  { name: "Chorus", start: 124, end: 164 }, { name: "Bridge", start: 164, end: 204 }, { name: "Chorus", start: 204, end: LENGTH },
];
const MARKERS: TimelineMarker[] = [4, 30, 44, 70, 84, 110, 124, 150, 164, 190, 204, 232].map((at, i) => ({ id: `m${i}`, at, label: `Look ${i + 1}`, color: null, type: "look-captured", captured: true }));
/** Louder in the choruses, quiet in the bridge: the same shape for both pictures so the two states are easy to compare. */
const shape = (second: number) => {
  const section = SECTIONS.find((item) => second >= item.start && second < item.end);
  const base = section?.name === "Chorus" ? 0.85 : section?.name === "Bridge" ? 0.35 : 0.55;
  return base * (0.75 + 0.25 * Math.abs(Math.sin(second * 1.7)));
};
const ROOM: SongLoudness = { step_seconds: 0.25, takes: 2, values: Array.from({ length: LENGTH * 4 }, (_, i) => 1 + Math.round(shape(i / 4) * 200)) };
const WAVE: SongWaveform = { step_s: 0.125, peak: Array.from({ length: LENGTH * 8 }, (_, i) => Math.round(shape(i / 8) * 255)) };

/** The Lighting timeline of one song, as it looks with Playback's waveform behind the markers or with the room's loudness. */
export function LightingTimelineSample({ kind }: { kind: "waveform" | "loudness" }) {
  return (
    <LightingTimeline
      arrangement={SECTIONS}
      loudness={kind === "loudness" ? ROOM : null}
      markers={MARKERS}
      measureLabel={(text) => text.length * 6.5}
      onMove={() => undefined}
      onRemove={() => undefined}
      playhead={null}
      songLength={LENGTH}
      songMap
      waveform={kind === "waveform" ? WAVE : null}
    />
  );
}

/** A 16-fixture chase that repeats every 4 s through the chorus, with and without small changes on top of the loop. */
const chase = (id: string, start: number, end: number, patches: EffectPatch[]): LightingEffect => ({
  id, label: "", start_seconds: start, end_seconds: end, period_seconds: 4, source: "captured",
  members: Array.from({ length: 16 }, (_, i) => ({ fixture: `P${i + 1}`, offset_seconds: i * 0.25 })),
  cycle: [{ at_seconds: 0, property: "dimmer", value: [0.6], fade_seconds: 0.25 }, { at_seconds: 0.5, property: "dimmer", value: [0], fade_seconds: 0.25 }],
  ...(patches.length ? { patches } : {}),
});
const BRIGHT = (fixture: string, from: number): EffectPatch => ({
  fixture, property: "dimmer", from_seconds: from, to_seconds: from + 0.9,
  steps: [{ at_seconds: 0, value: [1], fade_seconds: 0.25 }, { at_seconds: 0.5, value: [0], fade_seconds: 0.25 }],
});
const DROPPED = (fixture: string, from: number): EffectPatch => ({
  fixture, property: "dimmer", from_seconds: from, to_seconds: from + 0.9, steps: [{ at_seconds: 0, value: [0], fade_seconds: 0.25 }, { at_seconds: 0.5, value: [0], fade_seconds: 0.25 }],
});
const CHANGES: EffectPatch[] = [
  ...Array.from({ length: 6 }, (_, i) => BRIGHT(`P${i + 1}`, 92.5 + i * 0.25)),
  DROPPED("P9", 130.5), DROPPED("P12", 174.75),
];

/**
 * The Lighting timeline with two loops, one that repeats cleanly and one that carries changes on top of it (its label says
 * "+N changes"), and under it the loop's row open on its list of changes.
 */
export function LightingLoopChangesSample() {
  const clean = chase("clean", 44, 84, []);
  const changed = chase("changed", 84, 204, CHANGES);
  return (
    <div className="grid gap-3">
      <LightingTimeline
        arrangement={SECTIONS}
        effects={[clean, changed]}
        loudness={null}
        markers={MARKERS}
        measureLabel={(text) => text.length * 6.5}
        onMove={() => undefined}
        onRemove={() => undefined}
        playhead={null}
        songLength={LENGTH}
        songMap
        waveform={null}
      />
      <EffectRow disabled={false} effect={changed} onRemove={() => undefined} startOpen />
    </div>
  );
}
