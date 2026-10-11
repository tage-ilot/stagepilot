import { useEffect, useRef, useState, type ReactNode } from "react";

export interface ScanProgressProps {
  state: "idle" | "running" | "done" | "failed";
  steps: number;
  total: number;
  previousDuration?: number | null;
  estimatedSongs?: number;
  children?: ReactNode;
}

// Time supplies the baseline; real events only raise its target. No event can
// set the displayed fill directly, and the pre-completion ceiling is absolute.
export function scanTarget(elapsed: number, tau: number, steps: number, total: number, previousFloor = 0): number {
  const floor = Math.max(previousFloor, Math.min(0.85, steps / (total > 0 ? total : 5) * 0.85));
  return Math.min(0.9, floor + (0.9 - floor) * (1 - Math.exp(-elapsed / tau)));
}
export function ScanProgress(props: ScanProgressProps) {
  const current = useRef(props);
  current.current = props;
  const clock = useRef({ start: 0, last: 0, value: 0, floor: 0, finish: null as number | null, finishValue: 0, wasRunning: false });
  const [value, setValue] = useState(0);
  const [visible, setVisible] = useState(props.state === "running");
  const [reduced, setReduced] = useState(false);
  useEffect(() => {
    const media = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    const update = () => setReduced(media?.matches ?? false);
    update(); media?.addEventListener?.("change", update);
    return () => media?.removeEventListener?.("change", update);
  }, []);
  useEffect(() => {
    let frame: number;
    const tick = (now: number) => {
      const p = current.current;
      const c = clock.current;
      if (p.state === "running" && !c.wasRunning) {
        c.start = now; c.last = now; c.value = 0; c.floor = 0; c.finish = null; c.wasRunning = true;
        setVisible(true);
      }
      const dt = Math.min(32, Math.max(0, now - c.last));
      if (p.state === "running") {
        const estimated = p.estimatedSongs || 5;
        const tau = Math.max(10_000, (p.previousDuration ?? 6 * estimated) * 1000);
        // Retain the step floor when a late count lowers the ratio: retaining
        // only the displayed value would freeze until the baseline caught up.
        c.floor = Math.max(c.floor, Math.min(0.85, p.steps / (p.total || estimated) * 0.85));
        const target = scanTarget(now - c.start, tau, p.steps, p.total || estimated, c.floor);
        const next = reduced ? Math.min(target, c.value + dt / 1000 * 0.02) : c.value + (target - c.value) * (1 - Math.exp(-dt / 130));
        c.value = Math.max(c.value, Math.min(0.9, next));
      } else if (c.wasRunning && p.state === "done") {
        if (c.finish === null) { c.finish = now; c.finishValue = c.value; }
        const fraction = Math.min(1, (now - c.finish) / 300);
        c.value = Math.max(c.value, reduced ? Math.min(1, c.value + dt / 300) : c.finishValue + (1 - c.finishValue) * (1 - (1 - fraction) ** 3));
        if (now - c.finish >= 650) { setVisible(false); c.wasRunning = false; }
      } else if (c.wasRunning) {
        // Fail/cancel never passes through the completion path.
        setVisible(false); c.wasRunning = false;
      }
      c.last = now;
      setValue(c.value);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [reduced]);
  if (!visible || props.state === "failed") return <>{props.children}</>;
  const determinate = props.total > 0 || props.state === "done";
  return <div className="py-2" aria-live="polite">
    <div role="progressbar" aria-label="Reading Playback song lengths" aria-valuemin={determinate ? 0 : undefined} aria-valuemax={determinate ? 100 : undefined} aria-valuenow={determinate ? Math.round(value * 1000) / 10 : undefined} data-progress={value} data-reduced-motion={reduced} className="h-1.5 w-full overflow-hidden rounded-ds-full bg-edge">
      <div className="h-full w-full origin-left rounded-ds-full bg-integration-playback" style={{ transform: `scaleX(${value})` }} />
    </div>
    <p className="mt-2 text-xs tabular-nums text-ink-muted">{props.state === "done" ? "Song scan complete." : "Reading song lengths… Please wait."}</p>
  </div>;
}
