import { useEffect, useId, useRef, useState } from "react";

import type { PlaybackLibraryController } from "../hooks/usePlaybackLibrary";
import type { PlaybackController } from "../hooks/usePlaybackInput";
import { helperUpdateLine, updatesItself } from "./lightkeyProject";
import { playbackBoxProps } from "./helperBoxProps";
import { PLAYBACK_SCAN_WORDS, playbackScanState } from "./playbackScan";
import { NO_SCAN, AddressBox, ConnectionRow, ConnectionSection, HelperSetupBox, LiveLinkField, LiveLinkPill, ScanArea, ScanChooser, ScanHelp, ScanProgress, SectionNotice, scannedAddresses } from "./sections";
import { useRecheckNote } from "./useRecheckNote";
import { apiGood, apiLine, pillParts, scriptLine, scriptNote, withRemoteWords } from "./playbackLiveLinkState";
import { validPlaybackHost } from "./playbackStatus";

const SCAN_CANCEL_GUARD_MS = 600;
const TITLE = "Playback Live Link";
const PLAYBACK_API_NAME = "Playback API";

/**
 * The first block of the Playback API panel, built like the Lighting "sACN DMX Live Link" block: a bordered card with a title, a state pill
 * beside it, two status lines (the Playback API and the Library Link), labelled fields and the same spacing.
 *
 * It holds the Playback host scan and chooser (Scan, progress, the macOS permission help, the candidate chooser,
 * Other computer, Connect), moved here whole. Scan and Connect are the row's one pair, the same in every Live Link block. Choosing a computer sets the API host AND the Library Link's address (the helper
 * is on the same computer, so its address follows the Playback host; a stray address from an older version is cleared).
 *
 * The Library Link is optional. Without it nothing here is a warning: the line says "Not installed" in grey and the setup box offers
 * one command. Everything the library feeds elsewhere waits for the script to be ready (or outdated, which still works).
 */
export function PlaybackLiveLink({ playback, library }: { playback?: PlaybackController; library?: PlaybackLibraryController }) {
  const status = playback?.status;
  const [host, setHost] = useState("");
  const hostField = useRef<HTMLInputElement>(null);
  const errorId = useId();
  const [copied, setCopied] = useState(false);
  useEffect(() => { setHost(status?.settings.host ?? ""); }, [status?.settings.host]);
  // The Library Link is always on the Playback computer: an address typed in an older version is cleared once.
  const strayAddress = Boolean(library?.address);
  const cleared = useRef(false);
  useEffect(() => {
    if (!strayAddress || cleared.current || !library) return;
    cleared.current = true;
    void library.saveAddress("");
  }, [strayAddress, library]);

  const scan = status ? playbackScanState(status.scan) : NO_SCAN;
  const busy = scan.state === "scanning" || playback?.pending === "scan";
  // What was pressed: a typed or chosen computer is Connect, the network scan is Scan. The backend marks a one-address scan `typed`.
  const [pressed, setPressed] = useState<"scan" | "connect" | null>(null);
  useEffect(() => { if (!busy) setPressed(null); }, [busy]);
  const connecting = busy && (scan.typed === true || pressed === "connect");
  const scanning = busy && !connecting;
  const recheck = useRecheckNote();
  const trimmed = host.trim();
  const hostOk = validPlaybackHost(trimmed);
  const savedHost = status?.settings.host ?? null;
  const connectedNow = apiGood(status);
  // The computers the last scan found, and the last few that connected, are the lists of the arrow in the box: a computer used before is one pick and Connect.
  const scanned = scannedAddresses(scan);

  const copyDetails = async (text: string) => {
    try { await navigator.clipboard.writeText(text); setCopied(true); } catch { setCopied(false); }
  };
  // The scan button turns into Cancel once the scan starts, so the second click of a double click
  // must not land on Cancel. Clicks on Cancel in the first moments are ignored.
  const scanClickedAt = useRef(0);
  const startScan = () => { scanClickedAt.current = Date.now(); setPressed("scan"); recheck.clear(); playback?.scan(); };
  const cancelScan = () => { if (Date.now() - scanClickedAt.current < SCAN_CANCEL_GUARD_MS) return; playback?.cancelScan(); };
  // Choosing a computer sets the Playback API host (saved by the scan) and, with it, the Library Link's address: the script is asked at
  // the Playback host unless Advanced names another address, and a name typed for the old computer would be wrong for the new one.
  const choose = (chosen: string) => {
    scanClickedAt.current = Date.now();
    setPressed("connect");
    playback?.scan(chosen);
    if (library?.address) void library.saveAddress("");
  };
  // Connect on a computer that is already connected only checks again, and the status line says so.
  const connect = () => {
    if (!trimmed || !hostOk) return;
    if (connectedNow && trimmed === savedHost) recheck.mark(); else recheck.clear();
    choose(trimmed);
  };
  const connectHint = !trimmed ? "Type an address to connect." : !hostOk ? "Enter a computer name or address like 192.0.2.10." : scanning ? "Wait for the search to finish, or cancel it." : undefined;

  const api = status ? withRemoteWords(apiLine(status, "Checking Playback…"), status, library?.status) : { tone: "idle" as const, text: playback?.error ?? "Checking Playback…" };
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
          <ScanProgress scan={scan} wording={PLAYBACK_SCAN_WORDS} />

          <ConnectionRow
            as="form"
            connectDisabled={!trimmed || !hostOk || scanning}
            connectHint={connectHint}
            connecting={connecting}
            id="playback-specific"
            onCancelScan={cancelScan}
            onConnect={connect}
            onScan={startScan}
            scanning={scanning}
            testId="playback-live-link-line"
            tone="playback"
          >
            <LiveLinkField caption="Host" className="live-link-host">
              <AddressBox aria-describedby={hostOk ? undefined : errorId} aria-label="Playback host" error={!hostOk} inputRef={hostField} onChange={(next) => { setHost(next); recheck.clear(); }} placeholder="For example, 192.0.2.10" scanned={scanned} scope="playback" value={host} working={connectedNow ? savedHost : null} />
            </LiveLinkField>
          </ConnectionRow>
          {!hostOk && <SectionNotice className="mt-2" id={errorId} role="alert" tone="danger" variant="line">Enter a computer name or address like 192.0.2.10 (no http://, port or spaces).</SectionNotice>}

          <ScanArea>
            <ScanHelp copied={copied} onCopy={copyDetails} onOther={() => hostField.current?.focus()} scan={scan} scanning={busy} tone="playback" wording={PLAYBACK_SCAN_WORDS} />
            <ScanChooser onChoose={(chosen) => choose(chosen)} scan={scan} tone="playback" wording={PLAYBACK_SCAN_WORDS} />
            {playback?.error && <p className="text-sm text-danger-text" role="alert">{playback.error}</p>}
            {playback?.message && <p className="text-sm text-integration-playback-text" role="status">{playback.message}</p>}
          </ScanArea>
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
          <LiveLinkPill detail={api.text} name="" testId="playback-api-pill" text={`${PLAYBACK_API_NAME}: ${parts.api.word}`} tone={parts.api.tone} />
          {library && <LiveLinkPill detail={script?.text} name="" testId="library-link-pill" text={`Library Link: ${parts.script.word}`} tone={parts.script.tone} />}
        </>
      )}
      status={[
        // Prefixed like the other blocks' lines. Playback's sentence is MIDI's own while MIDI stands in (`Connected through MIDI.`), and that one is
        // not the API's to say, so it keeps no prefix.
        { text: api.tone === "midi" ? api.text : `${PLAYBACK_API_NAME}: ${api.text}`, testId: "playback-banner" },
        ...(recheck.note ? [{ text: recheck.note, testId: "playback-recheck" }] : []),
        ...(script ? [{ text: `Library Link: ${script.text}.`, testId: "library-link-status", tone: script.tone }] : []),
      ]}
      testId="playback-live-link"
      title={TITLE}
    />
  );
}
