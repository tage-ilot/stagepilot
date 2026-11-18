import { useEffect, useId, useRef, useState } from "react";

import { openExternalUrl } from "../desktop";
import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { CONTROL_34 } from "./compactControl";
import { CONTROL_40 } from "./liveLink";
import { DisclosureButton, LiveLinkField, LiveLinkPill, LiveLinkShell } from "./LightingOscParts";
import { PlaybackInstallBox } from "./PlaybackInstallBox";
import { apiGood, apiLine, pillParts, scriptLine, scriptNote } from "./playbackLiveLinkState";
import { scanProgressText, scanSummary, validPlaybackHost } from "./playbackStatus";
import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";
import { Field } from "./ui/Field";
import { InlineAlert } from "./ui/InlineAlert";
import { Input } from "./ui/Input";
import { Select } from "./ui/Select";

const linkButton = "min-h-ds-touch-target touch-manipulation text-integration-playback-text underline transition-colors duration-[140ms] motion-reduce:transition-none can-hover:text-integration-playback active:text-integration-playback-text";
const LOCAL_NETWORK_URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";
const OTHER_COMPUTER = "__other__";
const CHECKLIST = ["Playback is open on a computer", "Remote Connections is turned on in Playback", "Both computers are on the same network"];
const SCAN_CANCEL_GUARD_MS = 600;
const TITLE = "Playback Live Link";

/**
 * The first block of the Playback API panel, built like the Lighting "sACN DMX Live Link" block: a bordered card with a title, a state pill
 * beside it, two status lines (the Playback API and the Library Link), labelled fields and the same spacing.
 *
 * It holds the Playback host scan and chooser (Scan Network, progress, Scan again, the macOS permission help, the candidate chooser,
 * Other computer, Connect), moved here whole. Choosing a computer sets the API host AND the Library Link's address (the helper
 * is on the same computer, so its address follows the Playback host; an Advanced override is cleared when another computer is chosen).
 *
 * The Library Link is optional. Without it nothing here is a warning: the line says "Not installed" in grey and the setup box offers
 * one command. Everything the library feeds elsewhere waits for the script to be ready (or outdated, which still works).
 */
export function PlaybackLiveLink({ playback, library }: { playback?: PlaybackController; library?: PlaybackLibraryController }) {
  const status = playback?.status;
  const [host, setHost] = useState("");
  const [choice, setChoice] = useState("");
  const [copied, setCopied] = useState(false);
  const [forgetOpen, setForgetOpen] = useState(false);
  const [override, setOverride] = useState("");
  // The two quiet boxes under the strip, opened by its buttons (null = the default: the setup box is open while the script is not ready).
  const [setupOpen, setSetupOpen] = useState<boolean | null>(null);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const forgetId = useId();
  const setupId = useId();
  const advancedId = useId();
  useEffect(() => { setHost(status?.settings.host ?? ""); setChoice(status?.settings.host ?? ""); }, [status?.settings.host]);
  const savedOverride = (library?.address ?? "").replace(/^http:\/\//, "");
  useEffect(() => { setOverride(savedOverride); }, [savedOverride]);

  const scan = status?.scan;
  const scanning = scan?.state === "scanning" || playback?.pending === "scan";
  const other = choice === OTHER_COMPUTER;
  const trimmed = other ? host.trim() : choice;
  const hostOk = validPlaybackHost(trimmed);
  const savedHost = status?.settings.host ?? null;
  const candidates = status?.scan.candidates ?? [];
  const computers = [
    ...candidates.map((candidate) => ({ host: candidate.host, label: `${candidate.name} (${candidate.host})` })),
    ...(savedHost && !candidates.some((candidate) => candidate.host === savedHost) ? [{ host: savedHost, label: `${savedHost} (saved)` }] : []),
  ];

  const copyDetails = async (text: string) => {
    try { await navigator.clipboard.writeText(text); setCopied(true); } catch { setCopied(false); }
  };
  // The scan button turns into Cancel once the scan starts, so the second click of a double click
  // must not land on Cancel. Clicks on Cancel in the first moments are ignored.
  const scanClickedAt = useRef(0);
  const startScan = () => { scanClickedAt.current = Date.now(); playback?.scan(); };
  const cancelScan = () => { if (Date.now() - scanClickedAt.current < SCAN_CANCEL_GUARD_MS) return; playback?.cancelScan(); };
  // Choosing a computer sets the Playback API host (saved by the scan) and, with it, the Library Link's address: the script is asked at
  // the Playback host unless Advanced names another address, and a name typed for the old computer would be wrong for the new one.
  const choose = (chosen: string) => {
    scanClickedAt.current = Date.now();
    playback?.scan(chosen);
    if (library?.address) void library.saveAddress("");
  };
  const connect = () => { if (trimmed && hostOk) choose(trimmed); };

  const api = status ? apiLine(status, "Checking Playback…") : { tone: "idle" as const, text: playback?.error ?? "Checking Playback…" };
  const hasComputer = Boolean(library?.target || savedHost);
  const script = library ? scriptLine(library.status, hasComputer) : null;
  const parts = pillParts({ good: apiGood(status), line: api }, library?.status ?? null, hasComputer);
  const scriptState = library?.status?.helper.state ?? null;
  const needsSetup = scriptState !== "ready" && scriptState !== null;
  const setupShown = setupOpen ?? needsSetup;
  const setupLabel = scriptState === "outdated" ? "Update the Library Link" : "Library Link setup";
  const note = scriptNote(library?.status ?? null);
  const paired = Boolean(library?.status?.helper.paired) && library?.status?.helper.state !== "forgotten";
  const overrideChanged = override.trim() !== savedOverride;
  const busy = library?.pending != null;

  return (
    <LiveLinkShell
      className="!mt-0"
      pill={(
        <>
          <LiveLinkPill detail={api.text} name="" testId="playback-api-pill" text={`Playback API: ${parts.api.word}`} tone={parts.api.tone} />
          {library && <LiveLinkPill detail={script?.text} name="" testId="library-link-pill" text={`Library Link: ${parts.script.word}`} tone={parts.script.tone} />}
        </>
      )}
      testId="playback-live-link"
      title={TITLE}
    >
      {status?.scan.state === "scanning" && <p className="mt-2 text-[0.8125rem] tabular-nums text-ink-muted" data-testid="scan-progress">{scanProgressText(status.scan)}</p>}

      <form className="mt-2 flex flex-wrap items-end gap-x-2.5 gap-y-2" data-testid="playback-live-link-line" id="playback-specific" onSubmit={(event) => { event.preventDefault(); connect(); }}>
        <LiveLinkField caption="Playback computer" className="live-link-host">
          <Select aria-label="Playback computer" className={`w-full font-mono ${CONTROL_40}`} onChange={(event) => setChoice(event.target.value)} placeholder="Choose a computer…" value={computers.some((computer) => computer.host === choice) || other ? choice : ""}>
            {computers.map((computer) => <option key={computer.host} value={computer.host}>{computer.label}</option>)}
            <option value={OTHER_COMPUTER}>Other computer…</option>
          </Select>
        </LiveLinkField>
        {other && (
          <LiveLinkField caption="Computer name or address" className="live-link-host">
            <Input aria-label="Computer name or address" className={`w-full font-mono ${CONTROL_40}`} error={!hostOk} onChange={(event) => setHost(event.target.value)} placeholder="For example, 192.0.2.10…" spellCheck={false} value={host} />
          </LiveLinkField>
        )}
        {scanning
          ? <Button className={`flex-none ${CONTROL_40}`} onClick={cancelScan} tone="playback" type="button" variant="tinted">Scanning… (cancel)</Button>
          : <Button className={`flex-none ${CONTROL_40}`} onClick={startScan} tone="playback" type="button" variant="tinted">{scan?.state === "not_found" ? "Scan again" : "Scan Network"}</Button>}
        {choice && <Button className={CONTROL_40} disabled={!trimmed || !hostOk || scanning} tone="playback" type="submit" variant="tinted">Connect</Button>}
        {other && !trimmed && <p className="w-full text-xs text-ink-muted">Type an address to connect.</p>}
        {other && !hostOk && <p className="w-full text-sm text-danger-text" role="alert">Enter a computer name or address like 192.0.2.10 (no http://, port or spaces).</p>}
      </form>

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
                <p className="font-semibold">Make sure:</p>
                <ul className="mt-1 list-disc pl-5 text-ink-body">{CHECKLIST.map((line) => <li key={line}>{line}</li>)}</ul>
                {scan.hosts_total ? <p className="mt-1 text-xs tabular-nums text-ink-muted">{scanSummary(scan)}</p> : null}
                <button className={`mt-2 ${linkButton}`} type="button" onClick={() => setChoice(OTHER_COMPUTER)}>Enter address instead</button>
              </div>
            ))}
        {scan?.state === "found" && scan.candidates.length > 1 && !scanning && (
          <div className="rounded-ds-lg bg-edge-subtle p-3" role="group" aria-label="Choose which Playback">
            <p className="text-sm font-semibold text-ink">Choose which Playback</p>
            <ul className="mt-2 grid gap-2">{scan.candidates.map((candidate) => (
              <li key={candidate.host}><Button className="w-full justify-start text-left" onClick={() => choose(candidate.host)} tone="playback" variant="tinted">{candidate.name} <span className="ml-1.5 text-xs text-ink-muted">{candidate.host}</span></Button></li>
            ))}</ul>
          </div>
        )}
        {playback?.error && <p role="alert" className="text-sm text-danger-text">{playback.error}</p>}
        {playback?.message && <p role="status" className="text-sm text-integration-playback-text">{playback.message}</p>}
      </div>

      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2 text-[0.78125rem] text-ink-muted" data-testid="live-link-sub">
        <p aria-live="polite" className="min-w-0 flex-[1_1_16rem]" role="status">
          <span className="block sm:inline" data-testid="playback-banner">{api.text}</span>
          {script && (
            <>
              <span aria-hidden="true" className="hidden sm:inline">{" \u00B7 "}</span>
              <span className="block sm:inline" data-testid="library-link-status" data-tone={script.tone}>Library Link: {script.text}.</span>
            </>
          )}
        </p>
        {library && (
          <div className="flex min-w-0 flex-wrap items-center gap-2" data-testid="live-link-actions">
            <DisclosureButton controls={setupId} onToggle={() => setSetupOpen(!setupShown)} open={setupShown}>{setupLabel}</DisclosureButton>
            <DisclosureButton controls={advancedId} onToggle={() => setAdvancedOpen(!advancedOpen)} open={advancedOpen}>Advanced</DisclosureButton>
          </div>
        )}
      </div>

      {library && (
        <>
          {note && <p className="mt-2 text-type-body-sm text-warning-text" data-testid="library-link-note" role="status">{note}</p>}
          <div hidden={!setupShown} id={setupId}>
            <PlaybackInstallBox hasComputer={hasComputer} library={library} />
          </div>
          {library.error && <p className="mt-2 text-sm text-danger-text" role="alert">{library.error}</p>}
          <div className="mt-2 rounded-ds-lg border border-edge bg-surface-900/60 p-3" data-testid="playback-live-link-advanced" hidden={!advancedOpen} id={advancedId}>
            <div className="space-y-3">
              <form className="flex flex-wrap items-end gap-3" onSubmit={(event) => { event.preventDefault(); void library.saveAddress(override.trim()); }}>
                <Field className="w-full flex-none sm:min-w-48 sm:flex-1" label="Library Link address">
                  <Input className="font-mono" onChange={(event) => setOverride(event.target.value)} placeholder="Same as the Playback computer" spellCheck={false} value={override} />
                </Field>
                <Button className={CONTROL_34} disabled={busy || !overrideChanged} tone="playback" type="submit" variant="tinted">{library.pending === "save" ? "Saving…" : "Save"}</Button>
              </form>
              <p className="text-xs text-ink-muted">Leave this empty: the Library Link is on the same computer as the Playback API (port 8780). Fill it in only if it runs on another computer or port.</p>
              {paired && (
                <div>
                  <Button className={CONTROL_34} disabled={busy} onClick={() => setForgetOpen(true)} tone="playback" type="button" variant="tinted">Forget the Library Link</Button>
                  <p className="mt-1 text-xs text-ink-muted" id={forgetId}>Deletes the key StagePilot made for it. The Library Link keeps running on the Playback computer but StagePilot can no longer use it.</p>
                </div>
              )}
            </div>
          </div>
          <Dialog
            actions={
              <>
                <Button autoFocus onClick={() => setForgetOpen(false)} size="touch" tone="playback" variant="tinted">Cancel</Button>
                <Button onClick={() => { setForgetOpen(false); void library.forget(); }} size="touch" tone="playback" variant="tinted">Forget the Library Link</Button>
              </>
            }
            description="StagePilot deletes the key it made for the Library Link. The Library Link keeps running on the Playback computer, but StagePilot can no longer use it. Song details (key, BPM, arrangement) go away until you run the setup command again."
            dismissible={false}
            open={forgetOpen}
            title="Forget the Library Link?"
          />
        </>
      )}
    </LiveLinkShell>
  );
}
