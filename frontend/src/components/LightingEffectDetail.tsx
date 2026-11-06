import type { LightingEffect } from "../types";
import { formatElapsed } from "./lightingOsc";
import { effectCycleSamples, effectLabel, effectProperties, sparklinePoints } from "./lightingSmooth";
import { Button } from "./ui/Button";

const SPARK_W = 168;
const SPARK_H = 34;

/** One property across one cycle of the loop: a small line, with the words for screen readers. */
function CycleSparkline({ effect, property }: { effect: LightingEffect; property: string }) {
  const samples = effectCycleSamples(effect, property);
  if (samples.length < 2) return null;
  const low = Math.min(...samples);
  const high = Math.max(...samples);
  const words = `${property}, one ${effect.period_seconds.toFixed(1)} s cycle, from ${low.toFixed(2)} to ${high.toFixed(2)}`;
  return (
    <figure className="m-0 flex items-center gap-2" data-testid="effect-sparkline" data-property={property}>
      <svg aria-label={words} className="shrink-0 rounded-ds-sm border border-edge-subtle bg-surface-950 text-cue-effect" height={SPARK_H + 6} role="img" viewBox={`-3 -3 ${SPARK_W + 6} ${SPARK_H + 6}`} width={SPARK_W + 6}>
        <polyline fill="none" points={sparklinePoints(samples, SPARK_W, SPARK_H)} stroke="currentColor" strokeLinejoin="round" strokeWidth="2" />
      </svg>
      <figcaption className="text-type-body-sm text-ink-muted">{property}</figcaption>
    </figure>
  );
}

/**
 * What a loop is made of, in words and a picture: its window and period, the properties it drives, one small line per property
 * for a single cycle, and which fixtures play it and how far each is shifted (a chase). Read only: a loop is never edited by the
 * rows, so the only action is to remove it (and record again to get a new one).
 */
export function EffectDetail({ effect, disabled = false, onRemove, className = "" }: {
  effect: LightingEffect;
  disabled?: boolean;
  onRemove?: () => void;
  className?: string;
}) {
  const properties = effectProperties(effect);
  const span = effect.end_seconds - effect.start_seconds;
  const cycles = effect.period_seconds > 0 ? Math.floor(span / effect.period_seconds + 1e-9) : 0;
  return (
    <div className={`grid gap-2 ${className}`} data-testid="effect-detail">
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-type-body-sm">
        <dt className="text-ink-muted">Loop</dt>
        <dd className="text-ink">{effectLabel(effect)}</dd>
        <dt className="text-ink-muted">Period</dt>
        <dd className="tabular-nums text-ink" data-testid="effect-period">{effect.period_seconds.toFixed(2)} s</dd>
        <dt className="text-ink-muted">Window</dt>
        <dd className="tabular-nums text-ink" data-testid="effect-window">{formatElapsed(effect.start_seconds)} to {formatElapsed(effect.end_seconds)} ({cycles} cycle{cycles === 1 ? "" : "s"})</dd>
        <dt className="text-ink-muted">Properties</dt>
        <dd className="text-ink" data-testid="effect-properties">{properties.join(", ") || "none"}</dd>
      </dl>
      <div className="flex flex-wrap gap-x-4 gap-y-1">
        {properties.map((property) => <CycleSparkline effect={effect} key={property} property={property} />)}
      </div>
      <div>
        <h4 className="text-type-body-sm font-semibold text-ink">Fixtures and phase offsets</h4>
        <ul aria-label="Loop members" className="mt-1 flex max-h-28 flex-wrap gap-1 overflow-y-auto overscroll-contain" data-testid="effect-members">
          {effect.members.map((member) => (
            <li className="rounded-ds-sm border border-edge-subtle bg-surface-950 px-1.5 py-0.5 text-type-body-sm tabular-nums text-ink-body" key={member.fixture}>
              {member.fixture} <span className="text-ink-muted">+{(member.offset_seconds ?? 0).toFixed(2)} s</span>
            </li>
          ))}
        </ul>
      </div>
      <p className="text-type-body-sm text-ink-muted">A loop is played as one item. It is not changed by editing the rows; to get a different one, remove it and record the song again.</p>
      {onRemove && (
        <div>
          <Button aria-label={`Remove loop ${effectLabel(effect)}`} disabled={disabled} onClick={onRemove} tone="danger" variant="tinted">Remove loop...</Button>
        </div>
      )}
    </div>
  );
}
