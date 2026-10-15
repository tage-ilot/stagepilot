import { useEffect, useMemo, useRef, useState } from "react";

import { apiOrigin, websocketUrl } from "../api";
import { copyDiagnosticsLog, sendDiagnosticsBundle } from "../diagnostics";
import { usePersistedFlag } from "../hooks/usePersistedFlag";
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
import { ConnectionMethod, type ConnectionMethodOption } from "./ui/ConnectionMethod";
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

type AccessChoice = "host" | "network" | "internet";

export const REMOTE_ACCESS_OPEN_KEY = "stagepilot.backend.remote-access-open";
const remoteStateWord = { off: "Off", enabling: "Enabling…", connected: "Connected", reconnecting: "Reconnecting…", error: "Connection unavailable" } as const;

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
  const [dashboardPin, setDashboardPin] = useState("");
  const [releaseChannel, setReleaseChannel] = useState<GeneralSettingsInput["release_channel"]>("BETA");
  const remote = useRemoteAccess();
  // The Remote access section starts open and then stays however the user leaves it, across restarts (owner, 2026-10-14).
  const [remoteOpenSaved, setRemoteOpenSaved] = usePersistedFlag(REMOTE_ACCESS_OPEN_KEY, true);
  const [advancedOpen, setAdvancedOpen] = useState(false);

  // Each draft follows its own saved value.
  const savedSettings = settings?.settings ?? null;
  const syncedTimezone = savedSettings?.timezone ?? null;
  const syncedLogLevel = savedSettings?.log_level ?? null;
  const syncedPort = savedSettings ? String(savedSettings.server_port) : null;
  const syncedChannel = savedSettings ? (savedSettings.release_channel ?? "BETA") : null;
  useEffect(() => { if (syncedTimezone !== null) setTimezone(syncedTimezone); }, [syncedTimezone]);
  useEffect(() => { if (syncedLogLevel !== null) setLogLevel(syncedLogLevel); }, [syncedLogLevel]);
  useEffect(() => { if (syncedPort !== null) setServerPort(syncedPort); }, [syncedPort]);
  useEffect(() => { if (syncedChannel !== null) setReleaseChannel(syncedChannel); }, [syncedChannel]);

  const parsedSettings = useMemo<GeneralSettingsInput | null>(() => {
    const parsedPort = Number(serverPort);
    if (!timezone.trim()) return null;
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) return null;
    return {
      timezone: timezone.trim(),
      log_level: logLevel,
      server_port: parsedPort,
      // Network access is saved the moment it is chosen (Connection Access below), never held for this button.
      lan_access: savedSettings?.lan_access ?? false,
      // The dashboard PIN is always required (owner, 2026-10-14); only its value can be replaced.
      web_dashboard_pin_enabled: true,
      release_channel: releaseChannel,
    };
  }, [logLevel, releaseChannel, savedSettings, serverPort, timezone]);
  // Every field here is held until Save configuration. PUT /settings rewrites the whole settings
  // file, may reconfigure the MIDI port, and reports restart_required; the PIN endpoint signs out
  // every dashboard session (backend/src/stagepilot/api/dashboard_auth.py).
  const saved = settings?.settings ?? null;
  const heldTimezone = saved !== null && timezone !== saved.timezone;
  const heldPort = saved !== null && serverPort !== String(saved.server_port);
  const heldLogLevel = saved !== null && logLevel !== saved.log_level;
  const heldChannel = saved !== null && releaseChannel !== (saved.release_channel ?? "BETA");
  const changeCount = [heldTimezone, heldPort, heldLogLevel, heldChannel].filter(Boolean).length;
  useEffect(() => { onDirtyChange?.(changeCount > 0); return () => onDirtyChange?.(false); }, [changeCount, onDirtyChange]);
  const [saveFailed, setSaveFailed] = useState(false);
  const attempt = useSaveAttempt("backend-configuration");
  const portValid = Number.isInteger(Number(serverPort)) && Number(serverPort) >= 1 && Number(serverPort) <= 65535;
  const errTimezone = attempt.attempted && !timezone.trim() ? "Enter a timezone." : undefined;
  const errPort = attempt.attempted && !portValid ? "Enter a port from 1 to 65535." : undefined;
  const pinTooShort = dashboardPin !== "" && dashboardPin.length < 4;
  // A problem or a waiting change inside Advanced must never hide behind the closed row.
  const advancedHeld = [heldTimezone, heldPort, heldLogLevel, heldChannel].filter(Boolean).length;
  useEffect(() => { if (errPort || errTimezone) setAdvancedOpen(true); }, [errPort, errTimezone]);
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
    ? "Enter a timezone and a port from 1 to 65535 before saving."
    : null;
  const discard = () => {
    if (!saved) return;
    setTimezone(saved.timezone);
    setServerPort(String(saved.server_port));
    setLogLevel(saved.log_level);
    setReleaseChannel(saved.release_channel ?? "BETA");
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
  // ---- Connection Access (owner, 2026-10-14): Host / Local network / Internet in one switch that saves the moment it is chosen.
  // Host = this computer only. Local network = other devices on the network. Internet = Remote Access on, and the local
  // network stays available too. The choice is read from what is really saved/running, so it can never show something untrue.
  const savedLan = saved?.lan_access ?? false;
  // The backend reports the address it is listening on right now, which only follows a saved change after it restarts (the
  // desktop app does that straight away). Until then the option just chosen stays shown, with a note, instead of snapping back.
  const [chosenLan, setChosenLan] = useState<boolean | null>(null);
  useEffect(() => { if (chosenLan !== null && chosenLan === savedLan) setChosenLan(null); }, [chosenLan, savedLan]);
  useEffect(() => { if (saveFailed) setChosenLan(null); }, [saveFailed]);
  const shownLan = chosenLan ?? savedLan;
  const needsRestart = chosenLan !== null && chosenLan !== savedLan;
  const access: AccessChoice = remote.enabled ? "internet" : shownLan ? "network" : "host";
  const accessPayload = (overrides: { lan_access?: boolean; web_dashboard_pin?: string } = {}): GeneralSettingsInput | null => saved === null ? null : ({
    // Only the access fields change here: anything else the page is holding (timezone, port...) stays held, not saved by accident.
    timezone: saved.timezone,
    log_level: saved.log_level,
    server_port: saved.server_port,
    lan_access: overrides.lan_access ?? savedLan,
    web_dashboard_pin_enabled: true,
    release_channel: saved.release_channel ?? "BETA",
    ...(overrides.web_dashboard_pin ? { web_dashboard_pin: overrides.web_dashboard_pin } : {}),
  });
  const saveLan = (lan: boolean) => {
    const payload = accessPayload({ lan_access: lan });
    if (payload && !pending && lan !== shownLan) { setSaveFailed(false); setChosenLan(lan); onSave(payload); }
  };
  // Internet needs the local network too, and leaving Internet may need a different network setting. Both wait for Remote
  // Access to finish switching (or for the disable confirmation), so a half-finished change is never saved.
  const afterRemote = useRef<{ when: "enabled" | "disabled"; lan: boolean } | null>(null);
  useEffect(() => {
    const pendingStep = afterRemote.current;
    if (!pendingStep || remote.checkboxBusy || remote.confirmDisable) return;
    if (pendingStep.when === "enabled") {
      if (remote.enabled) { afterRemote.current = null; saveLan(pendingStep.lan); }
      else if (remote.error) afterRemote.current = null; // turning Internet on failed: change nothing else
    } else if (!remote.enabled) {
      afterRemote.current = null;
      saveLan(pendingStep.lan);
    } else {
      afterRemote.current = null; // the disable confirmation was cancelled
    }
  });
  const chooseAccess = (next: AccessChoice) => {
    if (next === access || pending || remote.checkboxBusy) return;
    if (next === "internet") { afterRemote.current = { when: "enabled", lan: true }; remote.requestEnable(); return; }
    if (access === "internet") { afterRemote.current = { when: "disabled", lan: next === "network" }; remote.requestDisable(); return; }
    saveLan(next === "network");
  };
  const showAccessPin = access !== "host";
  const savePin = () => {
    const payload = accessPayload({ web_dashboard_pin: dashboardPin });
    if (payload && !pending && dashboardPin.length >= 4) { setSaveFailed(false); onSave(payload); }
  };
  const accessOptions: ConnectionMethodOption<AccessChoice>[] = [
    { value: "host", label: "Host" },
    { value: "network", label: "Local network" },
    { value: "internet", label: "Internet" },
  ];

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

      {/* The removed Status heading used to add 52.5px under the header (24px margin + 16.5px line + 12px). The owner wanted about
          half of that back (2026-10-14), so 26px. */}
      <div className="backend-status-wrap mt-[26px]">
      <dl aria-label="Status" className="backend-status grid gap-3 text-sm">
        <div className={factCard}>
          <dt className={factLabel}>Version</dt>
          <dd className="mt-1 font-semibold text-ink">{health ? health.version : "Loading"}</dd>
        </div>
        <div className={factCard}>
          <dt className={factLabel}>State revision</dt>
          <dd className="mt-1 font-semibold tabular-nums text-ink">{state.revision}</dd>
        </div>
        <div className={factCard}>
          <dt className={factLabel}>API endpoint</dt>
          <dd className="mt-1 break-all font-mono text-xs text-ink">{apiOrigin}</dd>
        </div>
        <div className={factCard}>
          <dt className={factLabel}>WebSocket</dt>
          <dd className={`mt-1 font-semibold ${live ? "text-integration-backend-text" : "text-danger-text"}`}>
            {live ? "Connected" : "Disconnected"}
          </dd>
          <dd className="mt-1 break-all font-mono text-type-micro text-ink-muted">{websocketUrl}</dd>
        </div>
      </dl>
      </div>

      <div className="mt-3">
        <Disclosure
          onOpenChange={setAdvancedOpen}
          open={advancedOpen}
          summary={advancedSummary}
          title="Advanced"
          tone="backend"
          variant="advanced"
        >
        {/* Four settings, laid out 1, 2 or 4 across by the width of the section, exactly like the status cards above (never 3). */}
        <div className="backend-advanced-wrap">
        <div className="backend-advanced grid gap-4">
          <Field error={errTimezone} heldChange={heldTimezone} label="Timezone">
            <Select disabled={pending} onChange={(event) => setTimezone(event.target.value)} value={timezone}>
              {timezoneChoices.map((zone) => (
                <option key={zone} value={zone}>{zone}</option>
              ))}
            </Select>
          </Field>
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
        </div>
        </Disclosure>
      </div>

      <div className="mt-4" data-connection-access>
        <ConnectionMethod
          disabled={pending || remote.checkboxBusy || saved === null}
          label="Connection Access"
          onChange={chooseAccess}
          options={accessOptions}
          value={access}
        />
        <p className="mt-2 max-w-3xl text-xs text-ink-muted" data-access-hint>
          {access === "host"
            ? "Only this computer can open StagePilot."
            : `Other devices on this network can open http://<this-computer's-IP>:${serverPort}.`}
          {access === "internet" ? " Anyone you add below can also sign in from anywhere." : ""}
        </p>
        {remote.checkboxBusy && <p className="mt-2 inline-flex items-center gap-2 text-xs text-ink-muted"><ButtonSpinner />Applying…</p>}
        {needsRestart && !pending && !saveFailed && (
          <p className="mt-2 text-xs text-warning-text" data-access-restart role="status">
            Saved. Restart StagePilot to {chosenLan ? "let other devices connect" : "make it available to this computer only"}.
          </p>
        )}
        {showAccessPin && (
          <div className="mt-3" data-access-pin>
            {/* The label sits above; the small input and its Save PIN button share one row directly below it. */}
            <Field error={pinTooShort ? "Enter a PIN of at least 4 characters." : undefined} label="Access PIN">
              <div className="flex flex-wrap items-center gap-2">
                <Input
                  autoComplete="new-password"
                  className="!w-28 font-mono"
                  disabled={pending}
                  inputMode="numeric"
                  maxLength={64}
                  minLength={4}
                  onChange={(event) => setDashboardPin(event.target.value)}
                  onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); savePin(); } }}
                  type="password"
                  value={dashboardPin}
                />
                <Button disabled={pending || dashboardPin.length < 4} onClick={savePin} type="button" variant="secondary">Save PIN</Button>
              </div>
            </Field>
            <p className="mt-2 max-w-3xl text-xs text-ink-muted">The initial PIN is 1234. Replacing it signs out every open dashboard.</p>
          </div>
        )}
        {access === "internet" && remote.panelOpen && (
          <div className="mt-4" data-access-people>
            {/* A pending "turn Remote Access off?" question must never hide inside a folded section, so it opens the section for as
                long as it is asked, without changing the saved choice. */}
            <Disclosure
              onOpenChange={setRemoteOpenSaved}
              open={remoteOpenSaved || remote.confirmDisable}
              summary={remote.bootstrap ? "Create the first Operator" : remote.status ? remoteStateWord[remote.status.state] : undefined}
              title="Remote access"
              variant="activity"
            >
              <RemoteAccessPanel control={remote} />
            </Disclosure>
          </div>
        )}
        {access !== "internet" && remote.panelOpen && <div className="mt-4"><RemoteAccessPanel control={remote} /></div>}
      </div>

      <p className="mt-4 text-xs text-ink-muted">
        Timezone, logging and port changes take effect after a backend restart.
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
