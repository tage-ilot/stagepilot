import { useEffect, useMemo, useRef, useState } from "react";

import type {
  ProPresenterSettingsInput,
  ProPresenterStatusResponse,
} from "../types";
import { SetupPanelHeader } from "./SetupPanelHeader";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { InlineAlert } from "./ui/InlineAlert";
import { PanelAlert } from "./PanelAlert";
import { Input } from "./ui/Input";
import { Panel } from "./ui/Panel";
import { SaveBar, type SaveBarState } from "./ui/SaveBar";
import { useSaveAttempt } from "../hooks/useSaveAttempt";
import { Select } from "./ui/Select";
import { connectionWord } from "./ui/statusWord";

export function ProPresenterSetupPanel({
  propresenter,
  error,
  onRetryLoad,
  message,
  pendingOperation,
  onSave,
  onTest,
  onRefreshTimers,
  onClose,
  onDirtyChange,
}: {
  propresenter: ProPresenterStatusResponse | null;
  error: string | null;
  onRetryLoad?: () => void;
  message: string | null;
  pendingOperation: "save" | "test" | "refresh" | null;
  onSave: (settings: ProPresenterSettingsInput) => void;
  onTest: () => void;
  onRefreshTimers: () => void;
  onClose?: () => void;
  onDirtyChange?: (dirty: boolean) => void;
}) {
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState("1025");
  const [timerName, setTimerName] = useState("Song Countdown");
  const [lookId, setLookId] = useState("");
  const [timeout, setTimeout] = useState("3");
  const configuredHost = propresenter?.host;
  const configuredPort = propresenter?.port;
  const configuredTimerName = propresenter?.timer_name;
  const configuredLookId = propresenter?.look_id;
  const configuredTimeout = propresenter?.request_timeout_seconds;

  useEffect(() => {
    if (
      configuredHost === undefined
      || configuredPort === undefined
      || configuredTimerName === undefined
      || configuredLookId === undefined
      || configuredTimeout === undefined
    ) return;
    setHost(configuredHost);
    setPort(String(configuredPort));
    setTimerName(configuredTimerName);
    setLookId(configuredLookId ?? "");
    setTimeout(String(configuredTimeout));
  }, [configuredHost, configuredLookId, configuredPort, configuredTimeout, configuredTimerName]);

  const parsedSettings = useMemo<ProPresenterSettingsInput | null>(() => {
    const parsedPort = Number(port);
    const parsedTimeout = Number(timeout);
    if (!host.trim() || !timerName.trim()) return null;
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) return null;
    if (!Number.isFinite(parsedTimeout) || parsedTimeout <= 0 || parsedTimeout > 60) return null;
    return {
      host: host.trim(),
      port: parsedPort,
      timer_name: timerName.trim(),
      look_id: lookId || null,
      request_timeout_seconds: parsedTimeout,
    };
  }, [host, lookId, port, timeout, timerName]);

  const disabled = !propresenter?.enabled;
  const busy = pendingOperation !== null;

  // Held changes: fields that differ from the saved values. Nothing here saves on change.
  const held = {
    host: configuredHost !== undefined && host.trim() !== configuredHost,
    port: configuredPort !== undefined && port.trim() !== String(configuredPort),
    timer: configuredTimerName !== undefined && timerName.trim() !== configuredTimerName,
    look: configuredLookId !== undefined && lookId !== (configuredLookId ?? ""),
    timeout: configuredTimeout !== undefined && timeout.trim() !== String(configuredTimeout),
  };
  const changeCount = Object.values(held).filter(Boolean).length;
  useEffect(() => { onDirtyChange?.(changeCount > 0); return () => onDirtyChange?.(false); }, [changeCount, onDirtyChange]);
  const [saveFailed, setSaveFailed] = useState(false);
  const attempt = useSaveAttempt("propresenter-configuration");
  const portNumber = Number(port);
  const timeoutNumber = Number(timeout);
  const errHost = attempt.attempted && !host.trim() ? "Enter a host." : undefined;
  const errPort = attempt.attempted && !(Number.isInteger(portNumber) && portNumber >= 1 && portNumber <= 65535) ? "Enter a port from 1 to 65535." : undefined;
  const errTimer = attempt.attempted && !timerName.trim() ? "Choose a timer." : undefined;
  const errTimeout = attempt.attempted && !(Number.isFinite(timeoutNumber) && timeoutNumber > 0 && timeoutNumber <= 60) ? "Enter a timeout above 0 and up to 60 seconds." : undefined;
  const wasSaving = useRef(false);
  useEffect(() => {
    const saving = pendingOperation === "save";
    if (wasSaving.current && !saving) setSaveFailed(Boolean(error));
    wasSaving.current = saving;
  }, [pendingOperation, error]);
  const saveState: SaveBarState = pendingOperation === "save"
    ? { kind: "saving" }
    : saveFailed && changeCount > 0
      ? { kind: "error", message: "Couldn't save" }
      : changeCount > 0
        ? { kind: "held", count: changeCount }
        : { kind: "clean" };
  const blockedReason = changeCount > 0 && parsedSettings === null
    ? "Enter a host, a port from 1 to 65535, a timer and a timeout above 0 and up to 60 seconds before saving."
    : null;

  const discard = () => {
    if (!propresenter) return;
    setHost(propresenter.host);
    setPort(String(propresenter.port));
    setTimerName(propresenter.timer_name);
    setLookId(propresenter.look_id ?? "");
    setTimeout(String(propresenter.request_timeout_seconds));
    setSaveFailed(false);
    attempt.reset();
  };

  const save = () => {
    // Before Presentation is enabled, Save works with no edits (as it did before); afterwards a clean Save is a no-op.
    if (busy || (changeCount === 0 && propresenter?.enabled)) return;
    if (!parsedSettings) { attempt.refuse(); return; }
    setSaveFailed(false);
    onSave(parsedSettings);
  };
  const timerDetected = Boolean(
    propresenter?.timers.some((timer) => timer.name === timerName),
  );
  const lookDetected = Boolean(propresenter?.looks.some((look) => look.id === lookId));

  return (
    <Panel className="mt-5" id="propresenter-configuration" tone="presentation" variant="setup">
      <SetupPanelHeader
        closeLabel="Close Presentation configuration"
        description="Send the countdown timer to Presentation, with its configuration kept between StagePilot launches."
        onClose={onClose}
        status={propresenter?.connection_status ?? "loading"}
        title="Presentation"
      />

      {propresenter?.detail && (
        <p className="mt-3 text-xs text-ink-muted">{propresenter.detail}</p>
      )}

      {!propresenter && !error && (
        <InlineAlert className="mt-4" tone="info">
          Loading Presentation configuration…
        </InlineAlert>
      )}

      {propresenter && disabled && !error && (
        <InlineAlert className="mt-4" tone="warning">
          Save these settings to enable Presentation, then restart StagePilot.
        </InlineAlert>
      )}

      <h3 className="mt-5 font-semibold text-ink">Basic</h3>
      <div className="mt-3 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <Field error={errHost} heldChange={held.host} label="Host">
          <Input
            disabled={busy}
            onChange={(event) => setHost(event.target.value)}
            value={host}
          />
        </Field>
        <Field error={errPort} heldChange={held.port} label="Port">
          <Input
            disabled={busy}
            inputMode="numeric"
            onChange={(event) => setPort(event.target.value)}
            value={port}
          />
        </Field>
        <Field error={errTimer} heldChange={held.timer} label="Detected timer">
          <Select
            disabled={busy}
            onChange={(event) => setTimerName(event.target.value)}
            value={timerName}
          >
            <option value="">Choose a detected timer</option>
            {timerName && !timerDetected && (
              <option value={timerName}>{timerName} (currently unavailable)</option>
            )}
            {propresenter?.timers.map((timer) => (
              <option key={timer.id} value={timer.name}>
                {timer.name}{timer.is_countdown ? "" : " (converted when cued)"}
              </option>
            ))}
          </Select>
        </Field>
        <Field error={errTimeout} heldChange={held.timeout} label="Timeout (seconds)">
          <Input
            disabled={busy}
            inputMode="decimal"
            onChange={(event) => setTimeout(event.target.value)}
            value={timeout}
          />
        </Field>
      </div>

      <div className="mt-4 max-w-md">
        <Field
          heldChange={held.look}
          hint="Selecting a Look does not change Presentation until you save these settings."
          label="ProPresenter Look"
        >
          <Select
            aria-label="ProPresenter Look"
            disabled={busy}
            onChange={(event) => setLookId(event.target.value)}
            value={lookId}
          >
            <option value="">Do not change the current Look</option>
            {lookId && !lookDetected && (
              <option value={lookId}>Saved Look (currently unavailable)</option>
            )}
            {propresenter?.looks.map((look) => (
              <option key={look.id} value={look.id}>
                {look.name}{look.id === propresenter.current_look_id ? " (current)" : ""}
              </option>
            ))}
          </Select>
        </Field>
      </div>

      <div className="mt-5 flex flex-wrap gap-2">
        <Button
          disabled={disabled || busy}
          onClick={onTest}
          tone="presentation"
          variant="tinted"
        >
          {pendingOperation === "test" ? "Testing…" : "Test connection"}
        </Button>
        <Button
          disabled={disabled || busy}
          onClick={onRefreshTimers}
          tone="presentation"
          variant="tinted"
        >
          {pendingOperation === "refresh" ? "Refreshing…" : "Refresh timers and Looks"}
        </Button>
      </div>

      {changeCount > 0 && <p className="mt-3 text-sm text-ink-muted" id="propresenter-test-saved-note">Test connection and Refresh use the saved settings. Save configuration first to try the new ones.</p>}

      <PanelAlert className="mt-4" error={error} message={message} messageTone="success" onRetry={!propresenter ? onRetryLoad : undefined} />

      {blockedReason && <p className="mt-4 text-sm text-ink-body" id="propresenter-save-blocked">{blockedReason}</p>}
      <SaveBar className="mt-5" onDiscard={discard} onRetry={save} onSave={save} state={saveState} />

      <dl className="mt-5 grid gap-x-4 gap-y-3 border-t border-edge-subtle pt-4 text-sm sm:grid-cols-3">
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">API</dt>
          <dd className="mt-1 text-ink-body">{propresenter ? connectionWord[propresenter.connection_status] : "Unknown"}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">Configured timer</dt>
          <dd className={`mt-1 ${propresenter?.timer_found ? "text-success-text" : "text-warning-text"}`}>
            {propresenter?.timer_found ? propresenter.timer_name : "Not found"}
          </dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wider text-ink-muted">Last checked</dt>
          <dd className="mt-1 text-ink-body tabular-nums">
            {propresenter?.last_checked_at ? new Date(propresenter.last_checked_at).toLocaleTimeString() : "Not checked"}
          </dd>
        </div>
      </dl>
    </Panel>
  );
}
