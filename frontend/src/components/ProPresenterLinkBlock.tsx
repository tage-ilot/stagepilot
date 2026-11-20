import { useId, useState } from "react";

import type { ProPresenterLinkController } from "../hooks/useProPresenterLink";
import type { ProPresenterStatusResponse } from "../types";
import { helperUpdateLine, updatesItself } from "./lightkeyProject";
import { proPresenterBoxProps } from "./helperBoxProps";
import { AddressBox, ConnectionRow, ConnectionSection, HelperSetupBox, LiveLinkField, LiveLinkPill, ScanArea, ScanChooser, ScanHelp, ScanProgress, SectionNotice, scannedAddresses, type ScanState } from "./sections";
import { PROPRESENTER_SCAN_WORDS } from "./proPresenterScan";
import { useRecheckNote } from "./useRecheckNote";
import { CONTROL_40 } from "./liveLink";
import { PRESENTER_LINK_NAME, apiWords, importBlocked, linkLine, linkNote, linkPill, linkWords, proPresenterLine, proPresenterPill } from "./proPresenterLinkState";
import { Input } from "./ui/Input";

const TITLE = "ProPresenter Live Link";
const smallBox = "!w-[5.25rem] text-center tabular-nums";
/** What the API box is for (its caption says only API: this is the port of ProPresenter's network API, not a general port). */
export const API_TITLE = "The port of ProPresenter's network API";
export const API_LABEL = "ProPresenter API port";

/**
 * The connection the block holds, like the Lighting strip holds its Host and port: the panel owns the values and the save logic (a
 * held change is saved by the panel's Save bar, or by Connect, which saves the connection the way the panel does); the block only draws the boxes and the pair.
 */
export type ProPresenterConnectionFields = {
  host: string;
  port: string;
  onHost: (host: string) => void;
  onPort: (port: string) => void;
  /** Edits that differ from the saved values: marked in words and with the warning border, never by colour alone. */
  held: { host: boolean; port: boolean };
  /** Plain-words problems with what was typed (shown after a Save was tried). */
  errors: { host?: string; port?: string };
  /** A request is in flight: the boxes and the buttons wait. */
  busy: boolean;
  /** Connect: saves what the boxes hold and connects, as the panel's save does. When nothing is held and ProPresenter is saved, it only checks again. */
  onConnect: () => void;
  connecting: boolean;
  connectDisabled: boolean;
  /** Why Connect is switched off, as a hint on the button. */
  connectHint?: string;
  /** Connect would only check the saved connection again (nothing held, ProPresenter saved): the status line says so after it. */
  checksAgain: boolean;
  /** Scan: look for ProPresenter's API on the network (a hook beside `useProPresenterLink`); the panel puts the chosen computer in the boxes. */
  scan: { state: ScanState; start: () => void; cancel: () => void; choose: (host: string, port?: number) => void; error?: string | null; starting?: boolean };
};

function HeldMarker({ id }: { id: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-type-body-sm font-normal normal-case tracking-normal text-warning-text" data-held-marker id={id}>
      <span aria-hidden="true" className="h-2 w-2 rounded-ds-full bg-warning" />
      Not saved yet
    </span>
  );
}

/**
 * The ProPresenter Live Link block of the Presentation panel, built like the Playback and Lighting Live Link blocks: the FIRST block under
 * the header, a bordered card with a title, a state pill beside it, and two status lines (ProPresenter API, and the optional Presenter
 * Link). It holds the CONNECTION: the Host and API boxes (with the recent addresses) and the Search and Connect pair, under the two lines and above the
 * setup box. Timer, Look, Refresh and Save stay in the panel, after the block.
 *
 * The Presenter Link is how StagePilot reaches the song files and the screen of the ProPresenter computer; it adds Playback's songs to a
 * playlist (the Presenter Link section's Import songs to ProPresenter, in this panel). When ProPresenter runs on THIS computer there is nothing to set
 * up: the line says "on this computer", there is no setup button, and an import blocker is shown in plain words with its one-sentence
 * fix. Another computer gets the button **Set up the ProPresenter computer** (one command to paste into Terminal there).
 *
 * The Presenter Link is optional. Without it nothing here is a warning: the line says "Not installed" in grey, and nothing anywhere else asks for it.
 * The timer and Look settings are not part of this block and are never changed by it. Left out, `link` draws no Presenter Link line, pill or
 * setup; left out, `connection` draws no boxes.
 */
export function ProPresenterLinkBlock({ propresenter, link, connection }: { propresenter: ProPresenterStatusResponse | null; link?: ProPresenterLinkController; connection?: ProPresenterConnectionFields }) {
  const heldHostId = useId();
  const heldPortId = useId();
  const errorHostId = useId();
  const errorPortId = useId();
  const status = link?.status ?? null;
  const hasComputer = Boolean(propresenter?.enabled && propresenter.host);
  const local = Boolean(link) && status?.mode === "local";
  const helper = Boolean(link) && status?.mode === "helper";
  const api = proPresenterLine(propresenter);
  const line = linkLine(status, hasComputer);
  const pillApi = proPresenterPill(propresenter);
  const pillLink = linkPill(status, hasComputer);
  const state = status?.state ?? null;
  // A Link that is behind and can update itself is updated by StagePilot: its setup box stays closed, with only a line to read.
  const updatesBySelf = state === "outdated" && updatesItself(status?.update);
  const needsSetup = helper && state !== "ready" && state !== null && state !== "checking" && !updatesBySelf;
  // The Details button: "Presenter Link setup", or what is to be done when the Link is behind and StagePilot cannot update it.
  const detailsLabel = state === "outdated" && !updatesBySelf ? `Update the ${PRESENTER_LINK_NAME}` : `${PRESENTER_LINK_NAME} setup`;
  const updateLine = helperUpdateLine(status?.update);
  const note = link ? linkNote(status) : null;

  const fields = connection;
  const recheck = useRecheckNote();
  const [copied, setCopied] = useState(false);
  const copyDetails = async (text: string) => {
    try { await navigator.clipboard.writeText(text); setCopied(true); } catch { setCopied(false); }
  };
  // The scan's own request can take a moment before the backend says "scanning": the button already reads Scanning… (cancel).
  const scanState = fields?.scan.state;
  const scanning = scanState?.state === "scanning" || Boolean(fields?.scan.starting);
  const scanned = scanState ? scannedAddresses(scanState) : [];
  const connectNow = () => {
    if (!fields || fields.connectDisabled || fields.busy || fields.connecting) return;
    if (fields.checksAgain) recheck.mark(); else recheck.clear();
    fields.onConnect();
  };
  const hostMarker = fields?.held.host ? <HeldMarker id={heldHostId} /> : undefined;
  const portMarker = fields?.held.port ? <HeldMarker id={heldPortId} /> : undefined;

  const hasDetails = Boolean(link) && (helper || local || Boolean(note) || Boolean(updateLine));

  return (
    <ConnectionSection
      attention={needsSetup || (Boolean(note) && !importBlocked(status))}
      className="!mt-0"
      connection={fields && (
        <>
          <ConnectionRow
            connectDisabled={fields.connectDisabled || fields.busy}
            connectHint={fields.connectHint}
            connecting={fields.connecting}
            onCancelScan={fields.scan.cancel}
            onConnect={connectNow}
            onScan={() => { recheck.clear(); fields.scan.start(); }}
            scanDisabled={fields.busy}
            scanHint={fields.busy ? "Wait for the saving to finish." : undefined}
            scanning={scanning}
            testId="propresenter-live-link-line"
            tone="presentation"
          >
            <LiveLinkField caption="Host" className="live-link-host" marker={hostMarker}>
              <AddressBox
                aria-describedby={[hostMarker ? heldHostId : null, fields.errors.host ? errorHostId : null].filter(Boolean).join(" ") || undefined}
                aria-label="Host"
                disabled={fields.busy}
                error={Boolean(fields.errors.host)}
                held={fields.held.host}
                onChange={fields.onHost}
                // A computer chosen from the Scanned group brings the port it answered on, as the chooser does.
                onPick={(picked) => { if (picked.port !== undefined) fields.onPort(String(picked.port)); }}
                onSubmit={connectNow}
                scanned={scanned}
                scope="presentation"
                value={fields.host}
              />
            </LiveLinkField>
            <LiveLinkField caption="API" marker={portMarker}>
              <Input
                aria-describedby={[portMarker ? heldPortId : null, fields.errors.port ? errorPortId : null].filter(Boolean).join(" ") || undefined}
                aria-label={API_LABEL}
                className={`${smallBox} font-mono ${CONTROL_40} ${fields.held.port ? "!border-warning" : ""}`}
                disabled={fields.busy}
                error={Boolean(fields.errors.port)}
                inputMode="numeric"
                onChange={(event) => fields.onPort(event.target.value)}
                title={API_TITLE}
                value={fields.port}
              />
            </LiveLinkField>
          </ConnectionRow>
          {fields.errors.host && <SectionNotice className="mt-1" id={errorHostId} role="alert" size="xs" tone="danger" variant="line">{fields.errors.host}</SectionNotice>}
          {fields.errors.port && <SectionNotice className="mt-1" id={errorPortId} role="alert" size="xs" tone="danger" variant="line">{fields.errors.port}</SectionNotice>}
          <ScanProgress scan={fields.scan.state} wording={PROPRESENTER_SCAN_WORDS} />
          <ScanArea>
            <ScanHelp copied={copied} onCopy={copyDetails} scan={fields.scan.state} scanning={scanning} tone="presentation" wording={PROPRESENTER_SCAN_WORDS} />
            {scanState?.state === "found" && scanState.candidates.length === 1 && scanState.candidates[0] && (
              <p className="text-sm text-integration-presentation-text" data-testid="propresenter-scan-found" role="status">
                {`Found ${scanState.candidates[0].label} at ${scanState.candidates[0].host}:${scanState.candidates[0].port ?? ""}. It is in the Host and API boxes: press Connect to use it.`}
              </p>
            )}
            <ScanChooser onChoose={fields.scan.choose} scan={fields.scan.state} tone="presentation" wording={PROPRESENTER_SCAN_WORDS} />
            {fields.scan.error && <p className="text-sm text-danger-text" role="alert">{fields.scan.error}</p>}
          </ScanArea>
        </>
      )}
      details={hasDetails && link ? ({ byChoice }) => (
        <>
          {local && <SectionNotice className="mt-2" quiet size="xs" testId="propresenter-link-local" tone="info" variant="line">ProPresenter runs on this computer, so there is nothing to set up. StagePilot uses its folder directly.</SectionNotice>}
          {note && <SectionNotice className="mt-2" role="status" size="xs" testId="propresenter-link-note" tone={importBlocked(status) ? "info" : "warning"} variant="line">{note}</SectionNotice>}
          {updateLine && <SectionNotice className="mt-2" quiet role="status" size="xs" testId="propresenter-link-update" tone="info" variant="line">{updateLine}</SectionNotice>}
          {helper && <HelperSetupBox {...proPresenterBoxProps(link, hasComputer)} autoStart={byChoice} />}
        </>
      ) : undefined}
      detailsLabel={detailsLabel}
      footer={link?.error ? <SectionNotice className="mt-2" role="alert" tone="danger" variant="line">{link.error}</SectionNotice> : undefined}
      pills={(
        <>
          <LiveLinkPill detail={api.text} name="" testId="propresenter-api-pill" text={apiWords(pillApi.word)} tone={pillApi.tone} />
          {link && <LiveLinkPill detail={line.text} name="" testId="propresenter-link-pill" text={linkWords(pillLink.word)} tone={pillLink.tone} />}
        </>
      )}
      status={[
        { text: apiWords(`${api.text}.`), testId: "propresenter-banner", tone: api.tone },
        ...(recheck.note ? [{ text: recheck.note, testId: "propresenter-recheck" }] : []),
        ...(link ? [{ text: linkWords(`${line.text}.`), testId: "propresenter-link-status", tone: line.tone }] : []),
      ]}
      testId="propresenter-live-link"
      testIdPrefix="propresenter-live-link"
      title={TITLE}
    />
  );
}
