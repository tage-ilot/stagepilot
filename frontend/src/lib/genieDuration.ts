/**
 * Dev hook: stretch the config-panel animation so its middle can be inspected. Read when the duration is computed
 * (every render, so it applies at the next open): `localStorage.setItem("spGenieMs", "6")` or `?genieMs=6`.
 * A value from 1 to 49 multiplies the normal duration (6 = six times slower); 50..20000 sets the duration in ms
 * (the older form, `3000` = 3 s). Anything else is ignored. Harmless in production (nothing sets it); remove the key to go back to normal.
 */
export function genieDurationMs(base: number): number {
  try {
    const raw = new URLSearchParams(window.location.search).get("genieMs") ?? window.localStorage.getItem("spGenieMs");
    const value = raw === null ? Number.NaN : Number(raw);
    if (!Number.isFinite(value)) return base;
    if (value >= 1 && value < 50) return base * value;
    return value >= 50 && value <= 20000 ? value : base;
  } catch {
    return base;
  }
}
