import { barClassFor } from "./cueTypeBars";
import { CUE_TYPES, glyphFor, typeIdsFor, type CueTypeId, type CueTypeInfo } from "./lightingCueTypes";
import { Button } from "./ui/Button";

/** Glyph box + colour swatch, shown on cue list rows and in the legend. */
export function CueTypeBadge({ type, sharp = false }: { type: CueTypeId; sharp?: boolean }) {
  return (
    <span className="inline-flex items-center gap-1" data-testid="cue-type-badge" data-type={type} title={CUE_TYPES[type].name}>
      <span aria-hidden="true" className={`inline-block h-5 w-3 rounded-ds-full ${barClassFor(type, sharp)}`} data-testid="cue-type-swatch" />
      <span aria-hidden="true" className="rounded-ds-sm border border-edge-medium bg-surface-panel-strong px-1 text-type-body-sm text-ink" data-testid="cue-type-glyph">{glyphFor(type, sharp)}</span>
      <span className="sr-only">{CUE_TYPES[type].name}{sharp ? " (sharp)" : ""}</span>
    </span>
  );
}

const GROUPS: CueTypeInfo["group"][] = ["Looks", "Lightkey cues", "Single messages", "Notes"];

/** Legend above the timeline. Each chip is a toggle that hides or shows that type in the timeline and the cue list. */
export function CueTypeLegend({
  method, hidden, onToggle, onShowAll, present, className = "", compact = false,
}: {
  className?: string;
  /** One tight line: the group names are read by screen readers only and the gaps are smaller. */
  compact?: boolean;
  method: "osc" | "midi";
  hidden: ReadonlySet<CueTypeId>;
  onToggle: (type: CueTypeId) => void;
  onShowAll: () => void;
  /** Types that have at least one cue on screen; a type with none cannot be hidden or shown, so its chip is off (MIDI letters). */
  present: ReadonlySet<CueTypeId>;
}) {
  const ids = typeIdsFor(method);
  const hiddenCount = ids.filter((id) => hidden.has(id)).length;
  return (
    <div aria-label="Cue types" className={`mt-2 flex flex-wrap items-center ${compact ? "gap-x-3 gap-y-1.5" : "gap-x-4 gap-y-2"} ${className}`} data-testid="cue-legend" role="group">
      {GROUPS.map((group) => {
        const types = ids.filter((id) => CUE_TYPES[id].group === group);
        if (!types.length) return null;
        return (
          <div aria-label={group} className="flex flex-wrap items-center gap-1.5" key={group} role="group">
            <span className={compact ? "sr-only" : "text-type-body-sm uppercase tracking-wide text-ink-muted"}>{group}</span>
            {types.map((id) => {
              const off = hidden.has(id);
              const unused = !present.has(id) && !off;
              return (
                <button
                  aria-label={CUE_TYPES[id].name}
                  aria-pressed={!off}
                  className={`inline-flex min-h-[34px] touch-manipulation items-center gap-1.5 rounded-ds-full border border-edge-medium bg-surface-panel-strong py-1 pl-2 pr-3 text-type-body-sm text-ink-body focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink ${off ? "line-through opacity-40" : ""} ${unused ? "opacity-40" : ""}`}
                  data-testid="cue-legend-chip"
                  data-type={id}
                  disabled={unused}
                  key={id}
                  onClick={() => onToggle(id)}
                  type="button"
                >
                  <span aria-hidden="true" className={`inline-block h-4 w-2.5 rounded-ds-full ${barClassFor(id)}`} />
                  <span aria-hidden="true" className="rounded-ds-sm border border-edge-medium px-1 text-ink">{CUE_TYPES[id].glyph}</span>
                  <span aria-hidden="true">{group === "Notes" ? CUE_TYPES[id].glyph : CUE_TYPES[id].name}</span>
                </button>
              );
            })}
          </div>
        );
      })}
      <span aria-live="polite" className="text-type-body-sm text-ink-muted" data-testid="cue-legend-hidden">
        {hiddenCount > 0 ? `${hiddenCount} type${hiddenCount === 1 ? "" : "s"} hidden` : ""}
      </span>
      {hiddenCount > 0 && <Button onClick={onShowAll} variant="tinted">Show all</Button>}
    </div>
  );
}
