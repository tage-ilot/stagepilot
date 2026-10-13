import { useEffect, useRef, useState, type ReactNode } from "react";

import { openExternalUrl } from "../desktop";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import type { PlaybackDraft } from "./PlaybackSaveFooter";
import { PlanningCenterLengths } from "./PlanningCenterLengths";
import { ScanProgress } from "./ScanProgress";
import { PlaybackSongLengths } from "./PlaybackSongLengths";
import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";
import { Disclosure } from "./ui/Disclosure";
import { Field } from "./ui/Field";
import { InlineAlert } from "./ui/InlineAlert";
import { Input } from "./ui/Input";
import { Select } from "./ui/Select";
import { playbackActivityText, playbackBannerTone, playbackOrderNotice, playbackSongCountMismatch, scanProgressText, scanSummary, validPlaybackHost } from "./playbackStatus";

const linkButton = "min-h-ds-touch-target touch-manipulation text-info-text underline transition-colors duration-[140ms] motion-reduce:transition-none can-hover:text-info active:text-info-active";
const toneClass = {
  connected: "border-success/30 bg-success/10 text-success-text",
  midi: "border-warning/30 bg-warning/10 text-warning-text",
  searching: "border-warning/30 bg-warning/10 text-warning-text",
  idle: "border-edge bg-edge-subtle text-ink-body",
} as const;
const LOCAL_NETWORK_URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";
const OTHER_COMPUTER = "__other__";
const CHECKLIST = ["Playback is open on a computer", "Remote Connections is turned on in Playback", "Both computers are on the same network"];


// A checkbox row with the same held marker as Field heldChange (warning dot, "Not saved yet"), kept out of the label so the accessible name stays the same.
function HeldRow({ held, id, children }: { held: boolean; id: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-x-3">
      <label className="flex min-h-ds-touch-target touch-manipulation items-center gap-3 text-sm text-ink-body">{children}</label>
      {held && <span id={id} data-held-marker className="inline-flex items-center gap-1.5 text-type-body-sm text-warning-text"><span aria-hidden="true" className="h-2 w-2 rounded-ds-full bg-warning" />Not saved yet</span>}
    </div>
  );
}

const SCAN_CANCEL_GUARD_MS = 600;

export function PlaybackApiPanel({ playback, draft }: { playback?: PlaybackController; draft?: PlaybackDraft }) {
  const status = playback?.status;
  const [host, setHost] = useState("");
  const [choice, setChoice] = useState("");
  const [confirm, setConfirm] = useState(false);
  useEffect(() => { setHost(status?.settings.host ?? ""); setChoice(status?.settings.host ?? ""); }, [status?.settings.host]);

  const scan = status?.scan;
  const scanning = scan?.state === "scanning" || playback?.pending === "scan";
  const running = status?.discovery === "running" || playback?.pending === "discover";
  const apiConnected = status?.sources.playback_api.connected ?? false;
  const other = choice === OTHER_COMPUTER;
  const trimmed = other ? host.trim() : choice;
  const hostOk = validPlaybackHost(trimmed);
  const savedHost = status?.settings.host ?? null;
  const candidates = status?.scan.candidates ?? [];
  const computers = [
    ...candidates.map((candidate) => ({ host: candidate.host, label: `${candidate.name} (${candidate.host})` })),
    ...(savedHost && !candidates.some((candidate) => candidate.host === savedHost) ? [{ host: savedHost, label: `${savedHost} (saved)` }] : []),
  ];
  const orderSaved = Boolean(status?.song_order.length) && !status?.stale;
  const notice = status ? playbackOrderNotice(status) : null;
  const mismatch = status && orderSaved ? playbackSongCountMismatch(status) : null;
  // A dialog never outlives the state that allowed it (e.g. Playback started playing).
  const canDiscover = apiConnected && !status?.playing && !running;
  useEffect(() => { if (!canDiscover) setConfirm(false); }, [canDiscover]);

  const [copied, setCopied] = useState(false);
  const copyDetails = async (text: string) => {
    try { await navigator.clipboard.writeText(text); setCopied(true); } catch { setCopied(false); }
  };
  // The scan button turns into Cancel once the scan starts, so the second click of a double click
  // must not land on Cancel. Clicks on Cancel in the first moments are ignored.
  const scanClickedAt = useRef(0);
  const startScan = () => { scanClickedAt.current = Date.now(); playback?.scan(); };
  const cancelScan = () => { if (Date.now() - scanClickedAt.current < SCAN_CANCEL_GUARD_MS) return; playback?.cancelScan(); };
  const connect = () => { if (trimmed && hostOk) { scanClickedAt.current = Date.now(); playback?.scan(trimmed); } };
  const tone = status ? playbackBannerTone(status) : "idle";
  const apiOn = draft?.enabled ?? status?.enabled ?? true;

  return (
    <div className="mt-4 space-y-4">
      <div className={`rounded-ds-xl border p-4 ${toneClass[tone]}`} role="status" aria-live="polite" data-testid="playback-banner">
        <p className="text-base font-semibold">{status ? status.reason : playback?.error ?? "Checking Playback…"}</p>
        {status?.scan.state === "scanning" && <p className="mt-1 text-sm tabular-nums opacity-80" data-testid="scan-progress">{scanProgressText(status.scan)}</p>}
      </div>

      <div className="space-y-3">
        {scanning
          ? <Button className="w-full sm:w-auto" onClick={cancelScan} size="touch" variant="primary">Scanning… (cancel)</Button>
          : <Button className="w-full sm:w-auto" onClick={startScan} size="touch" variant="primary">{scan?.state === "not_found" ? "Scan again" : "Scan Network"}</Button>}
        {scan?.state === "not_found" && !scanning && (scan.error_class === "permission_denied"
          ? (
            <InlineAlert tone="danger" className="p-3 text-ink-body" data-testid="scan-error">
              <p className="font-semibold">macOS is blocking StagePilot's local-network access</p>
              <ol className="mt-2 list-decimal pl-5 space-y-1">
                <li>Quit StagePilot.</li>
                <li>Open System Settings &gt; Privacy &amp; Security &gt; Local Network.</li>
                <li>Turn StagePilot off and on again (if it is not listed, run Scan Network once and click Allow when macOS asks).</li>
                <li>Reopen StagePilot and scan again.</li>
              </ol>
              <div className="mt-2 flex flex-wrap gap-3">
                <button className={linkButton} type="button" onClick={() => void openExternalUrl(scan.settings_url ?? LOCAL_NETWORK_URL)}>Open Local Network settings</button>
                {scan.details && <button className={linkButton} type="button" onClick={() => void copyDetails(scan.details as string)}>{copied ? "Copied" : "Copy diagnostic details"}</button>}
                {!scan.typed && <button className={linkButton} type="button" onClick={() => setChoice(OTHER_COMPUTER)}>Enter address instead</button>}
              </div>
            </InlineAlert>
          )
          : scan.error_class
          ? (
            <InlineAlert tone="danger" className="p-3 text-ink-body" data-testid="scan-error">
              <p className="font-semibold">{scan.typed ? "Couldn't connect" : "Couldn't scan the network"}</p>
              <p className="mt-1">{scan.reason}</p>
              <p className="mt-1 text-xs text-ink-muted">{scanSummary(scan)}</p>
              <div className="mt-2 flex flex-wrap gap-3">
                {scan.settings_url && <button className={linkButton} type="button" onClick={() => void openExternalUrl(scan.settings_url as string)}>Open Local Network settings</button>}
                {scan.details && <button className={linkButton} type="button" onClick={() => void copyDetails(scan.details as string)}>{copied ? "Copied" : "Copy diagnostic details"}</button>}
                {!scan.typed && <button className={linkButton} type="button" onClick={() => setChoice(OTHER_COMPUTER)}>Enter address instead</button>}
              </div>
            </InlineAlert>
          )
          : (
            <div className="rounded-ds-lg bg-edge-subtle p-3 text-sm text-ink-body">
              <p className="font-semibold">Couldn't find Playback</p>
              <ul className="mt-1 list-disc pl-5 text-ink-body">{CHECKLIST.map((line) => <li key={line}>{line}</li>)}</ul>
              {scan.hosts_total ? <p className="mt-1 text-xs tabular-nums text-ink-muted">{scanSummary(scan)}</p> : null}
              <button className={`mt-2 ${linkButton}`} type="button" onClick={() => setChoice(OTHER_COMPUTER)}>Enter address instead</button>
            </div>
          ))}
        {scan?.state === "found" && scan.candidates.length > 1 && !scanning && (
          <div className="rounded-ds-lg bg-edge-subtle p-3" role="group" aria-label="Choose which Playback">
            <p className="text-sm font-semibold text-ink">Choose which Playback</p>
            <ul className="mt-2 grid gap-2">{scan.candidates.map((candidate) => (
              <li key={candidate.host}><Button className="w-full justify-start text-left" onClick={() => playback?.scan(candidate.host)} tone="playback" variant="tinted">{candidate.name} <span className="ml-1.5 text-xs text-ink-muted">{candidate.host}</span></Button></li>
            ))}</ul>
          </div>
        )}
        {playback?.error && <p role="alert" className="text-sm text-danger-text">{playback.error}</p>}
        {playback?.message && <p role="status" className="text-sm text-info-text">{playback.message}</p>}
      </div>

      {status && (
        <section className="space-y-2 rounded-ds-xl bg-integration-playback-soft/5 p-4" aria-labelledby="song-order-heading">
          <h3 id="song-order-heading" className="font-bold text-ink">Song order</h3>
          <div className="flex flex-wrap items-start gap-3"><div className="w-full min-w-0 sm:w-auto sm:flex-1">
          <ScanProgress state={running ? "running" : status.discovery} steps={status.discovery_song ?? 0} total={status.discovery_total ?? 0} previousDuration={status.discovery_duration_seconds} estimatedSongs={status.song_order.length}>
          {!apiConnected ? <p className="text-sm text-ink-body">Connect to Playback first.</p>
              : status.playing ? <p className="text-sm text-warning-text">Stop Playback to set up song order.</p>
                : <>
                  {orderSaved ? <p className="text-sm text-success-text">Song order saved ({status.song_order.length} {status.song_order.length === 1 ? "song" : "songs"})</p>
                    : <>{notice && <p className="text-sm text-warning-text">{notice}</p>}
                      <p className="text-sm text-ink-body">StagePilot steps through your Playback songs once to learn the order. Playback must be stopped.</p></>}
                  <Button className={orderSaved ? undefined : "w-full sm:w-auto"} onClick={() => setConfirm(true)} size={orderSaved ? "control" : "touch"} tone="playback" variant={orderSaved ? "tinted" : "primary"}>{orderSaved ? "Set up again" : "Set up song order"}</Button>
                </>}
          </ScanProgress></div>
          <PlanningCenterLengths status={status} onStatus={playback?.acceptStatus} />
          </div>
          {mismatch && <p className="text-sm text-ink-body">{mismatch}</p>}
          <PlaybackSongLengths status={status} />
          {status.discovery === "failed" && status.last_error && <p role="alert" className="text-sm text-danger-text">{status.last_error}</p>}
        </section>
      )}
      <Dialog
        open={confirm}
        dismissible={false}
        title="Set up song order?"
        description="StagePilot will step through the songs in Playback to read their lengths, then go back to the song that was selected. Playback must stay stopped. Nothing will be played."
        actions={
          <>
            <Button autoFocus onClick={() => { setConfirm(false); playback?.discover(); }} size="touch" tone="playback" variant="tinted">Set up song order</Button>
            <Button onClick={() => setConfirm(false)} size="touch" tone="playback" variant="tinted">Cancel</Button>
          </>
        }
      />

      <div className="space-y-3 border-t border-edge-subtle pt-4">
        <HeldRow held={Boolean(draft && status && draft.enabled !== status.settings.enabled)} id="held-enabled">
          <input className="size-4 accent-primary" type="checkbox" checked={apiOn} disabled={!status || !draft} aria-describedby={draft && status && draft.enabled !== status.settings.enabled ? "held-enabled" : undefined} onChange={(event) => draft?.setEnabled(event.target.checked)} />
          Connect to Playback automatically
        </HeldRow>
        <HeldRow held={Boolean(draft && status && draft.autoScan !== status.settings.auto_scan)} id="held-auto-scan">
          <input className="size-4 accent-primary" type="checkbox" checked={draft?.autoScan ?? true} disabled={!status || !draft} aria-describedby={draft && status && draft.autoScan !== status.settings.auto_scan ? "held-auto-scan" : undefined} onChange={(event) => draft?.setAutoScan(event.target.checked)} />
          Look for Playback on the network if it isn't on this computer
        </HeldRow>
        <HeldRow held={Boolean(draft && status && draft.fastTransport !== status.settings.fast_transport)} id="held-fast-transport">
          <input className="size-4 accent-primary" type="checkbox" checked={draft?.fastTransport ?? status?.settings.fast_transport ?? true} disabled={!status || !draft} aria-describedby={draft && status && draft.fastTransport !== status.settings.fast_transport ? "fast-transport-help held-fast-transport" : "fast-transport-help"} onChange={(event) => draft?.setFastTransport(event.target.checked)} />
          Respond to Play immediately
        </HeldRow>
        <p id="fast-transport-help" className="text-sm text-ink-muted">Reacts to the command instead of waiting up to one second for Playback to report it.</p>
      </div>

      <form id="playback-specific" className="flex flex-wrap items-end gap-3" onSubmit={(event) => { event.preventDefault(); connect(); }}>
        <Field className="w-full flex-none sm:min-w-48 sm:flex-1" label="Computer">
          <Select value={computers.some((computer) => computer.host === choice) || other ? choice : ""} placeholder="Choose a computer…" onChange={(event) => setChoice(event.target.value)}>
            {computers.map((computer) => <option key={computer.host} value={computer.host}>{computer.label}</option>)}
            <option value={OTHER_COMPUTER}>Other computer…</option>
          </Select>
        </Field>
        {other && <Field className="w-full flex-none sm:min-w-48 sm:flex-1" label="Computer name or address">
          <Input value={host} placeholder="For example, 192.0.2.10…" onChange={(event) => setHost(event.target.value)} error={!hostOk} />
        </Field>}
        {choice && <Button type="submit" disabled={!trimmed || !hostOk || scanning} tone="playback" variant="tinted">Connect</Button>}
        {other && !trimmed && <p className="w-full text-xs text-ink-muted">Type an address to connect.</p>}
        {other && !hostOk && <p role="alert" className="w-full text-sm text-danger-text">Enter a computer name or address like 192.0.2.10 (no http://, port or spaces).</p>}
      </form>

      <Disclosure title="Recent activity" variant="activity" count={playback?.events.length ?? 0}>
        {!playback?.events.length ? <p className="text-sm text-ink-muted">Nothing yet.</p> : <ul className="max-h-56 overflow-auto overscroll-contain text-sm text-ink-body" aria-label="Recent activity">
          {[...playback.events].reverse().map(({ event, discovery, at }, index) => {
            const position = event.song_id === null ? -1 : status?.song_order.indexOf(event.song_id) ?? -1;
            const known = !status?.stale && position >= 0 ? position : -1;
            return <li key={`${event.timestamp}-${index}`} className="flex gap-3 py-0.5"><span className="whitespace-nowrap tabular-nums text-ink-muted">{at ? new Date(at).toLocaleTimeString() : ""}</span><span>{playbackActivityText(event.type, known)}{discovery ? " (while setting up song order)" : ""}</span></li>;
          })}
        </ul>}
      </Disclosure>
    </div>
  );
}
