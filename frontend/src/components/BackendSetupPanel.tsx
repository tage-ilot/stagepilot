import { useEffect, useMemo, useRef, useState } from "react";

import { apiOrigin, websocketUrl } from "../api";
import { copyDiagnosticsLog, sendDiagnosticsBundle } from "../diagnostics";
import { useRemoteAccess } from "../hooks/useRemoteAccess";
import type {
  ApplicationState,
  GeneralSettingsInput,
  HealthResponse,
  SettingsResponse,
} from "../types";
import { ButtonSpinner, RemoteAccessPanel } from "./RemoteAccessPanel";
import { SetupPanelHeader } from "./SetupPanelHeader";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { Input } from "./ui/Input";
import { InlineAlert } from "./ui/InlineAlert";
import { Panel } from "./ui/Panel";
import { Select } from "./ui/Select";

/** Client-side cooldown (seconds) between manual "send all logs" clicks. */
const SEND_LOGS_COOLDOWN_SECONDS = 60;

/**
 * Copies the current backend log straight to the clipboard, entirely
 * offline/local -- no network call, no control-plane rate limit, no
 * installation/Remote Access requirement. Placed to the left of "Send all
 * logs to developer" as the quick local-only alternative: useful when the
 * developer is debugging interactively over a screen share, or Remote
 * Access/network isn't available.
 */
export function CopyLogButton() {
  const [status, setStatus] = useState<"idle" | "copying" | "copied" | "error">("idle");
  const [message, setMessage] = useState<string | null>(null);

  const onClick = async () => {
    if (status === "copying") return;
    setStatus("copying");
    setMessage(null);
    try {
      const content = await copyDiagnosticsLog();
      await navigator.clipboard.writeText(content);
      setStatus("copied");
      setMessage("Backend log copied to the clipboard.");
    } catch (error) {
      setStatus("error");
      setMessage(error instanceof Error ? error.message : "Unable to copy the StagePilot log.");
    }
  };

  return (
    <Button
      disabled={status === "copying"}
      onClick={() => void onClick()}
      title={message ?? undefined}
      variant="secondary"
    >
      {status === "copying" ? "Copying…" : "Copy Log"}
    </Button>
  );
}

/**
 * Manually-triggered "send everything right now" button, distinct from the
 * crash-triggered flows in `ErrorBoundary`/`CrashLoopAlertDialog`. Shares
 * the same `sendDiagnosticsBundle()` helper (same endpoint/auth/storage as
 * those flows) and layers a client-side 1/min cooldown on top purely as a
 * UX nicety -- the server-side `diagnosticsRate` limit on the control
 * plane (6/hour) is the real abuse backstop.
 */
export function SendAllLogsButton() {
  const [status, setStatus] = useState<"idle" | "sending" | "sent" | "error">("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [cooldown, setCooldown] = useState(0);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, []);

  const startCooldown = () => {
    setCooldown(SEND_LOGS_COOLDOWN_SECONDS);
    if (intervalRef.current) clearInterval(intervalRef.current);
    intervalRef.current = setInterval(() => {
      setCooldown((previous) => {
        if (previous <= 1) {
          if (intervalRef.current) clearInterval(intervalRef.current);
          intervalRef.current = null;
          return 0;
        }
        return previous - 1;
      });
    }, 1000);
  };

  const onClick = async () => {
    if (cooldown > 0 || status === "sending") return;
    setStatus("sending");
    setMessage(null);
    const result = await sendDiagnosticsBundle();
    setStatus(result.ok ? "sent" : "error");
    setMessage(result.message);
    startCooldown();
  };

  const disabled = status === "sending" || cooldown > 0;
  const label = status === "sending"
    ? "Sending…"
    : cooldown > 0
      ? `Send all logs to developer (${cooldown}s)`
      : "Send all logs to developer";

  return (
    <div className="mt-4 flex flex-wrap items-center gap-3 border-t border-edge-subtle pt-4">
      <CopyLogButton />
      <Button disabled={disabled} onClick={() => void onClick()} variant="secondary">
        {label}
      </Button>
      <p className="text-xs text-ink-muted">
        Sends recent backend/frontend logs and app info straight to the developer, for support purposes. Limited to once per minute.
      </p>
      {message && (
        <p
          aria-live="polite"
          className={`w-full text-xs ${status === "error" ? "text-danger-text" : "text-success-text"}`}
        >
          {message}
        </p>
      )}
    </div>
  );
}

export function BackendSetupPanel({
  state,
  health,
  live,
  onClose,
  settings,
  error,
  message,
  pending,
  onSave,
}: {
  state: ApplicationState;
  health: HealthResponse | null;
  live: boolean;
  onClose: () => void;
  settings: SettingsResponse | null;
  error: string | null;
  message: string | null;
  pending: boolean;
  onSave: (settings: GeneralSettingsInput) => void;
}) {
  const [timezone, setTimezone] = useState("America/Los_Angeles");
  const [logLevel, setLogLevel] = useState<GeneralSettingsInput["log_level"]>("INFO");
  const [serverPort, setServerPort] = useState("8765");
  const [lanAccess, setLanAccess] = useState(false);
  const [pinEnabled, setPinEnabled] = useState(true);
  const [dashboardPin, setDashboardPin] = useState("");
  const [releaseChannel, setReleaseChannel] = useState<GeneralSettingsInput["release_channel"]>("BETA");
  const remote = useRemoteAccess();

  useEffect(() => {
    if (!settings) return;
    setTimezone(settings.settings.timezone);
    setLogLevel(settings.settings.log_level);
    setServerPort(String(settings.settings.server_port));
    setLanAccess(settings.settings.lan_access ?? false);
    setPinEnabled(settings.settings.web_dashboard_pin_enabled ?? true);
    setDashboardPin("");
    setReleaseChannel(settings.settings.release_channel ?? "BETA");
  }, [settings]);

  const parsedSettings = useMemo<GeneralSettingsInput | null>(() => {
    const parsedPort = Number(serverPort);
    if (!timezone.trim()) return null;
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) return null;
    if (dashboardPin && dashboardPin.length < 4) return null;
    return {
      timezone: timezone.trim(),
      log_level: logLevel,
      server_port: parsedPort,
      lan_access: lanAccess,
      web_dashboard_pin_enabled: pinEnabled,
      release_channel: releaseChannel,
      ...(dashboardPin ? { web_dashboard_pin: dashboardPin } : {}),
    };
  }, [dashboardPin, lanAccess, logLevel, pinEnabled, releaseChannel, serverPort, timezone]);
  const connectionStatus = live
    ? "connected"
    : state.application_status === "error" ? "error" : "disconnected";

  return (
    <Panel
      aria-labelledby="backend-setup-heading"
      className="mt-5"
      id="backend-configuration"
      variant="setup"
    >
      <SetupPanelHeader
        closeLabel="Close StagePilot configuration"
        description="Shows the runtime identity, local API endpoints, WebSocket state and connection health for this session."
        headingId="backend-setup-heading"
        onClose={onClose}
        status={connectionStatus}
        title="StagePilot"
      />

      <h3 className="mt-5 font-semibold text-ink">Status</h3>
      <dl className="mt-3 grid gap-x-4 gap-y-4 text-sm sm:grid-cols-2 xl:grid-cols-4">
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">API endpoint</dt>
          <dd className="mt-1 break-all font-mono text-xs text-ink">{apiOrigin}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">WebSocket</dt>
          <dd className={`mt-1 font-semibold ${live ? "text-success-text" : "text-danger-text"}`}>
            {live ? "Connected" : "Not connected"}
          </dd>
          <dd className="mt-1 break-all font-mono text-type-micro text-ink-muted">{websocketUrl}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">Backend version</dt>
          <dd className="mt-1 text-ink">{health ? `v${health.version}` : "Loading"}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">State revision</dt>
          <dd className="mt-1 tabular-nums text-ink">{state.revision}</dd>
        </div>
      </dl>

      <div className="mt-5">
        <p className="text-xs font-bold uppercase tracking-wider text-ink-body">Connection health</p>
        <div className="mt-3 grid gap-x-4 gap-y-4 sm:grid-cols-2 xl:grid-cols-3">
          {(health?.plugins ?? Object.values(state.plugins)).map((plugin) => (
            <div key={plugin.name}>
              <div className="flex items-center justify-between gap-2">
                <p className="font-semibold text-ink">{plugin.name}</p>
                <span className="text-xs capitalize text-ink-muted">{plugin.status}</span>
              </div>
              <p className="mt-1 text-xs text-ink-muted">v{plugin.version}{plugin.last_error ? ` · ${plugin.last_error}` : ""}</p>
            </div>
          ))}
          {!health?.plugins.length && !Object.keys(state.plugins).length && (
            <p className="text-sm text-ink-muted">No connections reported.</p>
          )}
        </div>
      </div>

      <h3 className="mt-5 border-t border-edge-subtle pt-4 font-semibold text-ink">Basic</h3>
      <div className="mt-3 grid gap-x-4 gap-y-4 lg:grid-cols-3">
        <Field label="Timezone">
          <Input
            disabled={pending}
            onChange={(event) => setTimezone(event.target.value)}
            placeholder="America/Los_Angeles"
            value={timezone}
          />
        </Field>
        <label className="flex touch-manipulation items-start gap-3 text-sm text-ink-body">
          <input
            checked={lanAccess}
            className="mt-0.5 size-4 accent-primary"
            disabled={pending}
            onChange={(event) => setLanAccess(event.target.checked)}
            type="checkbox"
          />
          <span>
            <span className="block font-semibold text-ink">
              Allow dashboard access from this local network
            </span>
            <span className="mt-1 block text-xs text-ink-muted">
              Other devices can open http://&lt;this-computer&apos;s-IP&gt;:{serverPort}.
            </span>
          </span>
        </label>
        <div className="text-sm text-ink-body">
          <label className="flex touch-manipulation items-start gap-3">
            <input
              checked={pinEnabled}
              className="mt-0.5 size-4 accent-primary"
              disabled={pending}
              onChange={(event) => setPinEnabled(event.target.checked)}
              type="checkbox"
            />
            <span>
              <span className="block font-semibold text-ink">
                Require a PIN for web dashboards
              </span>
              <span className="mt-1 block text-xs text-ink-muted">
                Enabled by default. The initial PIN is 1234.
              </span>
            </span>
          </label>
          {pinEnabled && (
            <Field className="mt-3" label="Replace dashboard PIN">
              <Input
                autoComplete="new-password"
                className="font-mono"
                disabled={pending}
                inputMode="numeric"
                maxLength={64}
                minLength={4}
                onChange={(event) => setDashboardPin(event.target.value)}
                placeholder="Leave blank to keep the saved PIN"
                type="password"
                value={dashboardPin}
              />
            </Field>
          )}
        </div>
      </div>

      <h3 className="mt-5 border-t border-edge-subtle pt-4 font-semibold text-ink">People</h3>
      <div className="mt-3 space-y-4">
        <label className="flex min-h-ds-touch-target touch-manipulation items-start gap-3 text-sm text-ink-body">
          <input
            checked={remote.enabled}
            className="mt-0.5 size-4 accent-primary"
            disabled={remote.checkboxBusy}
            onChange={(event) => {
              if (event.target.checked) remote.requestEnable();
              else remote.requestDisable();
            }}
            type="checkbox"
          />
          <span>
            <span className="inline-flex items-center gap-2">
              <span className="block font-semibold text-ink">
                Remote Access
              </span>
              {remote.checkboxBusy && <ButtonSpinner />}
            </span>
            <span className="mt-1 block text-xs text-ink-muted">
              Securely view or operate this StagePilot from another device.
            </span>
          </span>
        </label>
        {remote.panelOpen && <RemoteAccessPanel control={remote} />}
      </div>

      <h3 className="mt-5 border-t border-edge-subtle pt-4 font-semibold text-ink">Advanced</h3>
      <div className="mt-3 grid gap-4 sm:grid-cols-3">
        <Field label="Server port">
          <Input
            disabled={pending}
            inputMode="numeric"
            onChange={(event) => setServerPort(event.target.value)}
            value={serverPort}
          />
        </Field>
        <Field label="Log level">
          <Select
            disabled={pending}
            onChange={(event) => setLogLevel(event.target.value as GeneralSettingsInput["log_level"])}
            value={logLevel}
          >
            {(["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"] as const).map((level) => (
              <option key={level} value={level}>{level}</option>
            ))}
          </Select>
        </Field>
        <Field label="Release channel">
          <Select
            disabled={pending}
            onChange={(event) => setReleaseChannel(event.target.value as GeneralSettingsInput["release_channel"])}
            value={releaseChannel}
          >
            {(["STABLE", "BETA"] as const).map((channel) => (
              <option key={channel} value={channel}>{channel}</option>
            ))}
          </Select>
        </Field>
      </div>

      <div className="mt-4 flex flex-wrap items-center gap-3">
        <Button
          disabled={pending || parsedSettings === null}
          onClick={() => parsedSettings && onSave(parsedSettings)}
          variant="primary"
        >
          {pending ? "Saving…" : "Save general settings"}
        </Button>
        <p className="text-xs text-ink-muted">
          Timezone, logging, port, and network-access changes take effect after a backend restart. PIN changes apply immediately.
        </p>
      </div>

      {(error || message) && (
        <InlineAlert className="mt-3" tone={error ? "danger" : "info"}>
          {error ?? message}
        </InlineAlert>
      )}

      <p className="mt-4 text-xs text-ink-muted">
        The dashboard remembers the saved server port for its next launch. Environment variables may still override these values for development.
      </p>


      <SendAllLogsButton />
    </Panel>
  );
}
