/**
 * Dev hook: stretch the config-panel animation so its middle can be inspected. Read when the duration is computed
 * (every render, so it applies at the next open): `localStorage.setItem("spGenieMs", "3000")` or `?genieMs=3000`.
 * Values outside 50..20000 are ignored. Harmless in production (nothing sets it); remove the key to go back to normal.
 */
export function genieDurationMs(base: number): number {
  try {
    const raw = new URLSearchParams(window.location.search).get("genieMs") ?? window.localStorage.getItem("spGenieMs");
    const value = raw === null ? Number.NaN : Number(raw);
    return Number.isFinite(value) && value >= 50 && value <= 20000 ? value : base;
  } catch {
    return base;
  }
}
