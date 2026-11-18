import { openExternalUrl } from "../desktop";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { scanSummary } from "./playbackStatus";
import { Button } from "./ui/Button";
import { InlineAlert } from "./ui/InlineAlert";

const linkButton = "min-h-ds-touch-target touch-manipulation text-integration-playback-text underline transition-colors duration-[140ms] motion-reduce:transition-none can-hover:text-integration-playback active:text-integration-playback-text";
const LOCAL_NETWORK_URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";
const CHECKLIST = ["Playback is open on a computer", "Remote Connections is turned on in Playback", "Both computers are on the same network"];

/**
 * What the Playback Live Link says under its connection row after a scan: why it found nothing (macOS blocking the local network, a
 * checklist), the chooser when it found several, and the error or message of the last request. It is part of the connection, so it
 * stays in view (a chooser or a macOS block cannot sit behind a collapsed button).
 */
export function PlaybackScanHelp({ playback, scanning, copied, onCopy, onChoose, onOther }: {
  playback?: PlaybackController;
  scanning: boolean;
  copied: boolean;
  onCopy: (text: string) => void;
  onChoose: (host: string) => void;
  onOther: () => void;
}) {
  const scan = playback?.status?.scan;
  return (
    <div className="mt-2 space-y-3">
      {scan?.state === "not_found" && !scanning && (scan.error_class === "permission_denied"
        ? (
          <InlineAlert tone="danger" className="p-3 text-ink-body" data-testid="scan-error">
            <p className="font-semibold">macOS is blocking StagePilot&apos;s local-network access</p>
            <ol className="mt-2 list-decimal pl-5 space-y-1">
              <li>Quit StagePilot.</li>
              <li>Open System Settings &gt; Privacy &amp; Security &gt; Local Network.</li>
              <li>Turn StagePilot off and on again (if it is not listed, run Scan Network once and click Allow when macOS asks).</li>
              <li>Reopen StagePilot and scan again.</li>
            </ol>
            <div className="mt-2 flex flex-wrap gap-3">
              <button className={linkButton} type="button" onClick={() => void openExternalUrl(scan.settings_url ?? LOCAL_NETWORK_URL)}>Open Local Network settings</button>
              {scan.details && <button className={linkButton} type="button" onClick={() => void onCopy(scan.details as string)}>{copied ? "Copied" : "Copy diagnostic details"}</button>}
              {!scan.typed && <button className={linkButton} type="button" onClick={() => onOther()}>Enter address instead</button>}
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
                {scan.details && <button className={linkButton} type="button" onClick={() => void onCopy(scan.details as string)}>{copied ? "Copied" : "Copy diagnostic details"}</button>}
                {!scan.typed && <button className={linkButton} type="button" onClick={() => onOther()}>Enter address instead</button>}
              </div>
            </InlineAlert>
          )
          : (
            <div className="rounded-ds-lg bg-edge-subtle p-3 text-sm text-ink-body">
              <p className="font-semibold">Make sure:</p>
              <ul className="mt-1 list-disc pl-5 text-ink-body">{CHECKLIST.map((line) => <li key={line}>{line}</li>)}</ul>
              {scan.hosts_total ? <p className="mt-1 text-xs tabular-nums text-ink-muted">{scanSummary(scan)}</p> : null}
              <button className={`mt-2 ${linkButton}`} type="button" onClick={() => onOther()}>Enter address instead</button>
            </div>
          ))}
      {scan?.state === "found" && scan.candidates.length > 1 && !scanning && (
        <div className="rounded-ds-lg bg-edge-subtle p-3" role="group" aria-label="Choose which Playback">
          <p className="text-sm font-semibold text-ink">Choose which Playback</p>
          <ul className="mt-2 grid gap-2">{scan.candidates.map((candidate) => (
            <li key={candidate.host}><Button className="w-full justify-start text-left" onClick={() => onChoose(candidate.host)} tone="playback" variant="tinted">{candidate.name} <span className="ml-1.5 text-xs text-ink-muted">{candidate.host}</span></Button></li>
          ))}</ul>
        </div>
      )}
      {playback?.error && <p role="alert" className="text-sm text-danger-text">{playback.error}</p>}
      {playback?.message && <p role="status" className="text-sm text-integration-playback-text">{playback.message}</p>}
    </div>
  );
}
