import { useEffect, useState } from "react";

import type { PresenterFollowController } from "../hooks/usePresenterFollow";
import { OFFSET_MAX_MS, OFFSET_MIN_MS } from "../presenterFollow";
import { Disclosure } from "./ui/Disclosure";
import { Field } from "./ui/Field";
import { Input } from "./ui/Input";

const OFFSET_ERROR = `Enter a whole number from ${OFFSET_MIN_MS} to ${OFFSET_MAX_MS}.`;

/** `\u2212` and `-` both read as a minus; an empty box or anything that is not a whole number in range is refused. */
function parseOffset(text: string): number | null {
  const clean = text.trim().replace(/\u2212/g, "-");
  if (!/^[-+]?\d+$/.test(clean)) return null;
  const value = Number(clean);
  return value >= OFFSET_MIN_MS && value <= OFFSET_MAX_MS ? value : null;
}

/** The folded **Timing** group of the Auto-Presenter: one number box, the offset in milliseconds, saved when the person leaves the box or presses Enter. */
export function AutoPresenterTiming({ follow }: { follow?: PresenterFollowController }) {
  const busy = follow?.pending != null;
  // The box holds what is typed until the person leaves it (or presses Enter); then it is saved if it is a whole number in range.
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
  // Folded, the row reads like any setting: its value in body ink, and a quiet `changed` mark when the offset is not the usual 0.
  const summary = (
    <span className="inline-flex min-w-0 items-center gap-2 text-ink-body" data-testid="presenter-follow-timing-summary">
      <span>{`Offset ${saved} ms`}</span>
      {saved !== 0 && <span className="border border-edge-medium px-1.5 text-xs leading-5 text-ink-body" data-testid="presenter-follow-timing-changed">changed</span>}
    </span>
  );
  return (
    <Disclosure data-testid="presenter-follow-timing" summary={summary} title={<span className="text-ink">Timing</span>} tone="presentation" variant="advanced">
      <Field
        error={refused ? OFFSET_ERROR : undefined}
        hint="Positive shows every slide earlier, negative later. Playback's cues come a little before the lyric, so 0 is usually right."
        label="Offset (ms)"
      >
        <div className="w-28 max-w-full">
          <Input
            disabled={busy || !follow}
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
  );
}
