import type { LightkeyDetail, OscArgument, OscLightingCue, OscMessage } from "../types";
import { snapTenths } from "./lightingTimelineMath";

/** A row with several messages is a "look". */
export const isLook = (cue: OscLightingCue): boolean => (cue.messages?.length ?? 0) > 0;

/** The messages a row sends: its look, or else its single address. */
export const rowMessages = (cue: OscLightingCue): OscMessage[] =>
  isLook(cue) ? cue.messages! : cue.address ? [{ address: cue.address, args: cue.args }] : [];

/** `/fixture/MH1/overrides/dimmer` -> `MH1`. Anything else groups under "Other". */
export const fixtureOf = (address: string): string => /^\/fixture\/([^/]+)\//.exec(address)?.[1] ?? "Other";

export const lookFixtureCount = (cue: OscLightingCue): number =>
  new Set((cue.messages ?? []).map((message) => fixtureOf(message.address))).size;

export const lookLabel = (cue: OscLightingCue): string => {
  const count = lookFixtureCount(cue);
  return `Look - ${count} fixture${count === 1 ? "" : "s"}`;
};

export interface FixtureGroup {
  fixture: string;
  items: { index: number; message: OscMessage }[];
}

/** Messages grouped by fixture in first-seen order; `index` is the position in the row. */
export const groupByFixture = (messages: OscMessage[]): FixtureGroup[] => {
  const groups = new Map<string, FixtureGroup>();
  messages.forEach((message, index) => {
    const fixture = fixtureOf(message.address);
    const group = groups.get(fixture) ?? { fixture, items: [] };
    group.items.push({ index, message });
    groups.set(fixture, group);
  });
  return [...groups.values()];
};

export const isCaptured = (cue: OscLightingCue): boolean => cue.source === "captured";

/** A change to a captured row counts as a hand edit; manual rows are returned as they are. */
export const markEdited = (cue: OscLightingCue): OscLightingCue => (isCaptured(cue) ? { ...cue, edited: true } : cue);

export const capturedCount = (cues: OscLightingCue[]): number => cues.filter(isCaptured).length;
export const hasEditedCaptured = (cues: OscLightingCue[]): boolean => cues.some((cue) => isCaptured(cue) && cue.edited);

export const fixturesOf = (cues: OscLightingCue[]): string[] =>
  [...new Set(cues.flatMap((cue) => rowMessages(cue).map((message) => fixtureOf(message.address))))].sort();

/** Deletes one fixture's messages from every row; rows left empty go. Touched captured rows become edited. */
export const removeFixtureFromRows = (cues: OscLightingCue[], fixture: string): OscLightingCue[] => {
  const next: OscLightingCue[] = [];
  for (const cue of cues) {
    if (!isLook(cue)) {
      if (!(cue.address && fixtureOf(cue.address) === fixture)) next.push(cue);
      continue;
    }
    const kept = cue.messages!.filter((message) => fixtureOf(message.address) !== fixture);
    if (kept.length === cue.messages!.length) next.push(cue);
    else if (kept.length) next.push(markEdited({ ...cue, messages: kept }));
  }
  return next;
};

const sameArgs = (left: OscArgument[], right: OscArgument[]) => JSON.stringify(left) === JSON.stringify(right);

export interface MergeResult {
  cues: OscLightingCue[];
  /** Addresses whose value differs between the merged rows: the later value now applies earlier. */
  changedAddresses: string[];
}

/** Merges the chosen rows into one at the earliest time; per address the latest value wins. */
export const mergeRows = (cues: OscLightingCue[], ids: string[]): MergeResult | null => {
  const chosen = cues.filter((cue) => ids.includes(cue.id)).sort((a, b) => a.at_seconds - b.at_seconds);
  if (chosen.length < 2) return null;
  const latest = new Map<string, OscMessage>();
  const changed = new Set<string>();
  for (const cue of chosen) {
    for (const message of rowMessages(cue)) {
      const before = latest.get(message.address);
      if (before && !sameArgs(before.args, message.args)) changed.add(message.address);
      latest.delete(message.address); // keep the latest value last, so order follows time
      latest.set(message.address, message);
    }
  }
  const first = chosen[0]!;
  const allCaptured = chosen.every(isCaptured);
  const merged: OscLightingCue = {
    ...first,
    address: first.address,
    args: first.args,
    messages: [...latest.values()],
    source: allCaptured ? "captured" : "manual",
    edited: allCaptured ? true : undefined,
  };
  if (!merged.edited) delete merged.edited;
  if (merged.source === "manual") delete merged.source;
  if (!first.label && chosen.length) merged.label = chosen.find((cue) => cue.label)?.label ?? "";
  const drop = new Set(chosen.slice(1).map((cue) => cue.id));
  return {
    cues: cues.filter((cue) => !drop.has(cue.id)).map((cue) => (cue.id === first.id ? merged : cue)),
    changedAddresses: [...changed],
  };
};

/** Rows that would be affected by a merge, ordered in time. */
export const rowsInRange = (cues: OscLightingCue[], from: number, to: number): string[] =>
  cues.filter((cue) => cue.at_seconds >= from && cue.at_seconds <= to).map((cue) => cue.id);

export const detailSeconds: Record<LightkeyDetail, number> = { fine: 0.3, normal: 0.5, coarse: 1 };
export const detailLabels: Record<LightkeyDetail, string> = { fine: "Fine", normal: "Normal", coarse: "Coarse" };

/**
 * Re-thins the captured rows to a Detail: rows that fall in the same time slot become one (at the earliest time,
 * latest value per address), and a message that repeats the address's previous value is dropped. Manual rows stay.
 */
export const simplifyCaptured = (cues: OscLightingCue[], detail: LightkeyDetail): OscLightingCue[] => {
  const slot = detailSeconds[detail];
  const captured = cues.filter((cue) => isCaptured(cue) && isLook(cue)).sort((a, b) => a.at_seconds - b.at_seconds);
  const buckets = new Map<number, OscLightingCue[]>();
  for (const cue of captured) {
    const key = Math.floor(snapTenths(cue.at_seconds) / slot + 1e-9);
    buckets.set(key, [...(buckets.get(key) ?? []), cue]);
  }
  const lastValue = new Map<string, OscArgument[]>();
  const out: OscLightingCue[] = [];
  for (const group of buckets.values()) {
    const latest = new Map<string, OscMessage>();
    for (const cue of group) for (const message of cue.messages!) { latest.delete(message.address); latest.set(message.address, message); }
    const messages = [...latest.values()].filter((message) => {
      const previous = lastValue.get(message.address);
      return !previous || !sameArgs(previous, message.args);
    });
    for (const message of latest.values()) lastValue.set(message.address, message.args);
    if (!messages.length) continue;
    const first = group[0]!;
    out.push({ ...first, messages, edited: group.some((cue) => cue.edited) || undefined });
    if (!out[out.length - 1]!.edited) delete out[out.length - 1]!.edited;
  }
  const others = cues.filter((cue) => !(isCaptured(cue) && isLook(cue)));
  return [...others, ...out].sort((a, b) => a.at_seconds - b.at_seconds);
};

export interface MarkerLike { id: string; at: number }
export type MarkerItem<T extends MarkerLike> =
  | { kind: "single"; marker: T }
  | { kind: "cluster"; id: string; at: number; to: number; markers: T[] };

/**
 * Groups markers that would sit closer than `minGapPx` on the track. Markers in `keep` (selected, being dragged)
 * always stay single, so the one you are working on never jumps into a cluster.
 */
export const clusterMarkers = <T extends MarkerLike>(
  markers: T[], pxPerSecond: number, minGapPx: number, keep: ReadonlySet<string> = new Set(),
): MarkerItem<T>[] => {
  const sorted = [...markers].sort((a, b) => a.at - b.at);
  const items: MarkerItem<T>[] = [];
  let run: T[] = [];
  const flush = () => {
    if (run.length === 1) items.push({ kind: "single", marker: run[0]! });
    else if (run.length > 1) items.push({ kind: "cluster", id: `cluster-${run[0]!.id}`, at: run[0]!.at, to: run[run.length - 1]!.at, markers: run });
    run = [];
  };
  for (const marker of sorted) {
    if (keep.has(marker.id)) { flush(); items.push({ kind: "single", marker }); continue; }
    if (run.length && (marker.at - run[0]!.at) * pxPerSecond < minGapPx) run.push(marker);
    else { flush(); run = [marker]; }
  }
  flush();
  return items;
};
