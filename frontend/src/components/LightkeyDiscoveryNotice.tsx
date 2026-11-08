import type { LightkeyDiscovery } from "../types";
import { CONTROL_34 } from "./compactControl";
import { Button } from "./ui/Button";

/** The words of the search, exactly as the operator reads them. */
const DISCOVERY_TEXT = {
  searching: "Looking for a Lighting computer on this network\u2026",
  none: "No Lighting computer found on this network.",
  several: (count: number) => `Found ${count} Lighting computers. Choose one:`,
  row: (address: string, running: boolean) => `${address} (Lightkey ${running ? "running" : "not running"})`,
  offer: (chosen: string, address: string) => `${chosen} isn't answering. A Lighting computer answered at ${address}. Use it?`,
  refusing: (address: string) => `A Lighting computer at ${address} is refusing StagePilot. Run the setup command on it again.`,
} as const;

const linkButton =
  "min-h-ds-touch-target touch-manipulation text-integration-lighting-text underline transition-colors duration-[140ms] motion-reduce:transition-none can-hover:text-integration-lighting active:text-integration-lighting-text";

/**
 * The strip's line about finding the Lighting computer. Nothing here is an error: no Lighting computer on the
 * network is an ordinary state (the setup box under it stays), and a found one is only ever switched to by
 * the person pressing a button (Automatic with exactly one found connects by itself and says so).
 */
export function LightkeyDiscoveryNotice({ discovery, busy, onUse, onKeepWaiting, onSearchAgain }: {
  discovery: LightkeyDiscovery | undefined;
  busy: boolean;
  onUse: (address: string) => void;
  onKeepWaiting: (address: string) => void;
  onSearchAgain: () => void;
}) {
  if (!discovery || discovery.state === "off") return null;
  const refusing = discovery.state === "connected" ? [] : discovery.refusing;
  const offered = discovery.found[0];
  return (
    <div aria-live="polite" className="mt-2 space-y-1 text-type-body-sm text-ink-muted" data-testid="lightkey-discovery" role="status">
      {discovery.state === "searching" && <p data-testid="lightkey-discovery-searching">{DISCOVERY_TEXT.searching}</p>}

      {discovery.state === "connected" && discovery.message && <p data-testid="lightkey-discovery-message">{discovery.message}</p>}
      {discovery.state === "connected" && discovery.note && <p data-testid="lightkey-discovery-note">{discovery.note}</p>}

      {discovery.state === "choose" && (
        <div data-testid="lightkey-discovery-choose">
          <p>{DISCOVERY_TEXT.several(discovery.found.length)}</p>
          <ul className="mt-1 space-y-1">
            {discovery.found.map((found) => (
              <li className="flex flex-wrap items-center gap-2" key={found.address}>
                <span className="font-mono text-ink-body" id={`lightkey-found-${found.address}`}>{DISCOVERY_TEXT.row(found.address, found.lightkey_running)}</span>
                <Button aria-describedby={`lightkey-found-${found.address}`} className={CONTROL_34} disabled={busy} onClick={() => onUse(found.address)} tone="lighting" variant="secondary">
                  Use this one
                </Button>
              </li>
            ))}
          </ul>
        </div>
      )}

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

      {discovery.state === "none" && (
        <p data-testid="lightkey-discovery-none">
          {DISCOVERY_TEXT.none}{" "}
          <button className={linkButton} disabled={busy} onClick={onSearchAgain} type="button">Search again</button>
        </p>
      )}

      {refusing.map((address) => (
        <p className="text-warning-text" data-testid="lightkey-discovery-refusing" key={address}>{DISCOVERY_TEXT.refusing(address)}</p>
      ))}
    </div>
  );
}
