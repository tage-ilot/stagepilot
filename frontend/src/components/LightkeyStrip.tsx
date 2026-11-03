import { useCallback, useEffect, useRef, useState } from "react";

import {
  clearLightkeyTake,
  getLightkeyStatus,
  saveLightkeySettings,
  setLightkeyRecording,
} from "../api";
import { describeError } from "../errorText";
import type { LightkeyActionResponse, LightkeyFixtureStatus, LightkeyLink, LightkeyStatus, LightkeyTakeSummary } from "../types";
import { Button } from "./ui/Button";
import { ConnectionMethod } from "./ui/ConnectionMethod";
import { Input } from "./ui/Input";
import { StatusDot } from "./ui/StatusDot";
import type { Tone } from "./ui/types";

const POLL_MS = 1500;

const linkWord: Record<LightkeyLink, string> = {
  off: "Off",
  connected: "Connected",
  lightkey_closed: "Lightkey is closed",
  unreachable: "Not connected",
};
const linkTone: Record<LightkeyLink, Tone> = {
  off: "idle",
  connected: "success",
  lightkey_closed: "warning",
  unreachable: "danger",
};

const fixtureTone: Record<LightkeyFixtureStatus["link"], Tone> = { waiting: "idle", live: "success", lost: "warning" };

/** The fixture-level line: what is captured, in words. */
function lightkeyFixtureWord(fixtures: LightkeyFixtureStatus): string {
  if (fixtures.link === "live") return `Colour and levels: ${fixtures.fixtures} fixture${fixtures.fixtures === 1 ? "" : "s"}`;
  if (fixtures.link === "waiting") return "Colour and levels: connecting";
  return fixtures.detail ?? "Colour and levels: not receiving";
}

function takeDetail(take: LightkeyTakeSummary): string {
  if (!take.fixture_event_count) return "no fixture capture";
  return `${take.fixture_event_count} colour/level change${take.fixture_event_count === 1 ? "" : "s"}`;
}

/** The line under the Replay | Record toggle: the one thing the operator most needs to know right now. */
export function lightkeyHeadline(status: LightkeyStatus): string {
  if (status.recording && status.recording_song) return `Recording “${status.recording_song}”.`;
  if (status.recording) return "Recording starts with the next song.";
  if (status.replaying_song) return `Replaying “${status.replaying_song}”.`;
  if (status.replay_blocked && status.takes.length > 0) return status.replay_blocked;
  if (status.link === "connected" && status.takes.length === 0) {
    return "Switch to Record, then start a song. Its lights are captured with the countdown.";
  }
  return status.takes.length === 1 ? "1 song captured." : `${status.takes.length} songs captured.`;
}

/** Same text, ignoring quote style and spacing (the backend writes straight quotes, the headline curly ones). */
export function normaliseLine(text: string): string {
  return text.replace(/[\u201C\u201D\u201E]/g, '"').replace(/[\u2018\u2019]/g, "'").replace(/\s+/g, " ").trim();
}

/** The lines under the toggle, top to bottom, never the same thing twice: what just happened (an error or the backend message), then the headline. */
export function lightkeyLines(status: LightkeyStatus, error: string | null): string[] {
  const candidates = [
    error ?? status.message ?? lightkeyHeadline(status),
    lightkeyHeadline(status),
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

/**
 * Lightkey live link, kept to the minimum: connect, record, replay.
 * Captures the fixtures' colour and levels from the lightkey-api agent on the Lightkey Mac and holds them with overrides on replay.
 */
export function LightkeyStrip({ lightingEnabled }: { lightingEnabled: boolean }) {
  const [status, setStatus] = useState<LightkeyStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [address, setAddress] = useState<string | null>(null); // null = untouched, show the saved one
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
    return (
      <section aria-label="Lightkey live link" className="mt-6">
        <h3 className="font-semibold text-ink">Lightkey live link</h3>
        <p className="mt-2 text-type-body-sm text-ink-muted">{error ?? "Loading…"}</p>
      </section>
    );
  }

  const shownAddress = address ?? status.api_url;
  const addressChanged = address !== null && address.trim() !== status.api_url;
  const connected = status.link === "connected";
  const connect = () => run(async () => {
    const response = await saveLightkeySettings({ api_url: (address ?? status.api_url).trim() });
    setAddress(null);
    return response;
  });

  // Record -> Replay: stop recording first, then make sure replay is on. A failure leaves the toggle on what the
  // backend says (it is re-read), and the message is shown.
  const backToReplay = async (): Promise<LightkeyActionResponse> => {
    try {
      let response: LightkeyActionResponse | null = null;
      if (status.recording) {
        response = await setLightkeyRecording(false);
        if (!response.accepted) return response;
      }
      if (!status.replay || response === null) {
        response = await saveLightkeySettings({ replay: true });
      }
      return response;
    } catch (cause) {
      await load();
      throw cause;
    }
  };

  const [headline, ...details] = lightkeyLines(status, error);
  return (
    <section aria-label="Lightkey live link" className="mt-6" data-testid="lightkey-strip">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h3 className="font-semibold text-ink">Lightkey live link</h3>
        <span className="inline-flex items-center gap-2 text-sm text-ink-body" data-testid="lightkey-link">
          <StatusDot ignoreAccent label={linkWord[status.link]} tone={linkTone[status.link]} />
          {linkWord[status.link]}
        </span>
      </div>
      {status.link !== "connected" && status.link !== "off" && status.detail && (
        <p className="mt-1 text-type-body-sm text-ink-muted">{status.detail}</p>
      )}
      {connected && status.fixtures && (
        <p className="mt-1 inline-flex items-center gap-2 text-type-body-sm text-ink-muted" data-testid="lightkey-fixtures">
          <StatusDot ignoreAccent label={lightkeyFixtureWord(status.fixtures)} tone={fixtureTone[status.fixtures.link]} />
          {lightkeyFixtureWord(status.fixtures)}
        </p>
      )}

      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Input
          aria-label="Lightkey API address"
          className="w-full sm:w-72"
          onChange={(event) => setAddress(event.target.value)}
          onKeyDown={(event) => { if (event.key === "Enter") void connect(); }}
          placeholder="10.30.0.211:8770"
          value={shownAddress}
        />
        <Button
          disabled={busy || !lightingEnabled}
          onClick={() => void connect()}
          variant={connected && !addressChanged ? "secondary" : "tinted"}
          tone="lighting"
        >
          {connected && !addressChanged ? "Check again" : "Connect"}
        </Button>
      </div>

      <ConnectionMethod
        className="mt-[-1.25rem]"
        disabled={busy}
        label="Lightkey mode"
        onChange={(mode) => void (mode === "record" ? run(() => setLightkeyRecording(true)) : run(backToReplay))}
        options={[
          { value: "replay", label: "Replay" },
          { value: "record", label: "Record", disabled: !status.recording && !connected },
        ]}
        value={status.recording ? "record" : "replay"}
      />

      <p aria-live="polite" className="mt-1 text-type-body-sm text-ink-muted" data-testid="lightkey-headline">
        {headline}
      </p>
      {details.map((line) => (
        <p className="text-type-body-sm text-ink-muted" data-testid="lightkey-detail" key={line}>{line}</p>
      ))}

      {status.takes.length > 0 && (
        <ul aria-label="Captured songs" className="mt-2 grid gap-1">
          {status.takes.map((take) => (
            <li className="flex items-center justify-between gap-3 text-sm text-ink-body" key={take.song_key}>
              <span className="min-w-0 truncate">
                {take.song_title}
                <span className="text-ink-muted"> · {takeDetail(take)}</span>
              </span>
              <Button
                aria-label={`Clear capture for ${take.song_title}`}
                disabled={busy}
                onClick={() => void run(() => clearLightkeyTake(take.song_key))}
                variant="secondary"
              >
                Clear
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
