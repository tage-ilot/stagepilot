import { useEffect, useMemo, useState } from "react";

import type {
  ProPresenterSettingsInput,
  ProPresenterStatusResponse,
} from "../types";
import { SetupPanelHeader } from "./SetupPanelHeader";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";
import { InlineAlert } from "./ui/InlineAlert";
import { Input } from "./ui/Input";
import { Panel } from "./ui/Panel";
import { Select } from "./ui/Select";
import { connectionWord } from "./ui/statusWord";

export function ProPresenterSetupPanel({
  propresenter,
  error,
  message,
  pendingOperation,
  onSave,
  onTest,
  onRefreshTimers,
  onClose,
}: {
  propresenter: ProPresenterStatusResponse | null;
  error: string | null;
  message: string | null;
  pendingOperation: "save" | "test" | "refresh" | null;
  onSave: (settings: ProPresenterSettingsInput) => void;
  onTest: () => void;
  onRefreshTimers: () => void;
  onClose?: () => void;
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
  const timerDetected = Boolean(
    propresenter?.timers.some((timer) => timer.name === timerName),
  );
  const lookDetected = Boolean(propresenter?.looks.some((look) => look.id === lookId));

  return (
    <Panel className="mt-5" id="propresenter-configuration" variant="setup">
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

      {propresenter && disabled && (
        <InlineAlert className="mt-4" tone="warning">
          Save these settings to enable Presentation, then restart StagePilot.
        </InlineAlert>
      )}

      <h3 className="mt-5 font-semibold text-ink">Basic</h3>
      <div className="mt-3 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <Field label="Host">
          <Input
            disabled={busy}
            onChange={(event) => setHost(event.target.value)}
            value={host}
          />
        </Field>
        <Field label="Port">
          <Input
            disabled={busy}
            inputMode="numeric"
            onChange={(event) => setPort(event.target.value)}
            value={port}
          />
        </Field>
        <Field label="Detected timer">
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
        <Field label="Timeout (seconds)">
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
          disabled={busy || parsedSettings === null}
          onClick={() => parsedSettings && onSave(parsedSettings)}
          variant="primary"
        >
          {pendingOperation === "save" ? "Saving…" : "Save settings"}
        </Button>
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

      {(error || message) && (
        <InlineAlert className="mt-4" tone={error ? "danger" : "success"}>
          {error ?? message}
        </InlineAlert>
      )}

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
