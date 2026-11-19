import { Button } from "../ui/Button";
import type { Tone } from "../ui/types";
import { CONTROL_40 } from "../liveLink";

/**
 * The words of the pair. Every Live Link block says exactly these, drawn exactly alike (tinted, in the connection's tone; there is no prop for a page-specific look); nothing else is ever drawn as a Search or Connect button. **Search**
 * finds a computer on the network; the word Scan is kept for reading the songs (the Playback config's Scan Playback API / Library).
 * The identifiers stay `scan` (routes, props, test ids).
 */
export const SCAN_WORDS = { scan: "Search", scanning: "Searching\u2026 (cancel)", connect: "Connect", connecting: "Connecting\u2026" } as const;

export interface ScanConnectProps {
  /** The connection's accent. */
  tone?: Tone;
  /** Looks for the computer on the network. Without it the button is drawn, switched off. */
  onScan?: () => void;
  /** A search is running: the Search button reads "Searching… (cancel)" and this stops it. */
  onCancelScan?: () => void;
  /** Connects to what the fields hold (a block that is already connected checks again). Without it the button is drawn, switched off. */
  onConnect?: () => void;
  scanning?: boolean;
  connecting?: boolean;
  scanDisabled?: boolean;
  connectDisabled?: boolean;
  /** Why Search / Connect is switched off, as a hint on the button (a disabled button shows no tooltip of its own, so it sits on the wrapper). */
  scanHint?: string;
  connectHint?: string;
  /** The Connect button submits the row when the row is a form (Enter in the address field presses Connect). */
  submit?: boolean;
}

const SIZE = `flex-none ${CONTROL_40}`;

/** The two buttons that end every connection row: **Search**, then **Connect**. Same size, same order, same place in every block. */
export function ScanConnectButtons({
  tone, onScan, onCancelScan, onConnect, scanning = false, connecting = false, scanDisabled = false, connectDisabled = false, scanHint, connectHint, submit = false,
}: ScanConnectProps) {
  const scanOff = !scanning && (scanDisabled || connecting || !onScan);
  const connectOff = connectDisabled || !onConnect;
  const scanTitle = scanOff ? scanHint : undefined;
  const connectTitle = connectOff ? connectHint : undefined;
  return (
    <>
      {/* A disabled button shows no tooltip of its own, so the reason sits on the wrapper. */}
      <span className="inline-flex" data-part="scan-wrap" title={scanTitle}>
        <Button
          className={SIZE}
          data-part="scan"
          disabled={scanOff}
          onClick={scanning ? onCancelScan : onScan}
          tone={tone}
          type="button"
          variant="tinted"
        >
          {scanning ? SCAN_WORDS.scanning : SCAN_WORDS.scan}
        </Button>
      </span>
      <span className="inline-flex" data-part="connect-wrap" title={connectTitle}>
        {/* While it connects the button is busy (the sweep, aria-busy, no second press), not dimmed: nothing else in the row says it is working. */}
        <Button
          className={`live-link-connect ${SIZE}`}
          data-part="connect"
          disabled={connectOff}
          loading={connecting}
          loadingLabel={SCAN_WORDS.connecting}
          onClick={submit ? undefined : onConnect}
          tone={tone}
          type={submit ? "submit" : "button"}
          variant="tinted"
        >
          {SCAN_WORDS.connect}
        </Button>
      </span>
    </>
  );
}
