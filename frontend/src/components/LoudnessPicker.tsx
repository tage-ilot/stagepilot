import { useEffect, useRef, useState } from "react";

import { saveLightkeySettings, setLightkeyAudioMeter } from "../api";
import { describeError } from "../errorText";
import type { LightkeyAudio } from "../types";
import { meterPercent } from "./loudness";
import { Button } from "./ui/Button";
import { Select } from "./ui/Select";

const METER_POLL_MS = 400;
const DEFAULT_INPUT = "";

/**
 * Record mode only: which audio input measures the room, with a live level meter, and a way to
 * clear the song's loudness. Where audio capture is unavailable (Docker, headless, no input,
 * permission refused) the picker is hidden and one short line says why.
 */
export function LoudnessPicker({ hasLoudness, onClear, disabled = false }: {
  hasLoudness: boolean;
  onClear?: () => void;
  disabled?: boolean;
}) {
  const [info, setInfo] = useState<LightkeyAudio | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  // The meter runs only while this is on screen: asking again keeps it open, leaving switches it off.
  useEffect(() => {
    alive.current = true;
    const tick = async () => {
      try {
        const next = await setLightkeyAudioMeter(true);
        if (alive.current) { setInfo(next); setError(null); }
      } catch {
        if (alive.current) setError("Loudness is not available right now.");
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), METER_POLL_MS);
    return () => {
      alive.current = false;
      window.clearInterval(timer);
      void Promise.resolve(setLightkeyAudioMeter(false)).catch(() => undefined);
    };
  }, []);

  const choose = async (name: string) => {
    try {
      await saveLightkeySettings({ audio_device: name });
      if (alive.current) setInfo((current) => (current ? { ...current, selected: name === DEFAULT_INPUT ? null : name } : current));
    } catch (cause) {
      if (alive.current) setError(describeError(cause, "That input could not be saved."));
    }
  };

  const clear = hasLoudness && onClear
    ? <Button disabled={disabled} onClick={onClear} variant="secondary">Clear loudness</Button>
    : null;

  if (error || (info && !info.available)) {
    return (
      <div className="mt-2" data-testid="lightkey-loudness">
        <p className="text-type-body-sm text-ink-muted" data-testid="loudness-unavailable">{error ?? info?.reason ?? "Loudness is not available."}</p>
        {clear && <div className="mt-1">{clear}</div>}
      </div>
    );
  }
  if (!info) return null;

  const saved = info.selected;
  const missing = saved !== null && !info.devices.includes(saved);
  const percent = meterPercent(info.level?.value);
  const db = info.level?.rms_db;
  return (
    <div className="mt-2" data-testid="lightkey-loudness">
      <label className="flex flex-wrap items-center gap-2 text-type-body-sm text-ink-body">
        Audio input
        <Select
          aria-label="Audio input"
          className="w-full sm:w-72"
          disabled={disabled}
          onChange={(event) => void choose(event.target.value)}
          value={saved ?? DEFAULT_INPUT}
        >
          <option value={DEFAULT_INPUT}>System default input</option>
          {missing && <option value={saved}>{saved} (not found)</option>}
          {info.devices.map((name) => <option key={name} value={name}>{name}</option>)}
        </Select>
      </label>
      <div className="mt-2 flex items-center gap-2">
        <div
          aria-label="Input level"
          aria-valuemax={100}
          aria-valuemin={0}
          aria-valuenow={percent}
          aria-valuetext={db === null || db === undefined ? "No signal" : `${Math.round(db)} dBFS`}
          className="h-2 w-full max-w-xs overflow-hidden rounded-ds-full bg-surface-panel"
          data-testid="loudness-meter"
          role="meter"
        >
          <div className="h-full bg-integration-lighting-osc-soft" style={{ width: `${percent}%` }} />
        </div>
        <span className="min-w-[4.5rem] text-type-body-sm tabular-nums text-ink-muted">{db === null || db === undefined ? "No signal" : `${Math.round(db)} dBFS`}</span>
      </div>
      {info.note && <p className="mt-1 text-type-body-sm text-ink-muted" data-testid="loudness-note">{info.note}</p>}
      <p className="mt-1 text-type-body-sm text-ink-muted">Measures how loud the room is while a song records. Only levels are saved, never audio.</p>
      {clear && <div className="mt-1">{clear}</div>}
    </div>
  );
}
