import { openExternalUrl } from "../../desktop";
import type { Tone } from "../ui/types";
import { InlineAlert } from "../ui/InlineAlert";
import { scanSummary, type ScanState } from "./scanState";
import type { ScanWording } from "./ScanProgress";

const LOCAL_NETWORK_URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";

const LINK = "min-h-ds-touch-target touch-manipulation underline transition-colors duration-[140ms] motion-reduce:transition-none";
// Written out in full so Tailwind can see them.
const LINK_TONE: Partial<Record<Tone, string>> = {
  playback: "text-integration-playback-text can-hover:text-integration-playback active:text-integration-playback-text",
  lighting: "text-integration-lighting-text can-hover:text-integration-lighting active:text-integration-lighting-text",
  presentation: "text-integration-presentation-text can-hover:text-integration-presentation active:text-integration-presentation-text",
};

/**
 * What a block says under its connection row after a scan that found nothing: macOS blocking the local network (with the steps and the page
 * to open), another failure in one sentence, or the plain "make sure" checklist. It is part of the connection, so it stays in view (a macOS
 * block cannot sit behind a collapsed button). Not drawn while scanning, or when computers were found.
 */
export function ScanHelp({ scan, wording, tone = "info", scanning = false, copied = false, onCopy, onOther, testId = "scan-error" }: {
  scan: ScanState;
  wording: ScanWording;
  tone?: Tone;
  scanning?: boolean;
  copied?: boolean;
  onCopy?: (text: string) => void;
  /** "Enter address instead": the person types the computer. Left out where the box is always there. */
  onOther?: () => void;
  testId?: string;
}) {
  if (scan.state !== "not_found" || scanning) return null;
  const link = `${LINK} ${LINK_TONE[tone] ?? "text-info-text"}`;
  const settings = scan.settingsUrl ?? LOCAL_NETWORK_URL;
  const actions = (withSettings: boolean) => (
    <div className="mt-2 flex flex-wrap gap-3">
      {withSettings && <button className={link} onClick={() => void openExternalUrl(settings)} type="button">Open Local Network settings</button>}
      {scan.details && onCopy && <button className={link} onClick={() => void onCopy(scan.details as string)} type="button">{copied ? "Copied" : "Copy diagnostic details"}</button>}
      {!scan.typed && onOther && <button className={link} onClick={onOther} type="button">{wording.other ?? "Enter address instead"}</button>}
    </div>
  );

  if (scan.error === "permission_denied") {
    return (
      <div data-part="scan-help">
        <InlineAlert className="p-3 text-ink-body" data-testid={testId} tone="danger">
          <p className="font-semibold">macOS is blocking StagePilot&apos;s local-network access</p>
          <ol className="mt-2 list-decimal space-y-1 pl-5">
            <li>Quit StagePilot.</li>
            <li>Open System Settings &gt; Privacy &amp; Security &gt; Local Network.</li>
            <li>Turn StagePilot off and on again (if it is not listed, press Scan once and click Allow when macOS asks).</li>
            <li>Reopen StagePilot and scan again.</li>
          </ol>
          {actions(true)}
        </InlineAlert>
      </div>
    );
  }
  if (scan.error) {
    return (
      <div data-part="scan-help">
        <InlineAlert className="p-3 text-ink-body" data-testid={testId} tone="danger">
          <p className="font-semibold">{scan.typed ? "Couldn't connect" : "Couldn't scan the network"}</p>
          <p className="mt-1">{scan.reason}</p>
          <p className="mt-1 text-xs text-ink-muted">{scanSummary(scan)}</p>
          {actions(Boolean(scan.settingsUrl))}
        </InlineAlert>
      </div>
    );
  }
  // A search with nothing to add (no checklist, nothing counted) is one quiet line, not a box.
  if (wording.notFound && wording.checklist.length === 0 && !scan.hostsTotal) {
    return <p data-part="scan-help" data-testid={`${testId}-none`}>{wording.notFound}</p>;
  }
  return (
    <div className="rounded-ds-lg bg-edge-subtle p-3 text-sm text-ink-body" data-part="scan-help" data-testid={`${testId}-none`}>
      {wording.notFound && <p className="font-semibold">{wording.notFound}</p>}
      {wording.checklist.length > 0 && (
        <>
          <p className={wording.notFound ? "mt-1" : "font-semibold"}>Make sure:</p>
          <ul className="mt-1 list-disc pl-5 text-ink-body">{wording.checklist.map((line) => <li key={line}>{line}</li>)}</ul>
        </>
      )}
      {scan.hostsTotal ? <p className="mt-1 text-xs tabular-nums text-ink-muted">{scanSummary(scan)}</p> : null}
      {!scan.typed && onOther && <button className={`mt-2 ${link}`} onClick={onOther} type="button">{wording.other ?? "Enter address instead"}</button>}
    </div>
  );
}
