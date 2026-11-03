import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

import type { SongLoudness } from "../types";
import { barClassFor } from "./cueTypeBars";
import { CueTypeLegend } from "./CueTypeParts";
import { useHiddenCueTypes } from "./useHiddenCueTypes";
import { describeTypeCounts, dominantType, glyphFor, typeSpokenLabel, type CueTypeId } from "./lightingCueTypes";
import { clusterMarkers } from "./lightingLooks";
import { formatElapsed, parseElapsed } from "./lightingOsc";
import {
  clampCueTime,
  MARKER_LABEL_ROWS,
  markerType,
    nudgeCueTime,
  padColorClasses,
  placeMarkerLabels,
  rulerStep,
  timeFromTrackOffset,
  type TimelineMarker,
} from "./lightingTimelineMath";
import { loudnessAltText, loudnessPath } from "./loudness";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";
import { ScrollRegion } from "./ui/ScrollRegion";

const ZOOMS = [0, 6, 12, 24, 60, 120] as const; // pixels per second; 0 = fit the width
const CLUSTER_GAP_PX = 14; // markers closer than this on screen become one cluster
const LONG_PRESS_MS = 500;
const MIN_TRACK_REM = 36; // below this the track scrolls sideways instead of squeezing

/**
 * The selected song's lighting timeline: ruler, cue markers and a live playhead.
 * The cue rows below stay the precise editor; this component only reports moves, nudges and deletes.
 */
export function LightingTimeline({
  songLength, markers, playhead, onMove, onRemove, disabled = false, selectedIds = [], onSelectionChange, onMerge, onSelect, loudness,
  method = "osc", hiddenTypes, onToggleType, onShowAllTypes,
}: {
  /** Which method's cues these are; the legend lists that method's types. */
  method?: "osc" | "midi";
  /** Types hidden from view (view-only). When not given the timeline keeps its own per-session state. */
  hiddenTypes?: ReadonlySet<CueTypeId>;
  onToggleType?: (type: CueTypeId) => void;
  onShowAllTypes?: () => void;
  /** The song's recorded room loudness, drawn as a faint envelope behind the markers. */
  loudness?: SongLoudness | null;
  /** Called when a single marker is picked, so the cue list can show its row. */
  onSelect?: (id: string) => void;
  /** Rows picked for merging (shift/ctrl-click, a drag on the track, or a long press on touch). */
  selectedIds?: string[];
  onSelectionChange?: (ids: string[]) => void;
  onMerge?: (ids: string[]) => void;
  songLength: number | null;
  markers: TimelineMarker[];
  /** Elapsed seconds while the song runs, otherwise null. */
  playhead: number | null;
  onMove: (id: string, at: number) => void;
  onRemove: (id: string) => void;
  disabled?: boolean;
}) {
  const ownHidden = useHiddenCueTypes();
  const hidden = hiddenTypes ?? ownHidden.hidden;
  const toggleType = onToggleType ?? ownHidden.toggle;
  const showAllTypes = onShowAllTypes ?? ownHidden.showAll;
  const [zoomIndex, setZoomIndex] = useState(0);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [drag, setDrag] = useState<{ id: string; at: number } | null>(null);
  const [trackWidth, setTrackWidth] = useState(0);
  const trackRef = useRef<HTMLDivElement>(null);
  const markerRefs = useRef(new Map<string, HTMLButtonElement>());
  const [multi, setMulti] = useState(false); // touch: after a long press, a tap toggles
  const [marquee, setMarquee] = useState<{ from: number; to: number } | null>(null);
  const pressTimer = useRef<number | null>(null);
  const pressStart = useRef<{ x: number; y: number } | null>(null);
  const longPressed = useRef(false);
  const marqueeStart = useRef<number | null>(null);
  const pendingScroll = useRef<number | null>(null);
  const picked = new Set(selectedIds);
  const setPicked = (ids: string[]) => { onSelectionChange?.(ids); if (!ids.length) setMulti(false); };
  const toggle = (id: string) => setPicked(picked.has(id) ? selectedIds.filter((item) => item !== id) : [...selectedIds, id]);
  const clearPress = () => { if (pressTimer.current !== null) { window.clearTimeout(pressTimer.current); pressTimer.current = null; } pressStart.current = null; };

  // A song with no known length still gets a usable bar: the last cue plus a minute.
  const length = songLength ?? Math.max(60, ...markers.map((marker) => marker.at + 60));
  const zoom = ZOOMS[zoomIndex]!;
  const trackStyle = {
    minWidth: `${MIN_TRACK_REM}rem`,
    width: zoom === 0 ? "100%" : `${Math.max(length * zoom, 1)}px`,
  };
  const step = rulerStep(length);
  const loudnessShape = loudness ? loudnessPath(loudness, length) : null;
  const ticks = Array.from({ length: Math.floor(length / step) + 1 }, (_, index) => index * step);

  // The width the labels have to share. jsdom (the tests) has no ResizeObserver, so fall back to one read.
  useEffect(() => {
    const node = trackRef.current;
    if (!node) return;
    const measure = () => setTrackWidth(node.getBoundingClientRect().width);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [zoomIndex]);

  useEffect(() => {
    if (pendingScroll.current === null) return;
    const at = pendingScroll.current;
    pendingScroll.current = null;
    let scroller: HTMLElement | null = trackRef.current;
    while (scroller && scroller.scrollWidth <= scroller.clientWidth) scroller = scroller.parentElement;
    if (scroller) scroller.scrollLeft = Math.max(0, (at / length) * (trackRef.current?.getBoundingClientRect().width ?? 0) - scroller.clientWidth / 2);
  });

  useEffect(() => () => clearPress(), []);

  useEffect(() => { if (selectedId) onSelect?.(selectedId); }, [selectedId]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (selectedId && !markers.some((marker) => marker.id === selectedId)) setSelectedId(null);
  }, [markers, selectedId]);

  // A hidden type is only hidden from view: the picked, selected and dragged markers always stay.
  const visibleMarkers = markers.filter((marker) => !hidden.has(markerType(marker)) || marker.id === selectedId || marker.id === drag?.id || picked.has(marker.id));
  const presentTypes = new Set(markers.map(markerType));
  const labelPlacements = placeMarkerLabels(visibleMarkers.map((marker) => ({ id: marker.id, label: marker.label, at: marker.at })), trackWidth, length);
  const percent = (at: number) => `${Math.min(100, Math.max(0, (at / length) * 100))}%`;
  const selected = markers.find((marker) => marker.id === selectedId) ?? null;

  const startDrag = (event: PointerEvent<HTMLButtonElement>, marker: TimelineMarker) => {
    if (disabled || event.button !== 0) return;
    longPressed.current = false;
    if (event.shiftKey || event.ctrlKey || event.metaKey || multi) { toggle(marker.id); longPressed.current = true; return; }
    if (picked.size) setPicked([]);
    setSelectedId(marker.id);
    setDrag({ id: marker.id, at: marker.at });
    event.currentTarget.setPointerCapture?.(event.pointerId);
    if (event.pointerType === "touch" || event.pointerType === "pen") {
      pressStart.current = { x: event.clientX, y: event.clientY };
      pressTimer.current = window.setTimeout(() => {
        pressTimer.current = null;
        longPressed.current = true;
        setDrag(null);
        setMulti(true);
        onSelectionChange?.([marker.id]);
      }, LONG_PRESS_MS);
    }
  };
  const timeAt = (clientX: number) => {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect) return 0;
    return timeFromTrackOffset(clientX - rect.left, rect.width, length);
  };
  const moveDrag = (event: PointerEvent<HTMLButtonElement>) => {
    if (pressStart.current && Math.hypot(event.clientX - pressStart.current.x, event.clientY - pressStart.current.y) > 6) clearPress();
    if (!drag) return;
    setDrag({ id: drag.id, at: clampCueTime(timeAt(event.clientX), length) });
  };
  const endDrag = (event: PointerEvent<HTMLButtonElement>) => {
    clearPress();
    if (!drag) return;
    const at = clampCueTime(timeAt(event.clientX), length);
    const original = markers.find((marker) => marker.id === drag.id);
    setDrag(null);
    if (original && at !== original.at) onMove(drag.id, at);
  };

  const pxPerSecond = trackWidth > 0 ? trackWidth / length : 0;
  const keepSingle = new Set([...(selectedId ? [selectedId] : []), ...(drag ? [drag.id] : []), ...selectedIds]);
  const items = pxPerSecond > 0 ? clusterMarkers(visibleMarkers, pxPerSecond, CLUSTER_GAP_PX, keepSingle) : visibleMarkers.map((marker) => ({ kind: "single" as const, marker }));
  const zoomInto = (at: number, count: number) => {
    if (zoomIndex < ZOOMS.length - 1) { pendingScroll.current = at; setZoomIndex(zoomIndex + 1); }
    else { const first = markers.find((marker) => marker.at === at); if (first) setSelectedId(first.id); }
    void count;
  };
  const trackDown = (event: PointerEvent<HTMLDivElement>) => {
    if (disabled || event.target !== event.currentTarget || event.button !== 0 || event.pointerType === "touch") return;
    marqueeStart.current = timeAt(event.clientX);
    setMarquee({ from: marqueeStart.current, to: marqueeStart.current });
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };
  const trackMove = (event: PointerEvent<HTMLDivElement>) => {
    if (marqueeStart.current === null) return;
    setMarquee({ from: marqueeStart.current, to: timeAt(event.clientX) });
  };
  const trackUp = (event: PointerEvent<HTMLDivElement>) => {
    if (marqueeStart.current === null) return;
    const from = Math.min(marqueeStart.current, timeAt(event.clientX));
    const to = Math.max(marqueeStart.current, timeAt(event.clientX));
    const wide = pxPerSecond === 0 || (to - from) * pxPerSecond >= 4;
    marqueeStart.current = null;
    setMarquee(null);
    if (!wide) { setPicked([]); setSelectedId(null); return; }
    setSelectedId(null);
    setPicked(markers.filter((marker) => marker.at >= from && marker.at <= to).map((marker) => marker.id));
  };

  const onMarkerKey = (event: KeyboardEvent<HTMLButtonElement>, marker: TimelineMarker) => {
    if (disabled) return;
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      setSelectedId(marker.id);
      const next = nudgeCueTime(marker.at, event.key === "ArrowLeft" ? -1 : 1, event.shiftKey, length);
      if (next !== marker.at) onMove(marker.id, next);
    } else if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      onRemove(marker.id);
    } else if ((event.key === "Enter" || event.key === " ") && (event.shiftKey || event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      toggle(marker.id);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      setSelectedId((current) => (current === marker.id ? null : marker.id));
    } else if (event.key === "Escape") {
      setSelectedId(null);
      if (picked.size) setPicked([]);
    }
  };

  return (
    <div className="lighting-timeline mt-4" data-testid="lighting-timeline">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="font-semibold text-ink">Timeline</h3>
        <div aria-label="Timeline zoom" className="flex items-center gap-1" role="group">
          <Button aria-label="Zoom out" disabled={zoomIndex === 0} onClick={() => setZoomIndex((index) => Math.max(0, index - 1))} variant="tinted">−</Button>
          <span aria-live="polite" className="min-w-[3.5rem] text-center text-type-body-sm tabular-nums text-ink-muted">{zoom === 0 ? "Fit" : `${zoom} px/s`}</span>
          <Button aria-label="Zoom in" disabled={zoomIndex === ZOOMS.length - 1} onClick={() => setZoomIndex((index) => Math.min(ZOOMS.length - 1, index + 1))} variant="tinted">+</Button>
        </div>
      </div>
      <CueTypeLegend hidden={hidden} method={method} onShowAll={showAllTypes} onToggle={toggleType} present={presentTypes} />
      <ScrollRegion axis="x" className="overflow-x-auto overscroll-x-contain" data-testid="lighting-timeline-scroll" frameClassName="mt-2 rounded-ds-lg" tabIndex={-1}>
        <div className="relative px-3 pb-3" style={trackStyle}>
          <div aria-hidden="true" className="relative mx-0 h-6 border-b border-edge-subtle" data-testid="lighting-timeline-ruler">
            {ticks.map((tick) => (
              <span className={`absolute top-0 text-type-body-sm tabular-nums text-ink-muted ${tick === 0 ? "" : "-translate-x-1/2"}`} key={tick} style={{ left: percent(tick) }}>{formatElapsed(tick)}</span>
            ))}
          </div>
          <div
            aria-label={`Lighting cues, 0 to ${formatElapsed(length)}`}
            className="relative mt-2"
            data-testid="lighting-timeline-track"
            onPointerDown={trackDown}
            onPointerMove={trackMove}
            onPointerUp={trackUp}
            ref={trackRef}
            role="group"
            style={{ height: `${MARKER_LABEL_ROWS * 20 + 20}px` }}
          >
            {loudness && loudnessShape && (
              <svg
                aria-label={loudnessAltText(loudness)}
                className="pointer-events-none absolute inset-0 h-full w-full text-ink-muted opacity-25"
                data-testid="lighting-loudness"
                preserveAspectRatio="none"
                role="img"
                viewBox="0 0 1000 100"
              >
                <path d={loudnessShape} fill="currentColor" />
              </svg>
            )}
            {playhead !== null && (
              <div
                aria-hidden="true"
                className="pointer-events-none absolute inset-y-0 w-px bg-live-reload"
                data-testid="lighting-playhead"
                style={{ left: percent(Math.min(playhead, length)) }}
              />
            )}
            {marquee && (
              <div aria-hidden="true" className="pointer-events-none absolute inset-y-0 border border-ink-muted bg-ink-muted/10" data-testid="lighting-marquee" style={{ left: percent(Math.min(marquee.from, marquee.to)), width: `${Math.abs(marquee.to - marquee.from) / length * 100}%` }} />
            )}
            {items.map((item) => {
              if (item.kind === "cluster") {
                const counts = new Map<CueTypeId, number>();
                for (const member of item.markers) counts.set(markerType(member), (counts.get(markerType(member)) ?? 0) + 1);
                const lead = dominantType(counts);
                const text = `${item.markers.length} cues (${describeTypeCounts(counts)}), ${formatElapsed(item.at)} to ${formatElapsed(item.to)}. Zoom in`;
                return (
                  <button
                    aria-label={text}
                    className="absolute top-0 flex h-6 min-w-[1.5rem] -translate-x-1/2 touch-manipulation items-center justify-center rounded-ds-full border border-edge-medium bg-surface-panel px-1 text-type-body-sm tabular-nums text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink"
                    data-count={item.markers.length}
                    data-testid="lighting-cluster"
                    data-types={[...counts.keys()].join(" ")}
                    key={item.id}
                    onClick={() => zoomInto(item.at, item.markers.length)}
                    style={{ left: percent(item.at) }}
                    type="button"
                  >{lead && <span aria-hidden="true" className="mr-1 text-ink-muted" data-testid="lighting-cluster-glyph">{glyphFor(lead, lead.startsWith("note-") && item.markers.every((m) => m.sharp))}</span>}<span aria-hidden="true">{item.markers.length}</span></button>
                );
              }
              const marker = item.marker;
              const at = drag?.id === marker.id ? drag.at : marker.at;
              const colors = marker.color ? padColorClasses[marker.color] : null;
              const isSelected = marker.id === selectedId || picked.has(marker.id);
              const type = markerType(marker);
              const spoken = marker.spokenLabel ?? typeSpokenLabel(type, marker.label || "Cue");
              return (
                <button
                  aria-label={`${spoken}, at ${formatElapsed(at)}`}
                  aria-pressed={isSelected}
                  className={`absolute flex w-4 -translate-x-1/2 cursor-grab touch-none items-start justify-center focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink ${drag?.id === marker.id ? "cursor-grabbing" : ""}`}
                  data-source={marker.captured ? "captured" : "manual"}
                  data-testid="lighting-marker"
                  data-type={type}
                  aria-disabled={disabled || undefined}
                  key={marker.id}
                  onClick={() => { if (longPressed.current) { longPressed.current = false; return; } setSelectedId(marker.id); }}
                  onKeyDown={(event) => onMarkerKey(event, marker)}
                  onPointerDown={(event) => startDrag(event, marker)}
                  onPointerMove={moveDrag}
                  onPointerUp={endDrag}
                  ref={(node) => { if (node) markerRefs.current.set(marker.id, node); else markerRefs.current.delete(marker.id); }}
                  style={{ left: percent(at), top: `${(labelPlacements.get(marker.id)?.row ?? 0) * 20}px`, height: `calc(100% - ${(labelPlacements.get(marker.id)?.row ?? 0) * 20}px)` }}
                  type="button"
                >
                  <span className={`block h-full ${type === "cue-off" ? "w-2" : "w-1.5"} rounded-ds-full ${barClassFor(type, marker.sharp)} ${isSelected ? "outline outline-2 outline-ink" : ""}`} data-testid="lighting-marker-bar" />
                  <span
                    className="pointer-events-none absolute left-[14px] top-0 flex items-center gap-1 text-type-body-sm text-ink-body"
                    data-label-row={labelPlacements.get(marker.id)?.row ?? 0}
                    style={{ maxWidth: `${labelPlacements.get(marker.id)?.width ?? 112}px` }}
                  >
                    <span aria-hidden="true" className="shrink-0 rounded-ds-sm border border-edge-medium bg-surface-panel-strong px-1 text-ink" data-testid="lighting-marker-glyph">{glyphFor(type, marker.sharp)}</span>
                    <span className="truncate">{marker.label}</span>
                    {colors && <span aria-hidden="true" className={`inline-block h-2 w-2 shrink-0 rounded-ds-sm border border-ink ${colors.swatch}`} data-testid="lighting-pad-square" />}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      </ScrollRegion>
      {selectedIds.length > 0 && (
        <div aria-label="Selected rows" className="mt-2 flex flex-wrap items-center gap-2 rounded-ds-lg border border-edge-medium bg-surface-panel p-2" data-testid="lighting-selection" role="group">
          <span className="text-type-body-sm text-ink-body">{selectedIds.length} row{selectedIds.length === 1 ? "" : "s"} selected</span>
          <Button disabled={disabled || selectedIds.length < 2 || !onMerge} onClick={() => onMerge?.(selectedIds)} variant="tinted">Merge rows</Button>
          <Button onClick={() => setPicked([])} variant="secondary">Clear selection</Button>
        </div>
      )}
      {!markers.length && <p className="mt-2 text-type-body-sm text-ink-muted">No cues yet. Press a pad while the song runs to place one.</p>}
      {selected && (
        <div aria-label="Selected cue" className="mt-2 flex flex-wrap items-center gap-2 rounded-ds-lg border border-edge-medium bg-surface-panel p-2" data-testid="lighting-marker-popover" role="group">
          <span className="text-type-body-sm text-ink-body">
            <strong>{selected.label || "Cue"}</strong> at <span className="tabular-nums">{formatElapsed(selected.at)}</span>
          </span>
          <label className="flex items-center gap-1 text-type-body-sm text-ink-muted">
            Time
            <Input
              aria-label="Selected cue time"
              className="w-24 font-mono tabular-nums"
              defaultValue={formatElapsed(selected.at)}
              disabled={disabled}
              key={`${selected.id}-${selected.at}`}
              onBlur={(event) => {
                const parsed = parseElapsed(event.target.value);
                if (parsed === null) { event.target.value = formatElapsed(selected.at); return; }
                const next = clampCueTime(parsed, length);
                if (next !== selected.at) onMove(selected.id, next);
                else event.target.value = formatElapsed(selected.at);
              }}
              onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }}
            />
          </label>
          <span className="text-type-body-sm text-ink-muted">Arrow keys move a marker 0.1 s, Shift 1 s. To merge, Shift-click rows, drag across the timeline, or press and hold on touch.</span>
          <Button aria-label={`Delete cue at ${formatElapsed(selected.at)}`} disabled={disabled} onClick={() => onRemove(selected.id)} tone="danger" variant="tinted">Delete</Button>
          <Button onClick={() => { setSelectedId(null); }} variant="secondary">Close</Button>
        </div>
      )}
    </div>
  );
}
