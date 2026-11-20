import type { LightkeyDiscovery } from "../types";
import { CONTROL_34 } from "./compactControl";
import { lightkeyScanState, LIGHTKEY_SCAN_WORDS } from "./lightkeyScan";
import { ScanChooser, ScanHelp, ScanProgress } from "./sections";
import { Button } from "./ui/Button";

/** The words of the one case that is not a scan result, exactly as the operator reads them. */
const DISCOVERY_TEXT = {
  offer: (chosen: string, address: string) => `${chosen} isn't answering. A Lighting computer answered at ${address}. Use it?`,
  refusing: (address: string) => `A Lighting computer at ${address} is refusing StagePilot. Run the setup command on it again.`,
} as const;

/**
 * The strip's line about finding the Lighting computer. Nothing here is an error: no Lighting computer on the
 * network is an ordinary state (the setup box under it stays), and a found one is only ever switched to by
 * the person pressing a button (Automatic with exactly one found connects by itself and says so).
 *
 * The search itself (looking, several found, none found) is drawn by the same scan templates as the Playback and ProPresenter
 * scans; the **Scan** button of the connection row starts it now, so there is no button of its own here.
 */
export function LightkeyDiscoveryNotice({ discovery, busy, stopped = false, onChoose, onUse, onKeepWaiting }: {
  discovery: LightkeyDiscovery | undefined;
  busy: boolean;
  /** The person stopped watching the search (Scanning… (cancel)). */
  stopped?: boolean;
  /** One of several computers was chosen: it goes in the Host box; Connect connects. */
  onChoose: (address: string) => void;
  onUse: (address: string) => void;
  onKeepWaiting: (address: string) => void;
}) {
  if (!discovery || discovery.state === "off") return null;
  const refusing = discovery.state === "connected" ? [] : discovery.refusing;
  const offered = discovery.found[0];
  const scan = lightkeyScanState(discovery, stopped);
  return (
    <div aria-live="polite" className="mt-2 space-y-1 text-type-body-sm text-ink-muted" data-testid="lightkey-discovery" role="status">
      <ScanProgress className="" scan={scan} testId="lightkey-discovery-searching" wording={LIGHTKEY_SCAN_WORDS} />

      {discovery.state === "connected" && discovery.message && <p data-testid="lightkey-discovery-message">{discovery.message}</p>}
      {discovery.state === "connected" && discovery.note && <p data-testid="lightkey-discovery-note">{discovery.note}</p>}

      <ScanChooser onChoose={(address) => onChoose(address)} scan={scan} testId="lightkey-discovery-choose" tone="lighting" wording={LIGHTKEY_SCAN_WORDS} disabled={busy} />

      {discovery.state === "offer" && offered && (
        <div className="flex flex-wrap items-center gap-2" data-testid="lightkey-discovery-offer">
          <p className="min-w-0 flex-[1_1_16rem]">{DISCOVERY_TEXT.offer(discovery.chosen ?? "The chosen address", offered.address)}</p>
          <Button className={CONTROL_34} disabled={busy} onClick={() => onUse(offered.address)} tone="lighting" variant="secondary">
            {`Use ${offered.address}`}
          </Button>
          <Button className={CONTROL_34} disabled={busy} onClick={() => onKeepWaiting(offered.address)} variant="secondary">
            Keep waiting
          </Button>
        </div>
      )}

      <ScanHelp scan={scan} testId="lightkey-discovery" tone="lighting" wording={LIGHTKEY_SCAN_WORDS} />

      {refusing.map((address) => (
        <p className="text-warning-text" data-testid="lightkey-discovery-refusing" key={address}>{DISCOVERY_TEXT.refusing(address)}</p>
      ))}
    </div>
  );
}
