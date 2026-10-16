import { useEffect, useRef, useState, type CSSProperties } from "react";

import { Button } from "./components/ui";
import { Dashboard } from "./components/Dashboard";
import { DesktopTitleBar } from "./components/DesktopTitleBar";
import { DashboardAccessGate } from "./components/DashboardAccessGate";
import { CrashLoopAlertDialog } from "./components/CrashLoopAlertDialog";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { GlobalErrorListener } from "./components/GlobalErrorListener";
import {
  backendStartupTitle,
  copyBackendLog,
  desktopBackendStatus,
  listenForDesktopBackend,
  listenForDesktopBackendCrashLoop,
  restartDesktopBackend,
  type BackendCrashLoopDetected,
  type BackendSupervisorStatus,
} from "./desktop";
import { useDashboardAccess } from "./access/AccessContext";
import { useStagePilot } from "./hooks/useStagePilot";
import { useUpdater } from "./hooks/useUpdater";
import { rememberLoadingLogo } from "./startup/logoLanding";
import { useStartupProgress } from "./startup/useStartupProgress";

function StagePilotApp() {
  const access = useDashboardAccess();
  const stagePilot = useStagePilot();
  const {
    activateConfiguredServices,
    settings: stagePilotSettings,
    planningCenterStatus,
  } = stagePilot;
  const [backendSupervisor, setBackendSupervisor] = useState<BackendSupervisorStatus | null>(null);
  const [dashboardVisible, setDashboardVisible] = useState(false);
  const [backendActionMessage, setBackendActionMessage] = useState<string | null>(null);
  const [retryingBackend, setRetryingBackend] = useState(false);
  const [startupAttempt, setStartupAttempt] = useState(0);
  const startupServicesActivated = useRef(false);
  const loadingLogo = useRef<HTMLHeadingElement>(null);
  const startupProgress = useStartupProgress({
    supervisorState: backendSupervisor?.state ?? null,
    applicationStateLoaded: Boolean(stagePilot.state),
    connectionEstablished: Boolean(stagePilot.health || stagePilot.live),
    retrying: retryingBackend,
    startupAttempt,
  });
  const updater = useUpdater({
    ready: dashboardVisible && Boolean(stagePilot.state),
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
    const reveal = window.setTimeout(() => {
      // The dashboard's own logo starts where this big one is and flies to the header (see startup/logoLanding.ts).
      rememberLoadingLogo(loadingLogo.current);
      setDashboardVisible(true);
    }, 40);
    return () => window.clearTimeout(reveal);
  }, [dashboardVisible, stagePilot.state, startupProgress.complete]);

  useEffect(() => {
    // Planning Center activation needs `planningCenterStatus` to know whether
    // an OAuth-only account (no PAT) is connected; that status is fetched
    // asynchronously by useStagePilot, so it can still be null when this
    // effect first fires. Without waiting for it, the readiness check in
    // `activateConfiguredServices` sees neither a PAT nor a resolved OAuth
    // status, so Planning Center activation is skipped -- and, because this
    // effect only ever runs once (`startupServicesActivated`), it is never
    // retried once the status arrives. Hold the one-shot timer until either
    // Planning Center isn't the configured service source, or its status has
    // resolved.
    const planningCenterReady =
      stagePilotSettings?.settings?.integration_modes?.service_source !== "planning_center"
      || planningCenterStatus !== null;
    if (
      !access.capabilities.canActivateServices
      || !dashboardVisible
      || !stagePilotSettings
      || !planningCenterReady
      || startupServicesActivated.current
    ) return;
    const activate = window.setTimeout(() => {
      startupServicesActivated.current = true;
      void activateConfiguredServices();
    }, 1_000);
    return () => window.clearTimeout(activate);
  }, [
    access.capabilities.canActivateServices,
    activateConfiguredServices,
    dashboardVisible,
    planningCenterStatus,
    stagePilotSettings,
  ]);

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
        const content = await copyBackendLog();
        await navigator.clipboard.writeText(content);
        setBackendActionMessage("Backend log copied.");
      } catch (error) {
        try {
          await navigator.clipboard.writeText(backendSupervisor.log_path);
          setBackendActionMessage("Backend log path copied.");
        } catch {
          setBackendActionMessage(
            error instanceof Error ? error.message : `Backend log: ${backendSupervisor.log_path}`,
          );
        }
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
          <div className="w-full max-w-4xl overflow-hidden px-2">
            <h1 className="select-none font-brand text-6xl leading-none text-ink sm:text-8xl lg:text-[11.25rem]" ref={loadingLogo}>
              StagePilot
            </h1>
            <span className="loading-spinner-circular loading-spinner-circular--mobile-only" aria-hidden="true" />
            <div
              aria-label="StagePilot startup progress"
              aria-valuemax={100}
              aria-valuemin={1}
              aria-valuenow={Math.round(startupProgress.value)}
              aria-valuetext={startupProgress.valueText}
              className={`loading-progress-track loading-progress-track--${startupProgress.tone} loading-progress-track--desktop-only mx-auto mt-10 w-full max-w-xl`}
              role="progressbar"
            >
              <div
                className={`loading-progress-fill ${startupProgress.moving ? "loading-progress-fill--moving" : ""}`}
                style={{ "--progress": startupProgress.value / 100 } as CSSProperties}
              >
                <span className="loading-progress-scan" />
              </div>
            </div>
            <p className="mt-6 text-lg font-semibold text-ink">
              {progressHeadline}
            </p>
            {showStartupDetails && (
              <p
                aria-live={startupProgress.phase === "failed" ? "assertive" : undefined}
                className={`mt-2 text-sm ${backendSupervisor?.state === "failed" ? "text-danger-text" : "text-ink-muted"}`}
              >
                {backendSupervisor?.message ?? "Waiting for StagePilot."}
              </p>
            )}
            {startupProgress.phase === "failed" && (
              <div className="mt-6 flex flex-wrap justify-center gap-3">
                {backendSupervisor?.managed && (
                  <Button
                    onClick={() => void retryBackend()}
                    size="touch"
                    variant="destructive-soft"
                  >
                    Retry Backend
                  </Button>
                )}
                {backendSupervisor?.log_path && (
                  <Button
                    onClick={() => void copyLogPath()}
                    size="touch"
                    variant="secondary"
                  >
                    Copy Backend Log
                  </Button>
                )}
              </div>
            )}
            {backendActionMessage && (
              <p aria-live="polite" className="mt-3 text-sm text-ink-body">
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
      <Dashboard {...stagePilot} capabilities={access.capabilities} state={stagePilot.state} updater={updater} />
    </>
  );
}

export default function App() {
  const [crashLoop, setCrashLoop] = useState<BackendCrashLoopDetected | null>(null);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    void listenForDesktopBackendCrashLoop((payload) => {
      if (active) setCrashLoop(payload);
    }).then((nextUnlisten) => {
      if (!active) nextUnlisten?.();
      else unlisten = nextUnlisten;
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  return (
    <ErrorBoundary>
      <DashboardAccessGate>
        <StagePilotApp />
        <CrashLoopAlertDialog crashLoop={crashLoop} onDismiss={() => setCrashLoop(null)} />
        <GlobalErrorListener />
      </DashboardAccessGate>
    </ErrorBoundary>
  );
}
