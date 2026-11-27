import { useEffect, useId, useState } from "react";

import type { PresenterFollowController } from "../hooks/usePresenterFollow";
import { FOLLOW_LABEL, followLine, lockedReason, OFFSET_MAX_MS, OFFSET_MIN_MS } from "../presenterFollow";
import { ActionButton, ActionRow, ReasonPill } from "./sections";
import { Disclosure } from "./ui/Disclosure";
import { Field } from "./ui/Field";
import { Input } from "./ui/Input";
import { Switch } from "./ui/Switch";
import { cx } from "./ui/cx";

const LINE_TONE = {
  success: "text-success-text",
  idle: "text-ink-body",
  warning: "text-warning-text",
  danger: "text-danger-text",
  info: "text-info-text",
} as const;

const OFFSET_ERROR = `Enter a whole number from ${OFFSET_MIN_MS} to ${OFFSET_MAX_MS}.`;

/** `\u2212` and `-` both read as a minus; an empty box or anything that is not a whole number in range is refused. */
function parseOffset(text: string): number | null {
  const clean = text.trim().replace(/\u2212/g, "-");
  if (!/^[-+]?\d+$/.test(clean)) return null;
  const value = Number(clean);
  return value >= OFFSET_MIN_MS && value <= OFFSET_MAX_MS ? value : null;
}

/**
 * **Follow Playback in ProPresenter**, in the Presenter Link area: the master switch (off by default), ONE status line (a mark and words, so
 * the state is never a colour alone), Try again only while following is paused by failures, and the timing offset in a folded Timing group.
 * It is always drawn: while the Presenter Link or Playback's Library Link is not ready it is greyed, nothing in it can be pressed and a
 * small pill says why. Nothing here ever triggers ProPresenter by itself; the backend follows only while the switch is on.
 */
export function PresenterFollowBlock({ follow, linkReady, libraryReady }: { follow?: PresenterFollowController; linkReady: boolean; libraryReady: boolean }) {
  const ids = { heading: useId(), line: useId(), reason: useId(), offsetError: useId() };
  const reason = lockedReason(linkReady, libraryReady);
  const locked = reason !== null || !follow;
  const enabled = follow?.enabled ?? false;
  const line = followLine(follow?.status ?? null, enabled, follow?.error ?? null);
  const busy = follow?.pending != null;
  const canTryAgain = enabled && !locked && Boolean(follow?.status?.can_try_again);

  // The offset box holds what is typed until the person leaves it (or presses Enter); then it is saved if it is a whole number in range.
  const saved = follow?.offsetMs ?? 0;
  const [text, setText] = useState(String(saved));
  const [refused, setRefused] = useState(false);
  // What was saved is what the box shows again (after a save, or when the settings are read anew).
  useEffect(() => { setText(String(saved)); setRefused(false); }, [saved]);
  const commit = () => {
    const value = parseOffset(text);
    if (value === null) { setRefused(true); return; }
    setRefused(false);
    if (value !== saved) void follow?.setOffset(value);
    else setText(String(value));
  };

  return (
    <section aria-labelledby={ids.heading} className={cx("min-w-0 space-y-2 rounded-ds-lg border border-edge-subtle bg-surface-950/40 p-4", locked && "opacity-60")} data-locked={locked || undefined} data-testid="presenter-follow">
      <h3 className="sr-only" id={ids.heading}>{FOLLOW_LABEL}</h3>
      <fieldset aria-describedby={`${ids.line}${reason ? ` ${ids.reason}` : ""}`} className="m-0 min-w-0 space-y-3 border-0 p-0" disabled={locked}>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <Switch
            checked={enabled}
            data-testid="presenter-follow-switch"
            disabled={busy}
            label={FOLLOW_LABEL}
            onChange={(event) => void follow?.setEnabled(event.target.checked)}
          />
          {reason && <ReasonPill id={ids.reason} testId="presenter-follow-reason">{reason}</ReasonPill>}
        </div>
        <p aria-live="polite" className={cx("flex min-w-0 items-start gap-2 text-sm", LINE_TONE[line.tone])} data-state={line.state} data-testid="presenter-follow-status" id={ids.line} role="status">
          <span aria-hidden="true" className="w-3 flex-none text-center font-mono">{line.mark}</span>
          <span className="min-w-0 break-words">{line.text}</span>
        </p>
        {canTryAgain && (
          <ActionRow>
            <ActionButton busy={follow?.pending === "retry"} busyLabel="Trying again…" label="Try again" onClick={() => follow?.tryAgain()} tone="presentation" />
          </ActionRow>
        )}
        <Disclosure data-testid="presenter-follow-timing" summary={`Offset ${saved} ms`} title="Timing" tone="presentation" variant="advanced">
          <Field
            error={refused ? OFFSET_ERROR : undefined}
            hint="Positive shows every slide earlier, negative later. Playback's cues come a little before the lyric, so 0 is usually right."
            label="Offset (ms)"
          >
            <div className="w-28 max-w-full">
              <Input
                disabled={busy}
                inputMode="numeric"
                max={OFFSET_MAX_MS}
                min={OFFSET_MIN_MS}
                onBlur={commit}
                onChange={(event) => { setText(event.target.value); setRefused(false); }}
                onKeyDown={(event) => { if (event.key === "Enter") commit(); }}
                step={50}
                type="number"
                value={text}
              />
            </div>
          </Field>
        </Disclosure>
      </fieldset>
    </section>
  );
}
