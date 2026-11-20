import { useEffect, useRef, useState } from "react";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import { rememberAddress, useRecentAddresses } from "../hooks/useRecentAddresses";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { CONTROL_40 } from "./liveLink";
import { helperUpdateLine, updatesItself } from "./lightkeyProject";
import { playbackBoxProps } from "./helperBoxProps";
import { PlaybackScanHelp } from "./PlaybackScanHelp";
import { ConnectionRow, ConnectionSection, HelperSetupBox, LiveLinkField, LiveLinkPill, SectionNotice } from "./sections";
import { apiGood, apiLine, pillParts, scriptLine, scriptNote } from "./playbackLiveLinkState";
import { scanProgressText, validPlaybackHost } from "./playbackStatus";
import { Button } from "./ui/Button";
import { Input } from "./ui/Input";
import { Select } from "./ui/Select";

const OTHER_COMPUTER = "__other__";
const SCAN_CANCEL_GUARD_MS = 600;
const TITLE = "Playback Live Link";

/**
 * The first block of the Playback API panel, built like the Lighting "sACN DMX Live Link" block: a bordered card with a title, a state pill
 * beside it, two status lines (the Playback API and the Library Link), labelled fields and the same spacing.
 *
 * It holds the Playback host scan and chooser (Scan Network, progress, Scan again, the macOS permission help, the candidate chooser,
 * Other computer, Connect), moved here whole. Choosing a computer sets the API host AND the Library Link's address (the helper
 * is on the same computer, so its address follows the Playback host; a stray address from an older version is cleared).
 *
 * The Library Link is optional. Without it nothing here is a warning: the line says "Not installed" in grey and the setup box offers
 * one command. Everything the library feeds elsewhere waits for the script to be ready (or outdated, which still works).
 */
export function PlaybackLiveLink({ playback, library }: { playback?: PlaybackController; library?: PlaybackLibraryController }) {
  const status = playback?.status;
  const [host, setHost] = useState("");
  const [choice, setChoice] = useState("");
  const [copied, setCopied] = useState(false);
  useEffect(() => { setHost(status?.settings.host ?? ""); setChoice(status?.settings.host ?? ""); }, [status?.settings.host]);
  // The Library Link is always on the Playback computer: an address typed in an older version is cleared once.
  const strayAddress = Boolean(library?.address);
  const cleared = useRef(false);
  useEffect(() => {
    if (!strayAddress || cleared.current || !library) return;
    cleared.current = true;
    void library.saveAddress("");
  }, [strayAddress, library]);

  const recent = useRecentAddresses("playback");
  const scan = status?.scan;
  const scanning = scan?.state === "scanning" || playback?.pending === "scan";
  const other = choice === OTHER_COMPUTER;
  const trimmed = other ? host.trim() : choice;
  const hostOk = validPlaybackHost(trimmed);
  const savedHost = status?.settings.host ?? null;
  const connectedNow = apiGood(status);
  useEffect(() => { if (connectedNow) rememberAddress("playback", savedHost); }, [connectedNow, savedHost]);
  const candidates = status?.scan.candidates ?? [];
  const computers = [
    ...candidates.map((candidate) => ({ host: candidate.host, label: `${candidate.name} (${candidate.host})` })),
    ...(savedHost && !candidates.some((candidate) => candidate.host === savedHost) ? [{ host: savedHost, label: `${savedHost} (saved)` }] : []),
    // The last few that connected, so a computer used before is one choice and Connect, with no scan.
    ...recent.filter((address) => address !== savedHost && !candidates.some((candidate) => candidate.host === address)).map((address) => ({ host: address, label: `${address} (recent)` })),
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
  // A script that is behind and can update itself is updated by StagePilot: its setup box stays closed, with only a line to read.
  const updatesBySelf = scriptState === "outdated" && updatesItself(library?.status?.helper.update);
  const needsSetup = scriptState !== "ready" && scriptState !== null && !updatesBySelf;
  const setupLabel = scriptState === "outdated" && !updatesBySelf ? "Update the Library Link" : "Library Link setup";
  const updateLine = helperUpdateLine(library?.status?.helper.update);
  const note = scriptNote(library?.status ?? null);

  return (
    <ConnectionSection
      attention={Boolean(library) && (needsSetup || Boolean(note))}
      className="!mt-0"
      connection={(
        <>
          {status?.scan.state === "scanning" && <p className="mt-2 text-[0.8125rem] tabular-nums text-ink-muted" data-testid="scan-progress">{scanProgressText(status.scan)}</p>}

          <ConnectionRow as="form" id="playback-specific" onSubmit={(event) => { event.preventDefault(); connect(); }} testId="playback-live-link-line">
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
            {other && !hostOk && <SectionNotice className="w-full" role="alert" tone="danger" variant="line">Enter a computer name or address like 192.0.2.10 (no http://, port or spaces).</SectionNotice>}
          </ConnectionRow>

          <PlaybackScanHelp copied={copied} onChoose={choose} onCopy={copyDetails} onOther={() => setChoice(OTHER_COMPUTER)} playback={playback} scanning={scanning} />
        </>
      )}
      details={library ? ({ byChoice }) => (
        <>
          {note && <SectionNotice className="mt-2" role="status" size="xs" testId="library-link-note" tone="warning" variant="line">{note}</SectionNotice>}
          {updateLine && <SectionNotice className="mt-2" quiet role="status" size="xs" testId="library-link-update" tone="info" variant="line">{updateLine}</SectionNotice>}
          <HelperSetupBox {...playbackBoxProps(library, hasComputer)} autoStart={byChoice} />
        </>
      ) : undefined}
      detailsLabel={setupLabel}
      footer={library?.error ? <SectionNotice className="mt-2" role="alert" tone="danger" variant="line">{library.error}</SectionNotice> : undefined}
      pills={(
        <>
          <LiveLinkPill detail={api.text} name="" testId="playback-api-pill" text={`Playback API: ${parts.api.word}`} tone={parts.api.tone} />
          {library && <LiveLinkPill detail={script?.text} name="" testId="library-link-pill" text={`Library Link: ${parts.script.word}`} tone={parts.script.tone} />}
        </>
      )}
      status={[
        { text: api.text, testId: "playback-banner" },
        ...(script ? [{ text: `Library Link: ${script.text}.`, testId: "library-link-status", tone: script.tone }] : []),
      ]}
      testId="playback-live-link"
      title={TITLE}
    />
  );
}
