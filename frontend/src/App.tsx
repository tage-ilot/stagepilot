import { useEffect, useRef, useState } from "react";

import { Dashboard } from "./components/Dashboard";
import { DesktopTitleBar } from "./components/DesktopTitleBar";
import { WebDashboardPinGate } from "./components/WebDashboardPinGate";
import {
  backendStartupTitle,
  desktopBackendStatus,
  listenForDesktopBackend,
  restartDesktopBackend,
  type BackendSupervisorStatus,
} from "./desktop";
import { useStagePilot } from "./hooks/useStagePilot";
import { useUpdater } from "./hooks/useUpdater";
import { useStartupProgress } from "./startup/useStartupProgress";

function StagePilotApp() {
  const stagePilot = useStagePilot();
  const {
    activateConfiguredServices,
    settings: stagePilotSettings,
  } = stagePilot;
  const [backendSupervisor, setBackendSupervisor] = useState<BackendSupervisorStatus | null>(null);
  const [dashboardVisible, setDashboardVisible] = useState(false);
  const [backendActionMessage, setBackendActionMessage] = useState<string | null>(null);
  const [retryingBackend, setRetryingBackend] = useState(false);
  const [startupAttempt, setStartupAttempt] = useState(0);
  const startupServicesActivated = useRef(false);
  const startupProgress = useStartupProgress({
    supervisorState: backendSupervisor?.state ?? null,
    applicationStateLoaded: Boolean(stagePilot.state),
    connectionEstablished: Boolean(stagePilot.health || stagePilot.live),
    retrying: retryingBackend,
    startupAttempt,
  });
  const updater = useUpdater({
    ready: dashboardVisible && Boolean(stagePilot.state),
    betaEnabled: stagePilotSettings?.settings.beta_updates_enabled ?? false,
  });

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    void desktopBackendStatus()
      .then((status) => {
        if (active && status) setBackendSupervisor(status);
      })
      .catch(() => undefined);
    void listenForDesktopBackend((status) => {
      if (active) setBackendSupervisor(status);
    }).then((nextUnlisten) => {
      if (!active) nextUnlisten?.();
      else unlisten = nextUnlisten;
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (
      backendSupervisor?.state === "ready" ||
      backendSupervisor?.state === "external"
    ) {
      setRetryingBackend(false);
    }
  }, [backendSupervisor?.state]);

  useEffect(() => {
    if (!stagePilot.state || !startupProgress.complete || dashboardVisible) return;
    const reveal = window.setTimeout(() => setDashboardVisible(true), 40);
    return () => window.clearTimeout(reveal);
  }, [dashboardVisible, stagePilot.state, startupProgress.complete]);

  useEffect(() => {
    if (!dashboardVisible || !stagePilotSettings || startupServicesActivated.current) return;
    const activate = window.setTimeout(() => {
      startupServicesActivated.current = true;
      void activateConfiguredServices();
    }, 1_000);
    return () => window.clearTimeout(activate);
  }, [activateConfiguredServices, dashboardVisible, stagePilotSettings]);

  if (!stagePilot.state || !dashboardVisible) {
    const retryBackend = async () => {
      setStartupAttempt((current) => current + 1);
      setRetryingBackend(true);
      setBackendActionMessage("Retrying the packaged backend…");
      try {
        await restartDesktopBackend();
        setRetryingBackend(false);
        setBackendActionMessage(null);
      } catch (error) {
        setRetryingBackend(false);
        setBackendActionMessage(error instanceof Error ? error.message : "Backend retry failed.");
      }
    };
    const copyLogPath = async () => {
      if (!backendSupervisor?.log_path) return;
      try {
        await navigator.clipboard.writeText(backendSupervisor.log_path);
        setBackendActionMessage("Backend log path copied.");
      } catch {
        setBackendActionMessage(`Backend log: ${backendSupervisor.log_path}`);
      }
    };
    const progressHeadline =
      startupProgress.phase === "failed"
        ? backendStartupTitle(backendSupervisor)
        : startupProgress.headline;
    const showStartupDetails = startupProgress.phase === "failed";
    return (
      <>
        <DesktopTitleBar />
        <main className="grid min-h-[calc(100vh-2.25rem)] place-items-center px-6 text-center">
          <div className="w-full max-w-4xl">
            <h1 className="select-none font-brand text-[11.25rem] leading-none text-white">StagePilot</h1>
            <div
              aria-label="StagePilot startup progress"
              aria-valuemax={100}
              aria-valuemin={1}
              aria-valuenow={Math.round(startupProgress.value)}
              aria-valuetext={startupProgress.valueText}
              className={`loading-progress-track loading-progress-track--${startupProgress.tone} mx-auto mt-10 w-full max-w-xl`}
              role="progressbar"
            >
              <div
                className={`loading-progress-fill ${startupProgress.moving ? "loading-progress-fill--moving" : ""}`}
                style={{ width: `${startupProgress.value}%` }}
              >
                <span className="loading-progress-scan" />
              </div>
            </div>
            <p className="mt-6 text-lg font-semibold text-white">
              {progressHeadline}
            </p>
            {showStartupDetails && (
              <p
                aria-live={startupProgress.phase === "failed" ? "assertive" : undefined}
                className={`mt-2 text-sm ${backendSupervisor?.state === "failed" ? "text-rose-300" : "text-slate-400"}`}
              >
                {backendSupervisor?.message ?? "Waiting for the StagePilot backend."}
              </p>
            )}
            {startupProgress.phase === "failed" && (
              <div className="mt-6 flex flex-wrap justify-center gap-3">
                {backendSupervisor?.managed && (
                  <button
                    className="rounded-lg border border-rose-300/50 bg-rose-950/60 px-4 py-2 text-sm font-semibold text-rose-100 hover:bg-rose-900/70"
                    onClick={() => void retryBackend()}
                    type="button"
                  >
                    Retry Backend
                  </button>
                )}
                {backendSupervisor?.log_path && (
                  <button
                    className="rounded-lg border border-slate-400/40 bg-slate-950/60 px-4 py-2 text-sm font-semibold text-slate-100 hover:bg-slate-800/70"
                    onClick={() => void copyLogPath()}
                    type="button"
                  >
                    Copy Log Path
                  </button>
                )}
              </div>
            )}
            {backendActionMessage && (
              <p aria-live="polite" className="mt-3 text-sm text-slate-300">
                {backendActionMessage}
              </p>
            )}
          </div>
        </main>
      </>
    );
  }

  return (
    <>
      <DesktopTitleBar />
      <Dashboard {...stagePilot} state={stagePilot.state} updater={updater} />
    </>
  );
}

export default function App() {
  return (
    <WebDashboardPinGate>
      <StagePilotApp />
    </WebDashboardPinGate>
  );
}
