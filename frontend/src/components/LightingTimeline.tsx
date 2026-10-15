import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

import { formatElapsed, parseElapsed } from "./lightingOsc";
import {
  clampCueTime,
  neutralPad,
  nudgeCueTime,
  padColorClasses,
  rulerStep,
  timeFromTrackOffset,
  type TimelineMarker,
} from "./lightingTimelineMath";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";

const ZOOMS = [0, 6, 12, 24] as const; // pixels per second; 0 = fit the width
const MIN_TRACK_REM = 36; // below this the track scrolls sideways instead of squeezing

/**
 * The selected song's lighting timeline: ruler, cue markers and a live playhead.
 * The cue rows below stay the precise editor; this component only reports moves, nudges and deletes.
 */
export function LightingTimeline({
  songLength, markers, playhead, onMove, onRemove, disabled = false,
}: {
  songLength: number | null;
  markers: TimelineMarker[];
  /** Elapsed seconds while the song runs, otherwise null. */
  playhead: number | null;
  onMove: (id: string, at: number) => void;
  onRemove: (id: string) => void;
  disabled?: boolean;
}) {
  const [zoomIndex, setZoomIndex] = useState(0);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [drag, setDrag] = useState<{ id: string; at: number } | null>(null);
  const trackRef = useRef<HTMLDivElement>(null);
  const markerRefs = useRef(new Map<string, HTMLButtonElement>());

  // A song with no known length still gets a usable bar: the last cue plus a minute.
  const length = songLength ?? Math.max(60, ...markers.map((marker) => marker.at + 60));
  const zoom = ZOOMS[zoomIndex]!;
  const trackStyle = {
    minWidth: `${MIN_TRACK_REM}rem`,
    width: zoom === 0 ? "100%" : `${Math.max(length * zoom, 1)}px`,
  };
  const step = rulerStep(length);
  const ticks = Array.from({ length: Math.floor(length / step) + 1 }, (_, index) => index * step);

  useEffect(() => {
    if (selectedId && !markers.some((marker) => marker.id === selectedId)) setSelectedId(null);
  }, [markers, selectedId]);

  const percent = (at: number) => `${Math.min(100, Math.max(0, (at / length) * 100))}%`;
  const selected = markers.find((marker) => marker.id === selectedId) ?? null;

  const startDrag = (event: PointerEvent<HTMLButtonElement>, marker: TimelineMarker) => {
    if (disabled || event.button !== 0) return;
    setSelectedId(marker.id);
    setDrag({ id: marker.id, at: marker.at });
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };
  const timeAt = (clientX: number) => {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect) return 0;
    return timeFromTrackOffset(clientX - rect.left, rect.width, length);
  };
  const moveDrag = (event: PointerEvent<HTMLButtonElement>) => {
    if (!drag) return;
    setDrag({ id: drag.id, at: clampCueTime(timeAt(event.clientX), length) });
  };
  const endDrag = (event: PointerEvent<HTMLButtonElement>) => {
    if (!drag) return;
    const at = clampCueTime(timeAt(event.clientX), length);
    const original = markers.find((marker) => marker.id === drag.id);
    setDrag(null);
    if (original && at !== original.at) onMove(drag.id, at);
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
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      setSelectedId((current) => (current === marker.id ? null : marker.id));
    } else if (event.key === "Escape") {
      setSelectedId(null);
    }
  };

  return (
    <div className="lighting-timeline mt-4" data-testid="lighting-timeline">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="font-semibold text-ink">Timeline</h3>
        <div aria-label="Timeline zoom" className="flex items-center gap-1" role="group">
          <Button aria-label="Zoom out" disabled={zoomIndex === 0} onClick={() => setZoomIndex((index) => Math.max(0, index - 1))} tone="lighting" variant="tinted">−</Button>
          <span aria-live="polite" className="min-w-[3.5rem] text-center text-type-body-sm tabular-nums text-ink-muted">{zoom === 0 ? "Fit" : `${zoom} px/s`}</span>
          <Button aria-label="Zoom in" disabled={zoomIndex === ZOOMS.length - 1} onClick={() => setZoomIndex((index) => Math.min(ZOOMS.length - 1, index + 1))} tone="lighting" variant="tinted">+</Button>
        </div>
      </div>
      <div className="scroll-region scroll-region-x mt-2 overflow-x-auto overscroll-x-contain rounded-ds-lg" data-testid="lighting-timeline-scroll" tabIndex={-1}>
        <div className="relative px-3 pb-3" style={trackStyle}>
          <div aria-hidden="true" className="relative mx-0 h-6 border-b border-edge-subtle" data-testid="lighting-timeline-ruler">
            {ticks.map((tick) => (
              <span className="absolute top-0 -translate-x-1/2 text-type-body-sm tabular-nums text-ink-muted" key={tick} style={{ left: percent(tick) }}>{formatElapsed(tick)}</span>
            ))}
          </div>
          <div
            aria-label={`Lighting cues, 0 to ${formatElapsed(length)}`}
            className="relative mt-2 h-14"
            data-testid="lighting-timeline-track"
            ref={trackRef}
            role="group"
          >
            {playhead !== null && (
              <div
                aria-hidden="true"
                className="pointer-events-none absolute inset-y-0 w-px bg-live-reload"
                data-testid="lighting-playhead"
                style={{ left: percent(Math.min(playhead, length)) }}
              />
            )}
            {markers.map((marker) => {
              const at = drag?.id === marker.id ? drag.at : marker.at;
              const colors = marker.color ? padColorClasses[marker.color] : null;
              const isSelected = marker.id === selectedId;
              return (
                <button
                  aria-label={`${marker.label || "Cue"} at ${formatElapsed(at)}`}
                  aria-pressed={isSelected}
                  className={`absolute top-0 flex h-full w-4 -translate-x-1/2 cursor-grab touch-none items-start justify-center focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink ${drag?.id === marker.id ? "cursor-grabbing" : ""}`}
                  data-testid="lighting-marker"
                  aria-disabled={disabled || undefined}
                  key={marker.id}
                  onClick={() => setSelectedId(marker.id)}
                  onKeyDown={(event) => onMarkerKey(event, marker)}
                  onPointerDown={(event) => startDrag(event, marker)}
                  onPointerMove={moveDrag}
                  onPointerUp={endDrag}
                  ref={(node) => { if (node) markerRefs.current.set(marker.id, node); else markerRefs.current.delete(marker.id); }}
                  style={{ left: percent(at) }}
                  type="button"
                >
                  <span className={`block h-full w-1 rounded-ds-full ${colors?.marker ?? neutralPad.marker} ${isSelected ? "outline outline-2 outline-ink" : ""}`} />
                  <span className="pointer-events-none absolute left-2 top-0 max-w-[7rem] truncate text-type-body-sm text-ink-body">{marker.label}</span>
                </button>
              );
            })}
          </div>
        </div>
      </div>
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
          <span className="text-type-body-sm text-ink-muted">Arrow keys move a marker 0.1 s, Shift 1 s.</span>
          <Button aria-label={`Delete cue at ${formatElapsed(selected.at)}`} disabled={disabled} onClick={() => onRemove(selected.id)} tone="danger" variant="tinted">Delete</Button>
          <Button onClick={() => { setSelectedId(null); }} variant="secondary">Close</Button>
        </div>
      )}
    </div>
  );
}
