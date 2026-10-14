import { useEffect, useMemo, useRef, useState } from "react";

import { SectionLabel } from "./ui/SectionLabel";
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
import { Disclosure } from "./ui/Disclosure";
import { Field } from "./ui/Field";
import { Input } from "./ui/Input";
import { PanelAlert } from "./PanelAlert";
import { Panel } from "./ui/Panel";
import { SaveBar, type SaveBarState } from "./ui/SaveBar";
import { useSaveAttempt } from "../hooks/useSaveAttempt";
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
          className={`w-full text-xs ${status === "error" ? "text-danger-text" : "text-integration-backend-text"}`}
        >
          {message}
        </p>
      )}
    </div>
  );
}

function HeldMarker() {
  return (
    <span className="mt-1 inline-flex items-center gap-1.5 text-type-body-sm text-warning-text" data-held-marker>
      <span aria-hidden="true" className="h-2 w-2 rounded-ds-full bg-warning" />
      Not saved yet
    </span>
  );
}

// Status facts are small bordered cards, like the style guide (a recessed well on the panel).
const factCard = "rounded-ds-lg border border-edge-subtle bg-surface-950/40 px-3 py-2.5";
const factLabel = "font-type-kicker text-type-kicker uppercase text-ink-muted";

export function BackendSetupPanel({
  state,
  health,
  live,
  onClose,
  onDirtyChange,
  settings,
  error,
  onRetryLoad,
  message,
  pending,
  onSave,
}: {
  state: ApplicationState;
  health: HealthResponse | null;
  live: boolean;
  onClose: () => void;
  onDirtyChange?: (dirty: boolean) => void;
  settings: SettingsResponse | null;
  error: string | null;
  onRetryLoad?: () => void;
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
  const [advancedOpen, setAdvancedOpen] = useState(false);

  // Each draft follows its own saved value.
  const savedSettings = settings?.settings ?? null;
  const syncedTimezone = savedSettings?.timezone ?? null;
  const syncedLogLevel = savedSettings?.log_level ?? null;
  const syncedPort = savedSettings ? String(savedSettings.server_port) : null;
  const syncedLan = savedSettings ? (savedSettings.lan_access ?? false) : null;
  const syncedPinEnabled = savedSettings ? (savedSettings.web_dashboard_pin_enabled ?? true) : null;
  const syncedChannel = savedSettings ? (savedSettings.release_channel ?? "BETA") : null;
  useEffect(() => { if (syncedTimezone !== null) setTimezone(syncedTimezone); }, [syncedTimezone]);
  useEffect(() => { if (syncedLogLevel !== null) setLogLevel(syncedLogLevel); }, [syncedLogLevel]);
  useEffect(() => { if (syncedPort !== null) setServerPort(syncedPort); }, [syncedPort]);
  useEffect(() => { if (syncedLan !== null) setLanAccess(syncedLan); }, [syncedLan]);
  useEffect(() => { if (syncedPinEnabled !== null) setPinEnabled(syncedPinEnabled); }, [syncedPinEnabled]);
  useEffect(() => { if (syncedChannel !== null) setReleaseChannel(syncedChannel); }, [syncedChannel]);

  const parsedSettings = useMemo<GeneralSettingsInput | null>(() => {
    const parsedPort = Number(serverPort);
    if (!timezone.trim()) return null;
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) return null;
    if (pinEnabled && dashboardPin && dashboardPin.length < 4) return null;
    return {
      timezone: timezone.trim(),
      log_level: logLevel,
      server_port: parsedPort,
      lan_access: lanAccess,
      web_dashboard_pin_enabled: pinEnabled,
      release_channel: releaseChannel,
      ...(pinEnabled && dashboardPin ? { web_dashboard_pin: dashboardPin } : {}),
    };
  }, [dashboardPin, lanAccess, logLevel, pinEnabled, releaseChannel, serverPort, timezone]);
  // Every field here is held until Save configuration. PUT /settings rewrites the whole settings
  // file, may reconfigure the MIDI port, and reports restart_required; the PIN endpoint signs out
  // every dashboard session (backend/src/stagepilot/api/dashboard_auth.py).
  const saved = settings?.settings ?? null;
  const heldTimezone = saved !== null && timezone !== saved.timezone;
  const heldPort = saved !== null && serverPort !== String(saved.server_port);
  const heldLan = saved !== null && lanAccess !== (saved.lan_access ?? false);
  const heldLogLevel = saved !== null && logLevel !== saved.log_level;
  const heldChannel = saved !== null && releaseChannel !== (saved.release_channel ?? "BETA");
  const heldPinEnabled = saved !== null && pinEnabled !== (saved.web_dashboard_pin_enabled ?? true);
  const heldPin = pinEnabled && dashboardPin !== "";
  const changeCount = [heldTimezone, heldPort, heldLan, heldLogLevel, heldChannel, heldPinEnabled, heldPin]
    .filter(Boolean).length;
  useEffect(() => { onDirtyChange?.(changeCount > 0); return () => onDirtyChange?.(false); }, [changeCount, onDirtyChange]);
  const [saveFailed, setSaveFailed] = useState(false);
  const attempt = useSaveAttempt("backend-configuration");
  const portValid = Number.isInteger(Number(serverPort)) && Number(serverPort) >= 1 && Number(serverPort) <= 65535;
  const errTimezone = attempt.attempted && !timezone.trim() ? "Enter a timezone." : undefined;
  const errPort = attempt.attempted && !portValid ? "Enter a port from 1 to 65535." : undefined;
  const errPin = attempt.attempted && pinEnabled && dashboardPin !== "" && dashboardPin.length < 4 ? "Enter a PIN of at least 4 characters." : undefined;
  // A problem or a waiting change inside Advanced must never hide behind the closed row.
  const advancedHeld = [heldPort, heldLogLevel, heldChannel].filter(Boolean).length;
  useEffect(() => { if (errPort) setAdvancedOpen(true); }, [errPort]);
  const advancedSummary = advancedHeld > 0
    ? `${advancedHeld} ${advancedHeld === 1 ? "change" : "changes"} waiting`
    : `Port ${serverPort} · Logging ${logLevel.toLowerCase()}`;
  const timezoneChoices = useMemo(() => {
    const zones = typeof Intl.supportedValuesOf === "function" ? Intl.supportedValuesOf("timeZone") : [];
    // Browsers leave "UTC" out of this list, so add it: a studio on UTC must be able to pick it.
    const known = ["UTC", ...(zones.length > 0 ? zones : ["America/Los_Angeles"])].filter((zone, at, all) => all.indexOf(zone) === at);
    // The saved zone is always choosable, even if the browser's list does not carry it.
    return timezone && !known.includes(timezone) ? [timezone, ...known] : known;
  }, [timezone]);
  const [autoSavedAt, setAutoSavedAt] = useState<string | null>(null);
  const wasPending = useRef(false);
  useEffect(() => {
    if (wasPending.current && !pending) {
      setSaveFailed(Boolean(error));
      if (!error) {
        setDashboardPin("");
        setAutoSavedAt(new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
      }
    }
    wasPending.current = pending;
  }, [pending, error]);
  const saveState: SaveBarState = pending
    ? { kind: "saving" }
    : saveFailed && changeCount > 0
      ? { kind: "error", message: "Couldn't save" }
      : changeCount > 0
        ? { kind: "held", count: changeCount }
        : autoSavedAt
          ? { kind: "auto", at: autoSavedAt }
          : { kind: "clean" };
  const blockedReason = parsedSettings === null
    ? "Enter a timezone, a port from 1 to 65535 and a PIN of at least 4 characters before saving."
    : null;
  const discard = () => {
    if (!saved) return;
    setTimezone(saved.timezone);
    setServerPort(String(saved.server_port));
    setLanAccess(saved.lan_access ?? false);
    setLogLevel(saved.log_level);
    setReleaseChannel(saved.release_channel ?? "BETA");
    setPinEnabled(saved.web_dashboard_pin_enabled ?? true);
    setDashboardPin("");
    setSaveFailed(false);
    attempt.reset();
  };
  const save = () => {
    // A clean Save is a no-op once settings are loaded.
    if (pending) return;
    if (!parsedSettings) { attempt.refuse(); return; }
    if (saved !== null && changeCount === 0) return;
    setSaveFailed(false);
    setAutoSavedAt(null);
    onSave(parsedSettings);
  };
  const connectionStatus = live
    ? "connected"
    : state.application_status === "error" ? "error" : "disconnected";

  return (
    <Panel
      aria-labelledby="backend-setup-heading"
      className="mt-5"
      id="backend-configuration"
      tone="backend"
      variant="setup"
    >
      <SetupPanelHeader
        closeLabel="Close StagePilot configuration"
        kicker="Backend configuration"
        headingId="backend-setup-heading"
        onClose={onClose}
        status={connectionStatus}
        title="StagePilot"
      />

      <SectionLabel>Status</SectionLabel>
      <div className="backend-status-wrap">
      <dl className="backend-status mt-3 grid gap-3 text-sm">
        <div className={factCard}>
          <dt className={factLabel}>Version</dt>
          <dd className="mt-1 font-semibold text-ink">{health ? health.version : "Loading"}</dd>
        </div>
        <div className={factCard}>
          <dt className={factLabel}>API endpoint</dt>
          <dd className="mt-1 break-all font-mono text-xs text-ink">{apiOrigin}</dd>
        </div>
        <div className={factCard}>
          <dt className={factLabel}>WebSocket</dt>
          <dd className={`mt-1 font-semibold ${live ? "text-integration-backend-text" : "text-danger-text"}`}>
            {live ? "Connected" : "Not connected"}
          </dd>
          <dd className="mt-1 break-all font-mono text-type-micro text-ink-muted">{websocketUrl}</dd>
        </div>
        <div className={factCard}>
          <dt className={factLabel}>State revision</dt>
          <dd className="mt-1 font-semibold tabular-nums text-ink">{state.revision}</dd>
        </div>
      </dl>
      </div>

      <div className="mt-3 space-y-4">
        <Field error={errTimezone} heldChange={heldTimezone} label="Timezone">
          <Select disabled={pending} onChange={(event) => setTimezone(event.target.value)} value={timezone}>
            {timezoneChoices.map((zone) => (
              <option key={zone} value={zone}>{zone}</option>
            ))}
          </Select>
        </Field>
        <Field
          heldChange={heldLan}
          hint={lanAccess ? `Other devices can open http://<this-computer's-IP>:${serverPort}.` : undefined}
          label="Network access"
        >
          <Select disabled={pending} onChange={(event) => setLanAccess(event.target.value === "network")} value={lanAccess ? "network" : "local"}>
            <option value="local">This computer only</option>
            <option value="network">Other devices on this local network</option>
          </Select>
        </Field>
        <div className="text-sm text-ink-body">
          <label className="flex touch-manipulation items-start gap-3 coarse:min-h-ds-touch-target">
            <input
              checked={pinEnabled}
              className="mt-0.5 size-4 accent-integration-backend"
              disabled={pending}
              onChange={(event) => {
                setPinEnabled(event.target.checked);
              }}
              type="checkbox"
            />
            <span>
              <span className="block font-semibold text-ink">
                Require a PIN for web dashboards
              </span>
              {heldPinEnabled && <HeldMarker />}
              <span className="mt-1 block text-xs text-ink-muted">
                Enabled by default. The initial PIN is 1234.
              </span>
            </span>
          </label>
          {pinEnabled && (
            <Field className="mt-3" error={errPin} heldChange={heldPin} hint="Saved with Save configuration." label="Replace dashboard PIN">
              <Input
                autoComplete="new-password"
                className="font-mono"
                disabled={pending}
                inputMode="numeric"
                maxLength={64}
                minLength={4}
                onChange={(event) => setDashboardPin(event.target.value)}
                placeholder="Leave blank to keep the saved PIN…"
                type="password"
                value={dashboardPin}
              />
            </Field>
          )}
        </div>
      </div>

      <SectionLabel>People</SectionLabel>
      <p className="mt-2 text-sm text-ink-body">Let people sign in from a phone or another computer.</p>
      <div className="mt-3 space-y-4">
        <label className="flex min-h-ds-touch-target touch-manipulation items-start gap-3 text-sm text-ink-body">
          <input
            checked={remote.enabled}
            className="mt-0.5 size-4 accent-integration-backend"
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

      <div className="mt-6">
        <Disclosure
          onOpenChange={setAdvancedOpen}
          open={advancedOpen}
          summary={advancedSummary}
          title="Advanced"
          tone="backend"
          variant="advanced"
        >
        <div className="grid gap-4 sm:grid-cols-3">
          <Field error={errPort} heldChange={heldPort} label="Server port">
            <Input
              disabled={pending}
              inputMode="numeric"
              onChange={(event) => setServerPort(event.target.value)}
              value={serverPort}
            />
          </Field>
          <Field heldChange={heldLogLevel} label="Log level">
            <Select
              disabled={pending}
              onChange={(event) => {
                const value = event.target.value as GeneralSettingsInput["log_level"];
                setLogLevel(value);
              }}
              value={logLevel}
            >
              {(["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"] as const).map((level) => (
                <option key={level} value={level}>{level}</option>
              ))}
            </Select>
          </Field>
          <Field heldChange={heldChannel} label="Release channel">
            <Select
              disabled={pending}
              onChange={(event) => {
                const value = event.target.value as GeneralSettingsInput["release_channel"];
                setReleaseChannel(value);
              }}
              value={releaseChannel}
            >
              {(["STABLE", "BETA"] as const).map((channel) => (
                <option key={channel} value={channel}>{channel}</option>
              ))}
            </Select>
          </Field>
        </div>
        </Disclosure>
      </div>

      <p className="mt-4 text-xs text-ink-muted">
        Timezone, logging, port, and network-access changes take effect after a backend restart. Saving a PIN change signs out every open dashboard.
      </p>
      {blockedReason && <p className="mt-3 text-sm text-ink-body" id="backend-save-blocked">{blockedReason}</p>}
      <SaveBar className="mt-3" onDiscard={discard} onRetry={save} onSave={save} state={saveState} />

      <PanelAlert className="mt-3" error={error} message={message} messageTone="info" onRetry={!settings ? onRetryLoad : undefined} />

      <p className="mt-4 text-xs text-ink-muted">
        The dashboard remembers the saved server port for its next launch. Environment variables may still override these values for development.
      </p>


      <SendAllLogsButton />
    </Panel>
  );
}
