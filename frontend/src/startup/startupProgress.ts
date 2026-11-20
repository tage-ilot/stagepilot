import type { BackendSupervisorStatus } from "../desktop";

export type StartupPhase =
  | "initializing"
  | "starting-backend"
  | "backend-ready"
  | "loading-state"
  | "preparing-dashboard"
  | "complete"
  | "failed";

export type StartupTone = "normal" | "delayed" | "failed" | "complete";

export type StartupProgressInput = {
  supervisorState: BackendSupervisorStatus["state"] | null;
  applicationStateLoaded: boolean;
  connectionEstablished: boolean;
  retrying: boolean;
  startupAttempt: number;
};

export type StartupProgressView = {
  value: number;
  phase: StartupPhase;
  tone: StartupTone;
  headline: string;
  valueText: string;
  moving: boolean;
  complete: boolean;
};

export const STARTUP_PROGRESS_MINIMUM = 1;

export const STARTUP_PHASE_CEILINGS: Readonly<Record<StartupPhase, number>> = {
  initializing: 10,
  "starting-backend": 88,
  "backend-ready": 92,
  "loading-state": 94,
  "preparing-dashboard": 100,
  complete: 100,
  failed: 99,
};

const PHASE_TIME_CONSTANTS_MS: Readonly<Partial<Record<StartupPhase, number>>> = {
  initializing: 650,
  "starting-backend": 16_000,
  "backend-ready": 700,
  "loading-state": 600,
};

export const deriveStartupPhase = (
  input: StartupProgressInput,
  totalElapsedMs: number,
): StartupPhase => {
  if (
    !input.retrying &&
    (input.supervisorState === "failed" || input.supervisorState === "stopped")
  ) {
    return "failed";
  }
  if (input.applicationStateLoaded) return "preparing-dashboard";
  if (input.connectionEstablished) return "loading-state";
  if (input.supervisorState === "ready" || input.supervisorState === "external") {
    return "backend-ready";
  }
  if (input.retrying || input.supervisorState === "starting") return "starting-backend";
  return totalElapsedMs < 400 ? "initializing" : "starting-backend";
};

export const timeBasedProgress = ({
  phase,
  elapsedMs,
  startingValue,
}: {
  phase: StartupPhase;
  elapsedMs: number;
  startingValue: number;
}): number => {
  const safeElapsed = Number.isFinite(elapsedMs) ? Math.max(0, elapsedMs) : 0;
  const safeStart = Number.isFinite(startingValue)
    ? Math.max(STARTUP_PROGRESS_MINIMUM, Math.min(100, startingValue))
    : STARTUP_PROGRESS_MINIMUM;
  const ceiling = STARTUP_PHASE_CEILINGS[phase];
  if (phase === "failed") return Math.min(safeStart, ceiling);
  if (phase === "complete") return 100;
  if (phase === "preparing-dashboard") {
    const duration = 480;
    const portion = Math.min(1, safeElapsed / duration);
    const eased = 1 - (1 - portion) ** 3;
    return Math.min(100, safeStart + (100 - safeStart) * eased);
  }
  if (safeStart >= ceiling) return safeStart;
  const timeConstant = PHASE_TIME_CONSTANTS_MS[phase] ?? 1_000;
  const portion = 1 - Math.exp(-safeElapsed / timeConstant);
  return Math.min(ceiling, safeStart + (ceiling - safeStart) * portion);
};

export const startupPresentation = ({
  phase,
  totalElapsedMs,
  retrying,
}: {
  phase: StartupPhase;
  totalElapsedMs: number;
  retrying: boolean;
}): Pick<StartupProgressView, "tone" | "headline" | "valueText" | "moving"> => {
  switch (phase) {
    case "failed":
      return {
        tone: "failed",
        headline: "StagePilot could not start",
        valueText: "Startup stopped because the engine reported an error.",
        moving: false,
      };
    case "complete":
      return {
        tone: "complete",
        headline: "StagePilot is ready",
        valueText: "Startup complete.",
        moving: false,
      };
    case "preparing-dashboard":
      return {
        tone: "normal",
        headline: "Preparing the dashboard",
        valueText: "Dashboard data is ready. Finishing startup.",
        moving: true,
      };
    case "loading-state":
      return {
        tone: "normal",
        headline: "Loading dashboard data",
        valueText: "The engine is connected. Loading application state.",
        moving: true,
      };
    case "backend-ready":
      return {
        tone: "normal",
        headline: "Checking the engine",
        valueText: "The engine is ready. Establishing the dashboard connection.",
        moving: true,
      };
    case "starting-backend": {
      const delayed = totalElapsedMs >= 8_000;
      return {
        tone: delayed ? "delayed" : "normal",
        headline: retrying
          ? "Retrying the engine"
          : delayed
            ? "The engine is taking a little longer than usual"
            : "Starting the engine",
        valueText: retrying
          ? "Retrying the engine's startup."
          : delayed
            ? "The engine is still starting."
            : "The engine is starting.",
        moving: true,
      };
    }
    case "initializing":
      return {
        tone: "normal",
        headline: "Preparing StagePilot",
        valueText: "Initializing the application shell.",
        moving: true,
      };
  }
};
