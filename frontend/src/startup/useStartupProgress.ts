import { useEffect, useRef, useState } from "react";

import {
  deriveStartupPhase,
  STARTUP_PROGRESS_MINIMUM,
  startupPresentation,
  timeBasedProgress,
  type StartupPhase,
  type StartupProgressInput,
  type StartupProgressView,
} from "./startupProgress";

const UI_UPDATE_INTERVAL_MS = 80;
const REDUCED_MOTION_UPDATE_INTERVAL_MS = 200;
const MINIMUM_RENDERED_CHANGE = 0.08;

const initialView = (): StartupProgressView => ({
  value: STARTUP_PROGRESS_MINIMUM,
  phase: "initializing",
  tone: "normal",
  headline: "Preparing StagePilot",
  valueText: "Initializing the application shell.",
  moving: true,
  complete: false,
});

const usePrefersReducedMotion = (): boolean => {
  const [reduced, setReduced] = useState(
    () =>
      typeof window !== "undefined" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => setReduced(media.matches);
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  return reduced;
};

export const useStartupProgress = (
  input: StartupProgressInput,
  options: { reducedMotion?: boolean } = {},
): StartupProgressView => {
  const prefersReducedMotion = usePrefersReducedMotion();
  const reducedMotion = options.reducedMotion ?? prefersReducedMotion;
  const [view, setView] = useState<StartupProgressView>(initialView);
  const viewRef = useRef(view);
  const inputRef = useRef(input);
  const valueRef = useRef(STARTUP_PROGRESS_MINIMUM);
  const phaseRef = useRef<StartupPhase>("initializing");
  const startupStartedAtRef = useRef<number | null>(null);
  const phaseStartedAtRef = useRef<number | null>(null);
  const phaseStartingValueRef = useRef(STARTUP_PROGRESS_MINIMUM);
  const lastPublishedAtRef = useRef(0);
  inputRef.current = input;
  viewRef.current = view;

  useEffect(() => {
    let active = true;
    let frame = 0;
    let timer = 0;

    const tick = (now: number) => {
      if (!active) return;
      startupStartedAtRef.current ??= now;
      phaseStartedAtRef.current ??= now;
      const totalElapsedMs = Math.max(0, now - startupStartedAtRef.current);
      let phase = deriveStartupPhase(inputRef.current, totalElapsedMs);

      if (phase !== phaseRef.current) {
        if (phase === "failed") {
          valueRef.current = viewRef.current.value;
        }
        phaseRef.current = phase;
        phaseStartedAtRef.current = now;
        phaseStartingValueRef.current = valueRef.current;
      }

      const phaseElapsedMs = Math.max(0, now - (phaseStartedAtRef.current ?? now));
      const candidate = timeBasedProgress({
        phase,
        elapsedMs: phaseElapsedMs,
        startingValue: phaseStartingValueRef.current,
      });
      valueRef.current = Math.max(valueRef.current, candidate);
      if (phase === "preparing-dashboard" && valueRef.current >= 99.95) {
        valueRef.current = 100;
        phase = "complete";
        phaseRef.current = phase;
      }

      const presentation = startupPresentation({
        phase,
        totalElapsedMs,
        retrying: inputRef.current.retrying,
      });
      const currentView = viewRef.current;
      const shouldPublish =
        now - lastPublishedAtRef.current >=
          (reducedMotion ? REDUCED_MOTION_UPDATE_INTERVAL_MS : UI_UPDATE_INTERVAL_MS) ||
        phase !== currentView.phase ||
        Math.abs(valueRef.current - currentView.value) >= MINIMUM_RENDERED_CHANGE;
      if (shouldPublish) {
        lastPublishedAtRef.current = now;
        const nextView = {
          value: valueRef.current,
          phase,
          ...presentation,
          complete: phase === "complete",
        };
        viewRef.current = nextView;
        setView(nextView);
      }

      if (phase === "failed" || phase === "complete") return;
      if (reducedMotion) {
        timer = window.setTimeout(
          () => tick(performance.now()),
          REDUCED_MOTION_UPDATE_INTERVAL_MS,
        );
      } else {
        frame = window.requestAnimationFrame(tick);
      }
    };

    if (reducedMotion) {
      timer = window.setTimeout(() => tick(performance.now()), 0);
    } else {
      frame = window.requestAnimationFrame(tick);
    }
    return () => {
      active = false;
      window.cancelAnimationFrame(frame);
      window.clearTimeout(timer);
    };
  }, [
    input.applicationStateLoaded,
    input.connectionEstablished,
    input.retrying,
    input.startupAttempt,
    input.supervisorState,
    reducedMotion,
  ]);

  return view;
};

