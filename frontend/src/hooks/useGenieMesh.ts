import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";

import { blockedReason, disableForSession, getReadout, isAnimating, reportFallback, reportSnapshot, startGenie, type GenieRun } from "../lib/genieEngine";
import { capturePanel, SnapshotError, warmSnapshot, type PanelSnapshot } from "../lib/panelSnapshot";
import type { PresencePhase } from "./usePanelPresence";

/** A cached snapshot older than this is not trusted for the next open (content may have changed while closed). */
export const SNAPSHOT_TTL_MS = 120_000;
/** At most one capture per this long, whatever asks for it. */
export const MIN_CAPTURE_GAP_MS = 2_000;
const CHANGE_DEBOUNCE_MS = 400;
const SETTLE_TIMEOUT_MS = 600;

const snapshotCache = new Map<string, PanelSnapshot>();
let lastCaptureAt = Number.NEGATIVE_INFINITY;
let captureFailures = 0;

/** Test seam. */
export function resetGenieMeshCache() {
  snapshotCache.clear();
  lastCaptureAt = Number.NEGATIVE_INFINITY;
  captureFailures = 0;
}

/** Reduced motion and the single-column layout never use the mesh (they keep today's fade / vertical emerge). */
function environmentBlock(): string | null {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return null;
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return "reduced-motion";
  if (window.matchMedia("(max-width: 639px)").matches) return "single-column";
  return null;
}

function clearMesh(element: HTMLElement) {
  element.removeAttribute("data-mesh");
  element.style.height = "";
}

export interface GenieMeshOptions {
  root: RefObject<HTMLElement | null>;
  /** The element the shared canvas is appended into (GenieCanvas). */
  slot: RefObject<HTMLElement | null>;
  phase: PresencePhase;
  durationMs: number;
  trigger: () => HTMLElement | null;
}

/**
 * The mesh switch for PanelReveal. For opening / closing it tries to run the WebGL mesh (lib/genieEngine.ts) from a
 * cached snapshot; any doubt clears the `data-mesh` attribute and the CSS reveal plays exactly as before. While open it
 * captures the snapshot ahead of time. See DESIGN.md "Config panel reveal".
 */
export function useGenieMesh({ root, slot, phase, durationMs, trigger }: GenieMeshOptions) {
  const triggerRef = useRef(trigger);
  const durationRef = useRef(durationMs);
  const phaseRef = useRef(phase);
  const previousPhase = useRef<PresencePhase | null>(null);
  const snapshotRef = useRef<PanelSnapshot | null>(null);
  const dirtyRef = useRef(false);
  const runRef = useRef<GenieRun | null>(null);
  const progressRef = useRef<number | null>(null);
  const keyRef = useRef<string | null>(null);

  useLayoutEffect(() => {
    triggerRef.current = trigger;
    durationRef.current = durationMs;
    phaseRef.current = phase;
  });

  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    const before = previousPhase.current;
    previousPhase.current = phase;

    if (phase !== "opening" && phase !== "closing") {
      clearMesh(element);
      progressRef.current = null;
      return;
    }

    const fallback = (reason: string) => {
      clearMesh(element);
      progressRef.current = null;
      reportFallback(reason);
    };
    const blocked = environmentBlock() ?? blockedReason();
    if (blocked) { fallback(blocked); return; }

    // (before === phase happens when React re-runs the effect in dev strict mode; that is not a reversal)
    const reversal = before !== phase && (before === "opening" || before === "closing");
    if (reversal && progressRef.current === null) { fallback("reversal-without-mesh"); return; }

    const card = triggerRef.current();
    const clip = element.querySelector<HTMLElement>(".panel-reveal__clip");
    const fx = slot.current;
    if (!card || !clip || !fx) { fallback("no-card"); return; }
    const key = keyRef.current ?? card.getAttribute("aria-controls");
    keyRef.current = key;
    let snapshot = snapshotRef.current;
    if (!snapshot && key) {
      snapshot = snapshotCache.get(key) ?? null;
      if (snapshot) { snapshotRef.current = snapshot; dirtyRef.current = false; }
    }
    if (!snapshot) { fallback("no-snapshot"); return; }
    if (dirtyRef.current) { fallback("snapshot-stale"); return; }
    if (performance.now() - snapshot.takenAt > SNAPSHOT_TTL_MS) { fallback("snapshot-old"); return; }

    // Natural (full-height) geometry: the mesh attribute switches the CSS animation off before anything is measured.
    element.setAttribute("data-mesh", "run");
    element.style.height = "";
    const host = element.getBoundingClientRect();
    const natural = clip.getBoundingClientRect();
    if (Math.abs(natural.width - snapshot.width) > 1 || Math.abs(natural.height - snapshot.height) > 2) {
      dirtyRef.current = true;
      fallback("size-changed");
      return;
    }
    const cardBox = card.getBoundingClientRect();
    const opening = phase === "opening";
    const from = reversal ? (progressRef.current as number) : opening ? 1 : 0;
    const to = opening ? 0 : 1;
    const duration = reversal ? Math.max(120, durationRef.current * Math.abs(to - from)) : durationRef.current;

    const run = startGenie({
      host: fx,
      snapshot,
      card: { cx: cardBox.left + cardBox.width / 2 - host.left, y: cardBox.bottom - host.top, width: cardBox.width },
      opening,
      fromProgress: from,
      durationMs: duration,
      onLayout: (bottom) => { element.style.height = `${bottom}px`; },
      onEnd: (result) => {
        runRef.current = null;
        progressRef.current = to;
        if (opening) {
          // Final state is the real, fully laid-out panel; a bailed run fades it in over 120 ms instead.
          clearMesh(element);
          if (result === "bailed") element.setAttribute("data-mesh", "fade");
        } else {
          element.style.height = "";
          element.setAttribute("data-mesh", "collapsed");
        }
      },
    });
    if (!run) { clearMesh(element); progressRef.current = null; return; }
    runRef.current = run;
    progressRef.current = from;
    return () => {
      const current = runRef.current;
      if (current) { progressRef.current = current.cancel(); runRef.current = null; }
    };
  }, [phase, root, slot]);

  // Settled and open: capture the snapshot ahead of time (idle, rate-limited); re-capture only after a debounced change.
  useEffect(() => {
    if (phase !== "open") return;
    const element = root.current;
    const clip = element?.querySelector<HTMLElement>(".panel-reveal__clip");
    const panel = element?.querySelector<HTMLElement>(".panel-reveal__panel");
    if (!element || !clip || !panel) return;
    let disposed = false;
    let timer: number | undefined;
    let idle: number | undefined;
    let debounce: number | undefined;
    let sawFirstResize = false;

    const cancelScheduled = () => {
      window.clearTimeout(timer);
      if (idle !== undefined && typeof window.cancelIdleCallback === "function") window.cancelIdleCallback(idle);
      idle = undefined;
    };

    const capture = async () => {
      if (disposed || phaseRef.current !== "open" || isAnimating()) return;
      if (environmentBlock() ?? blockedReason()) return;
      if (snapshotRef.current && !dirtyRef.current) return;
      const wait = MIN_CAPTURE_GAP_MS - (performance.now() - lastCaptureAt);
      if (wait > 0) { timer = window.setTimeout(schedule, wait); return; }
      lastCaptureAt = performance.now();
      try {
        if (!getReadout().snapshotMs) await warmSnapshot(clip);
        if (disposed || phaseRef.current !== "open" || isAnimating()) return;
        const snapshot = await capturePanel(clip);
        if (disposed || phaseRef.current !== "open") return;
        captureFailures = 0;
        if (!reportSnapshot(snapshot)) return; // over budget: the mesh is off for the session
        snapshotRef.current = snapshot;
        dirtyRef.current = false;
        if (keyRef.current) snapshotCache.set(keyRef.current, snapshot);
      } catch (error) {
        const reason = error instanceof SnapshotError ? error.reason : "error";
        reportFallback(`snapshot-${reason}`);
        captureFailures += 1;
        if (captureFailures >= 2) disableForSession(`snapshot failed twice (${reason})`);
      }
    };

    function schedule() {
      cancelScheduled();
      const run = () => { idle = undefined; void capture(); };
      if (typeof window.requestIdleCallback === "function") idle = window.requestIdleCallback(run, { timeout: SETTLE_TIMEOUT_MS });
      else timer = window.setTimeout(run, 150);
    }

    const markChanged = () => {
      dirtyRef.current = true;
      window.clearTimeout(debounce);
      debounce = window.setTimeout(schedule, CHANGE_DEBOUNCE_MS);
    };

    // ResizeObserver only (no MutationObserver on the live timeline); form edits are caught from the events they raise.
    let observer: ResizeObserver | undefined;
    if (typeof ResizeObserver === "function") {
      observer = new ResizeObserver(() => {
        if (!sawFirstResize) { sawFirstResize = true; return; }
        markChanged();
      });
      observer.observe(panel);
    }
    element.addEventListener("input", markChanged);
    element.addEventListener("change", markChanged);
    if (!snapshotRef.current || dirtyRef.current) schedule();

    return () => {
      disposed = true;
      cancelScheduled();
      window.clearTimeout(debounce);
      observer?.disconnect();
      element.removeEventListener("input", markChanged);
      element.removeEventListener("change", markChanged);
    };
  }, [phase, root]);
}
