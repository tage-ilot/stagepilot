import { useEffect, useMemo, useRef, useState } from "react";
import { timeWords } from "../lib/dateWords";
import { rememberAddress } from "../hooks/useRecentAddresses";

import type {
  ProPresenterSettingsInput,
  ProPresenterStatusResponse,
  ServicePlan,
} from "../types";
import { SetupPanelHeader } from "./SetupPanelHeader";
import { Field } from "./ui/Field";
import { PanelAlert } from "./PanelAlert";
import { ActionButton, ActionRow, PanelLayout, SectionNotice, SettingsSection, StatusSummary } from "./sections";
import { SaveBar, type SaveBarState } from "./ui/SaveBar";
import { useSaveAttempt } from "../hooks/useSaveAttempt";
import { Select } from "./ui/Select";
import { connectionWord } from "./ui/statusWord";
import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import { useProPresenterScan, type ProPresenterScanController } from "../hooks/useProPresenterScan";
import { ProPresenterLinkBlock } from "./ProPresenterLinkBlock";
import { PresenterLinkSection } from "./PresenterLinkSection";
import { VerseVisionButton } from "./VerseVisionButton";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { PresenterFollowController } from "../hooks/usePresenterFollow";

const DEFAULT_TIMEOUT_SECONDS = 3;
const CHOOSE_TIMER = "Choose a timer";
const CURRENT = " (current)";
const UNAVAILABLE = " (unavailable)";
const CONVERTED = " (converted when cued)";
const LOOK_HINT = "Selecting a Look does not change Presentation until you save these settings.";
const LOOK_NEEDS_CONNECTION = "Connect to ProPresenter first";

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
  link,
  follow,
  library,
  plan,
  playing = false,
  scanner,
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
  /** The optional Presenter Link (its line, pill and setup in the Live Link block). Left out, the block holds the connection and the ProPresenter API line only. */
  link?: ProPresenterLinkController;
  /** Following Playback with the lyrics in ProPresenter (its switch, status and timing sit in the Presenter Link area). */
  follow?: PresenterFollowController;
  /** The Playback library (the Library Link): when it has given the songs of the upcoming plan, the Presenter Link section lists those instead of the plan's Planning Center names. */
  library?: PlaybackLibraryController;
  /** The loaded service plan: its song names are the Presenter Link list until Playback has given the songs. */
  plan?: ServicePlan | null;
  /** A song is playing (Playback says so, or a StagePilot countdown runs): the Presenter Link adds nothing then. */
  playing?: boolean;
  /** The network scan for ProPresenter's API. Left out, the panel runs its own (it asks nothing until Scan is pressed). */
  scanner?: ProPresenterScanController;
}) {
  const ownScanner = useProPresenterScan();
  const scanning = scanner ?? ownScanner;
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState("1025");
  const [timerName, setTimerName] = useState("Song Countdown");
  const [lookId, setLookId] = useState("");
  const configuredHost = propresenter?.host;
  const configuredPort = propresenter?.port;
  const configuredTimerName = propresenter?.timer_name;
  const configuredLookId = propresenter?.look_id;
  // The request timeout is a saved setting this page never shows or edits: it is loaded with the rest and saved back unchanged
  // (the backend still validates it; docs/propresenter.md says where it is changed).
  const savedTimeout = propresenter?.request_timeout_seconds ?? DEFAULT_TIMEOUT_SECONDS;

  useEffect(() => {
    if (
      configuredHost === undefined
      || configuredPort === undefined
      || configuredTimerName === undefined
      || configuredLookId === undefined
    ) return;
    setHost(configuredHost);
    setPort(String(configuredPort));
    setTimerName(configuredTimerName);
    setLookId(configuredLookId ?? "");
  }, [configuredHost, configuredLookId, configuredPort, configuredTimerName]);

  const parsedSettings = useMemo<ProPresenterSettingsInput | null>(() => {
    const parsedPort = Number(port);
    if (!host.trim() || !timerName.trim()) return null;
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) return null;
    return {
      host: host.trim(),
      port: parsedPort,
      timer_name: timerName.trim(),
      look_id: lookId || null,
      request_timeout_seconds: savedTimeout,
    };
  }, [host, lookId, port, savedTimeout, timerName]);

  const disabled = !propresenter?.enabled;
  const busy = pendingOperation !== null;

  // Held changes: fields that differ from the saved values. Nothing here saves on change.
  const held = {
    host: configuredHost !== undefined && host.trim() !== configuredHost,
    port: configuredPort !== undefined && port.trim() !== String(configuredPort),
    timer: configuredTimerName !== undefined && timerName.trim() !== configuredTimerName,
    look: configuredLookId !== undefined && lookId !== (configuredLookId ?? ""),
  };
  const changeCount = Object.values(held).filter(Boolean).length;
  useEffect(() => { onDirtyChange?.(changeCount > 0); return () => onDirtyChange?.(false); }, [changeCount, onDirtyChange]);
  const [saveFailed, setSaveFailed] = useState(false);
  const attempt = useSaveAttempt("propresenter-configuration");
  const portNumber = Number(port);
  const errHost = attempt.attempted && !host.trim() ? "Enter a host." : undefined;
  const errPort = attempt.attempted && !(Number.isInteger(portNumber) && portNumber >= 1 && portNumber <= 65535) ? "Enter the API port from 1 to 65535." : undefined;
  const errTimer = attempt.attempted && !timerName.trim() ? "Choose a timer." : undefined;
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
    ? "Enter a host, an API port from 1 to 65535 and a timer before saving."
    : null;

  const discard = () => {
    if (!propresenter) return;
    setHost(propresenter.host);
    setPort(String(propresenter.port));
    setTimerName(propresenter.timer_name);
    setLookId(propresenter.look_id ?? "");
    setSaveFailed(false);
    attempt.reset();
  };

  const save = () => {
    // Before Presentation is enabled, Save works with no edits (as it did before); afterwards a clean Save is a no-op.
    if (busy || (changeCount === 0 && propresenter?.enabled)) return;
    if (!parsedSettings) { attempt.refuse(); return; }
    setSaveFailed(false);
    rememberAddress("presentation", parsedSettings.host);
    onSave(parsedSettings);
  };

  // Connect: the connection the boxes hold, saved and connected the way Save does it (the same request), without taking the timer or Look
  // along while they are held. A saved ProPresenter with nothing held in the two boxes is only checked again.
  const [connectPressed, setConnectPressed] = useState(false);
  useEffect(() => { if (!busy) setConnectPressed(false); }, [busy]);
  const hostOk = Boolean(host.trim());
  const portOk = Number.isInteger(portNumber) && portNumber >= 1 && portNumber <= 65535;
  const checksAgain = Boolean(propresenter?.enabled) && !held.host && !held.port;
  const connect = () => {
    if (busy || !propresenter) return;
    setConnectPressed(true);
    if (checksAgain) { onTest(); return; }
    if (!hostOk || !portOk) { attempt.refuse(); return; }
    const settings: ProPresenterSettingsInput | null = propresenter.enabled
      ? { host: host.trim(), port: portNumber, timer_name: propresenter.timer_name, look_id: propresenter.look_id ?? null, request_timeout_seconds: savedTimeout }
      : parsedSettings;
    if (!settings) { attempt.refuse(); return; }
    setSaveFailed(false);
    rememberAddress("presentation", settings.host);
    onSave(settings);
  };
  // A computer found or chosen by the scan goes in the two boxes (a held change, saved by Connect); nothing is saved by the scan itself.
  const chooseComputer = (chosenHost: string, chosenPort?: number) => {
    setHost(chosenHost);
    if (chosenPort !== undefined) setPort(String(chosenPort));
  };
  const scanState = scanning.scan;
  const filled = useRef(false);
  useEffect(() => {
    if (scanState.state === "scanning") filled.current = false;
    const [only] = scanState.candidates;
    if (scanState.state !== "found" || scanState.candidates.length !== 1 || !only || filled.current) return;
    filled.current = true;
    chooseComputer(only.host, only.port);
  }, [scanState]);
  // What the Timer and Look boxes list. No option stands for "nothing": each box shows the real choices and marks the current one.
  const timers = propresenter?.timers ?? [];
  const looks = propresenter?.looks ?? [];
  const savedTimer = configuredTimerName ?? "";
  const currentLook = looks.find((look) => look.id === propresenter?.current_look_id);
  // The saved value is empty (follow ProPresenter's Look) or the Look ProPresenter shows now: both are the "(current)" option.
  const lookValue = lookId === "" || lookId === currentLook?.id ? "" : lookId;
  const lookKnown = looks.some((look) => look.id === lookId);

  return (
    <PanelLayout
      alerts={(
        <>
          {propresenter?.detail && <p className="text-xs text-ink-muted">{propresenter.detail}</p>}
          {!propresenter && !error && <SectionNotice tone="info">Loading Presentation configuration…</SectionNotice>}
          {pendingOperation === "save" && <SectionNotice tone="info">Connecting to Presentation…</SectionNotice>}
          <PanelAlert error={error} message={message} messageTone="success" onRetry={!propresenter ? onRetryLoad : undefined} />
        </>
      )}
      header={(
        <SetupPanelHeader
          closeLabel="Close Presentation configuration"
          description="Sends the countdown timer to Presentation."
          onClose={onClose}
          status={propresenter?.connection_status ?? "loading"}
          title="Presentation"
        />
      )}
      id="propresenter-configuration"
      tone="presentation"
      // The Live Link block is the first block under the header (and its alert lines) and holds the connection (Host, API, Scan,
      // Connect). Everything that is not the connection comes after it, in the order every page follows.
      connection={(
        <ProPresenterLinkBlock
          connection={{
            host,
            port,
            onHost: setHost,
            onPort: setPort,
            held: { host: held.host, port: held.port },
            errors: { host: errHost, port: errPort },
            busy,
            checksAgain,
            connectDisabled: !propresenter || busy,
            connectHint: !propresenter ? "Waiting for the saved settings to load." : busy ? "Wait for the saving to finish." : undefined,
            connecting: pendingOperation === "test" || (connectPressed && pendingOperation === "save"),
            onConnect: connect,
            scan: { state: scanState, start: scanning.start, cancel: scanning.cancel, choose: chooseComputer, error: scanning.error, starting: scanning.starting },
          }}
          link={link}
          propresenter={propresenter}
        />
      )}
      settings={(
        <>
          <SettingsSection busy={busy} columns="pair">
            <Field
              action={(
                // The button belongs to the Timer box (it reads the timers and Looks again), so it sits right under it, in this column.
                <ActionRow
                  note={changeCount > 0 ? "Refresh uses the saved settings. Save configuration first to try the new ones." : undefined}
                  noteId="propresenter-test-saved-note"
                >
                  <ActionButton aria-describedby={changeCount > 0 ? "propresenter-test-saved-note" : undefined} busy={pendingOperation === "refresh"} busyLabel="Refreshing…" disabled={disabled || busy} label="Refresh timers and Looks" onClick={onRefreshTimers} tone="presentation" />
                </ActionRow>
              )}
              error={errTimer}
              heldChange={held.timer}
              label="Timer"
            >
              <Select aria-label="Timer" onChange={(event) => setTimerName(event.target.value)} placeholder={CHOOSE_TIMER} value={timerName}>
                {savedTimer && !timers.some((timer) => timer.name === savedTimer) && <option value={savedTimer}>{savedTimer}{UNAVAILABLE}</option>}
                {timers.map((timer) => (
                  <option key={timer.id} value={timer.name}>
                    {timer.name}{timer.name === savedTimer ? CURRENT : ""}{timer.is_countdown ? "" : CONVERTED}
                  </option>
                ))}
              </Select>
            </Field>
            <Field
              heldChange={held.look}
              hint={looks.length === 0 ? LOOK_NEEDS_CONNECTION : LOOK_HINT}
              label="Look"
            >
              <Select aria-label="Look" disabled={looks.length === 0} onChange={(event) => setLookId(event.target.value)} value={lookValue}>
                {lookValue === "" && !currentLook && <option disabled hidden value="" />}
                {lookId && !lookKnown && <option value={lookId}>{lookId}{looks.length > 0 ? UNAVAILABLE : ""}</option>}
                {looks.map((look) => (
                  // The Look ProPresenter shows now stands for "leave the Look alone" (the saved value stays empty), so its value is "".
                  <option key={look.id} value={look.id === currentLook?.id ? "" : look.id}>
                    {look.name}{look.id === currentLook?.id ? CURRENT : ""}
                  </option>
                ))}
              </Select>
            </Field>
          </SettingsSection>
        </>
      )}
      content={(
        <>
          {link ? <PresenterLinkSection follow={follow} library={library} link={link} plan={plan} playing={playing} /> : undefined}
          <VerseVisionButton />
        </>
      )}
      notices={blockedReason ? <SectionNotice id="propresenter-save-blocked" tone="info" variant="line">{blockedReason}</SectionNotice> : undefined}
      saveBar={<SaveBar onDiscard={discard} onRetry={save} onSave={save} state={saveState} />}
      summary={(
        <StatusSummary
          items={[
            { label: "API", value: propresenter ? connectionWord[propresenter.connection_status] : "Unknown" },
            { label: "Configured timer", value: propresenter?.timer_found ? propresenter.timer_name : "Not found", tone: propresenter?.timer_found ? "ok" : "warn" },
            { label: "Last checked", value: propresenter?.last_checked_at ? timeWords(propresenter.last_checked_at) : "Not checked", tabular: true },
          ]}
        />
      )}
    />
  );
}
