import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import {
  getLightkeyStatus,
  releaseLightkeyLights,
  saveLightkeySettings,
  setLightkeyRecording,
} from "../api";
import { describeError } from "../errorText";
import type { LightkeyActionResponse, LightkeyStatus } from "../types";
import { captureLabelText, type CaptureLabel } from "./lightingLooks";
import { Button } from "./ui/Button";
import { ConnectionMethod } from "./ui/ConnectionMethod";
import { confirmRerecordLooks } from "./lightingCueConfirmations";
import { LoudnessPicker } from "./LoudnessPicker";
import { Input } from "./ui/Input";
import { CONTROL_34 } from "./compactControl";
import { apiHostMismatch, apiPortError, composeApiUrl, splitApiUrl } from "./lightkeyConnection";
import { oscHostError, oscPortError } from "./lightingOsc";
import { LiveLinkField, LiveLinkPill, LiveLinkShell, LiveLinkTools, OscConnectionFields } from "./LightingOscParts";
import { CONTROL_40, liveLinkPillState, lightkeyFixtureWord } from "./liveLink";

const POLL_MS = 1500;

/** The line under the Replay | Record toggle: the one thing the operator most needs to know right now. */
export function lightkeyHeadline(status: LightkeyStatus, looks?: SongLooks): string {
  if (status.mode === "auto" && status.recording_song) return `Auto: capturing “${status.recording_song}”.`;
  if (status.recording && status.recording_song) return `Recording “${status.recording_song}”.`;
  if (status.recording) return "Recording starts with the next song.";
  if (status.replaying_song) return `Replaying “${status.replaying_song}”.`;
  if (status.mode === "auto" && status.auto) return status.auto.reason;
  const count = looks?.count ?? status.captured?.looks ?? 0;
  const loops = looks?.loops ?? 0;
  if (status.replay_blocked && count + loops > 0) return status.replay_blocked;
  if (looks?.count || loops) return capturedLooksText(count, looks?.label, loops);
  if (looks?.label && looks.label !== "not_captured") return `${captureLabelText[looks.label]} on this song`;
  if (status.captured) return `${count} captured item${count === 1 ? "" : "s"} on “${status.captured.song_title}”.`;
  if (status.link === "connected") return "Switch to Record, then start a song. Its lights are captured with the countdown.";
  return looks ? (looks.label && looks.label !== "not_captured" ? `${captureLabelText[looks.label]} on this song` : capturedLooksText(0)) : "No lights captured yet.";
}

/** What the selected song holds: captured look rows, and whether any were changed by hand. */
export interface SongLooks { count: number; /** Repeating loops saved with the song. */ loops?: number; edited: boolean; /** What the song's rows say about capturing (shown in its header). */ label?: CaptureLabel }

/** "552 captured items (546 ramps/steps, 6 loops)": the rows (ramps and steps) and the loops a recording saved. */
export const capturedItemsText = (rows: number, loops: number): string =>
  `${rows + loops} captured item${rows + loops === 1 ? "" : "s"} (${rows} ramps/steps, ${loops} loop${loops === 1 ? "" : "s"})`;

export const capturedLooksText = (count: number, label?: CaptureLabel, loops = 0): string =>
  `${label ? `${captureLabelText[label]}: ` : ""}${capturedItemsText(count, loops)} on this song`;

/** One sentence: ends with a full stop (or ! ? ...) so statements joined with " · " each read as their own. */
export const endSentence = (text: string): string => {
  const trimmed = text.trim();
  return /[.!?\u2026]$/.test(trimmed) ? trimmed : `${trimmed}.`;
};

/** The state line: the statements in order, each ended properly, joined with " · ". */
export const joinStateLine = (parts: string[]): string => parts.map(endSentence).join(" \u00B7 ");

/** Same text, ignoring quote style and spacing (the backend writes straight quotes, the headline curly ones). */
export function normaliseLine(text: string): string {
  return text.replace(/[\u201C\u201D\u201E]/g, '"').replace(/[\u2018\u2019]/g, "'").replace(/\s+/g, " ").trim();
}

/** The lines under the toggle, top to bottom, never the same thing twice: what just happened (an error or the backend message), then the headline. */
export function lightkeyLines(status: LightkeyStatus, error: string | null, looks?: SongLooks): string[] {
  const candidates = [
    error ?? status.message ?? lightkeyHeadline(status, looks),
    lightkeyHeadline(status, looks),
  ];
  const lines: string[] = [];
  const seen = new Set<string>();
  for (const line of candidates) {
    const key = normaliseLine(line);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    lines.push(line);
  }
  return lines;
}

/** "Holding 5 fixtures \u00B7 2 yielded to the operator" or "" when there is nothing to say. */
function lightkeyHolding(status: LightkeyStatus): string {
  const holding = status.holding_fixtures ?? 0;
  const yielded = status.yielded_fixtures ?? 0;
  const parts: string[] = [];
  if (holding > 0) parts.push(`Holding ${holding} fixture${holding === 1 ? "" : "s"}`);
  if (yielded > 0) parts.push(`${yielded} yielded to the operator`);
  return parts.length ? endSentence(parts.join(" \u00B7 ")) : "";
}

/** The shared Lighting connection the panel owns: ONE host for the OSC output and the live-link API, and the OSC UDP port. */
export interface LiveLinkConnection {
  host: string;
  onHost: (host: string) => void;
  oscPort: string;
  onPort: (port: string) => void;
  /** The OSC host as saved (not the draft): the API address is left empty when it is this host on the default API port. */
  savedOscHost: string;
  /** A save was tried, so field errors show. */
  attempted: boolean;
  held: { host: boolean; port: boolean };
}

const smallBox = "!w-[5.25rem] text-center tabular-nums";

/**
 * sACN DMX Live Link, the approved bordered block: a header (title and the one state pill), a field row (the shared host, the OSC UDP and
 * API ports, Connect, then Mode alone at the far right), then a quiet strip (one line of state text, Release lights, Send a test cue). The Lighting computer for OSC and for the lightkey-api is always the same machine.
 * Captures the fixtures' colour and levels from the lightkey-api agent and holds them with overrides on replay.
 * `connection` is the panel's shared host/port draft; on its own (tests, other screens) the strip keeps a host of its own.
 */
export function LightkeyStrip({ lightingEnabled, looks, loudness, connection, testCue, footer }: {
  lightingEnabled: boolean;
  looks?: SongLooks;
  /** The selected song's loudness: whether it has any, and how to clear it. */
  loudness?: { present: boolean; onClear?: () => void; disabled?: boolean };
  connection?: LiveLinkConnection;
  /** The test cue's fields: opened from "Send a test cue" in the strip under the fields. */
  testCue?: ReactNode;
  /** Notes under the block's strip (last sent, what is kept while the method is held). */
  footer?: ReactNode;
}) {
  const [status, setStatus] = useState<LightkeyStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ownHost, setOwnHost] = useState<string | null>(null); // only without `connection`
  const [apiPort, setApiPort] = useState<string | null>(null); // null = untouched, show the saved one
  const [tried, setTried] = useState(false);
  const [busy, setBusy] = useState(false);
  const alive = useRef(true);

  const load = useCallback(async () => {
    try {
      const next = await getLightkeyStatus();
      if (alive.current) { setStatus(next); setError(null); }
    } catch (cause) {
      if (alive.current) setError(describeError(cause, "Lightkey status is unavailable."));
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void load();
    const timer = window.setInterval(() => void load(), POLL_MS);
    return () => { alive.current = false; window.clearInterval(timer); };
  }, [load, lightingEnabled]);

  const run = async (action: () => Promise<LightkeyActionResponse>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const response = await action();
      if (alive.current) {
        setStatus(response.lightkey);
        if (!response.accepted) setError(response.message);
      }
    } catch (cause) {
      if (alive.current) setError(describeError(cause, "That did not work."));
    } finally {
      if (alive.current) setBusy(false);
    }
  };

  if (!status) {
    // The host and OSC UDP boxes never wait for the live link to answer: they are the saved OSC output.
    const waiting = <p className="mt-2 text-type-body-sm text-ink-muted">{error ?? "Loading\u2026"}</p>;
    if (connection) {
      return (
        <OscConnectionFields attempted={connection.attempted} footer={<>{waiting}{footer}</>} held={connection.held} host={connection.host} onHost={connection.onHost} onPort={connection.onPort} port={connection.oscPort} testCue={testCue} />
      );
    }
    return <LiveLinkShell>{waiting}</LiveLinkShell>;
  }

  const saved = splitApiUrl(status.api_url);
  const savedOscHost = connection?.savedOscHost ?? saved.host;
  const host = connection ? connection.host : (ownHost ?? saved.host);
  const setHost = connection ? connection.onHost : setOwnHost;
  const shownApiPort = apiPort ?? saved.port;
  // A saved API address on another machine is kept (and used) until the person accepts the OSC host; it is never rewritten silently.
  const otherHost = apiHostMismatch(status.api_url, savedOscHost);
  const hostError = connection && !connection.attempted && !tried ? null : oscHostError(host);
  const apiError = tried || apiPort !== null ? apiPortError(shownApiPort) : null;
  const udpError = connection && connection.attempted ? oscPortError(connection.oscPort) : null;
  const connected = status.link === "connected";
  const changed = (apiPort !== null && apiPort.trim() !== saved.port) || (otherHost === null && host.trim().toLowerCase() !== saved.host.toLowerCase());
  const valid = !oscHostError(host) && !apiPortError(shownApiPort);
  const connect = () => {
    setTried(true);
    if (!valid) return Promise.resolve();
    return run(async () => {
      const apiUrl = otherHost !== null
        ? (apiPort === null ? status.api_url : `${otherHost}:${apiPort.trim()}`)
        : composeApiUrl({ host, apiPort: shownApiPort, savedOscHost });
      const response = await saveLightkeySettings({ api_url: apiUrl });
      setApiPort(null);
      if (!connection) setOwnHost(null);
      return response;
    });
  };
  const acceptOscHost = () => run(async () => {
    const response = await saveLightkeySettings({ api_url: composeApiUrl({ host: savedOscHost, apiPort: shownApiPort, savedOscHost }) });
    setApiPort(null);
    return response;
  });

  // Record -> Replay: stop recording first, then make sure replay is on. A failure leaves the toggle on what the
  // backend says (it is re-read), and the message is shown.
  const toMode = (mode: "auto" | "replay") => async (): Promise<LightkeyActionResponse> => {
    try {
      let response: LightkeyActionResponse | null = null;
      if (status.recording) {
        response = await setLightkeyRecording(false);
        if (!response.accepted) return response;
      }
      if (!status.replay || response === null || (status.mode ?? "replay") !== mode) {
        response = await saveLightkeySettings(mode === "replay" && status.mode === undefined ? { replay: true } : { mode, replay: true });
      }
      return response;
    } catch (cause) {
      await load();
      throw cause;
    }
  };

  const pillState = liveLinkPillState(status);
  // ONE line of state text: what just happened / the headline, why the link is down (once; the pill only carries it as a tooltip), what the
  // fixture stream is doing when it is not live, then what the replay holds.
  const fixtureNote = connected && status.fixtures && status.fixtures.link !== "live" ? lightkeyFixtureWord(status.fixtures) : null;
  const downNote = status.link !== "connected" && status.link !== "off" ? status.detail : null;
  const segments: string[] = [];
  const seen = new Set<string>();
  for (const part of [...lightkeyLines(status, error, looks), downNote, fixtureNote]) {
    const key = part ? normaliseLine(part) : "";
    if (!key || seen.has(key)) continue;
    seen.add(key);
    segments.push(part as string);
  }
  const holding = lightkeyHolding(status);
  const shownMode = status.recording || status.mode === "record" ? "record" : (status.mode ?? "replay");
  const enterToConnect = (event: { key: string }) => { if (event.key === "Enter") void connect(); };
  const releasable = Math.max(status.holding_fixtures ?? 0, status.known_fixtures ?? 0) > 0;
  const stateText = (
    <p aria-live="polite" className="min-w-0 flex-[1_1_16rem] text-[0.78125rem] text-ink-muted" data-testid="lightkey-headline">
      {segments.length > 0 && joinStateLine(segments)}
      {holding && (
        <>
          {segments.length > 0 && " \u00B7 "}
          <span data-testid="lightkey-holding">{holding}</span>
        </>
      )}
    </p>
  );
  return (
    <LiveLinkShell pill={<LiveLinkPill detail={pillState.detail} text={pillState.text} tone={pillState.tone} />} testId="lightkey-strip">
      <div className="mt-2 flex flex-wrap items-end gap-x-2.5 gap-y-2" data-testid="live-link-line">
        <LiveLinkField caption="Host or IP (OSC output + Live Link)" className="live-link-host">
          <Input
            aria-label="Host or IP address"
            autoCapitalize="none"
            autoCorrect="off"
            className={`w-full font-mono ${CONTROL_40} ${connection?.held.host ? "!border-warning" : ""}`}
            error={Boolean(hostError)}
            onChange={(event) => setHost(event.target.value)}
            onKeyDown={enterToConnect}
            placeholder="192.0.2.10"
            spellCheck={false}
            value={host}
          />
        </LiveLinkField>
        {connection && (
          <LiveLinkField caption="OSC UDP">
            <Input aria-label="OSC UDP port" className={`${smallBox} ${CONTROL_40} ${connection.held.port ? "!border-warning" : ""}`} error={Boolean(udpError)} inputMode="numeric" onChange={(event) => connection.onPort(event.target.value)} value={connection.oscPort} />
          </LiveLinkField>
        )}
        <LiveLinkField caption="API">
          <Input aria-label="API port" className={`${smallBox} ${CONTROL_40}`} error={Boolean(apiError)} inputMode="numeric" onChange={(event) => setApiPort(event.target.value)} onKeyDown={enterToConnect} value={shownApiPort} />
        </LiveLinkField>
        {/* The Lighting primary, never dimmed unless the panel itself is off; while a request is in flight it shows a busy state. */}
        <Button
          className={`live-link-connect ${CONTROL_40} !border-integration-lighting-soft !bg-surface-950 !text-ink can-hover:!bg-surface-900`}
          disabled={!lightingEnabled}
          loading={busy}
          loadingLabel={"Checking\u2026"}
          onClick={() => void connect()}
          variant="tinted"
          tone="lighting"
        >
          {connected && !changed ? "Check again" : "Connect"}
        </Button>
        <div className="live-link-modes flex flex-wrap items-end gap-x-2.5 gap-y-2">
          <ConnectionMethod
            disabled={busy}
            field
            label="Mode"
            onChange={(mode) => {
              if (mode !== "record") { void run(toMode(mode)); return; }
              if (looks?.edited && !confirmRerecordLooks()) return;
              void run(() => setLightkeyRecording(true));
            }}
            options={[
              { value: "auto", label: "Auto" },
              { value: "replay", label: "Replay" },
              { value: "record", label: "Record", disabled: !status.recording && !connected },
            ]}
            value={shownMode}
          />
        </div>
      </div>
      {(hostError || udpError || apiError) && (
        <p className="mt-1 text-type-body-sm text-danger-text" role="alert">{[hostError, udpError, apiError].filter(Boolean).join(" ")}</p>
      )}
      {otherHost !== null && (
        <div className="mt-2 flex flex-wrap items-center gap-2" data-testid="lightkey-host-notice" role="status">
          <p className="text-type-body-sm text-warning-text">
            {`The live-link API is set to ${otherHost}, not the OSC host ${savedOscHost}. It keeps using ${otherHost} until you change it.`}
          </p>
          <Button className={CONTROL_34} disabled={busy} onClick={() => void acceptOscHost()} variant="secondary" tone="lighting">Use the OSC host</Button>
        </div>
      )}

      {!status.recording && status.replay === false && (
        <div className="mt-2 flex flex-wrap items-center gap-2" data-testid="lightkey-replay-off" role="status">
          <p className="text-type-body-sm text-warning-text">
            Replay is off, so saved lights will not play. The toggle above does not show this.
          </p>
          <Button disabled={busy} onClick={() => void run(() => saveLightkeySettings({ replay: true }))} variant="secondary" tone="lighting">
            Turn Replay on
          </Button>
        </div>
      )}

      <LiveLinkTools
        footer={footer}
        release={(
          <Button
            className="!h-8 !min-h-8 !px-2.5 text-sm coarse:!h-11 coarse:!min-h-[44px]"
            disabled={busy || !releasable}
            onClick={() => void run(() => releaseLightkeyLights())}
            variant="secondary"
          >
            Release lights
          </Button>
        )}
        stateText={stateText}
        testCue={testCue}
      />

      {status.recording && (
        <LoudnessPicker disabled={loudness?.disabled} hasLoudness={Boolean(loudness?.present)} onClear={loudness?.onClear} />
      )}
    </LiveLinkShell>
  );
}
