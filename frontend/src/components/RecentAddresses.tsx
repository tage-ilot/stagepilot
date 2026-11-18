import { useEffect } from "react";

import { rememberAddress, useRecentAddresses, type RecentScope } from "../hooks/useRecentAddresses";

/**
 * "Recent: 10.30.0.211  192.168.4.20": the last few addresses that worked, one tap each, so nobody has to scan or retype. Draws nothing
 * until there is one that is not already in the box. `working` is the address that is connected right now; it is remembered.
 */
export function RecentAddresses({ scope, value, working, onPick, disabled = false, className = "" }: {
  scope: RecentScope;
  value: string;
  working?: string | null;
  onPick: (address: string) => void;
  disabled?: boolean;
  className?: string;
}) {
  const recent = useRecentAddresses(scope);
  useEffect(() => { rememberAddress(scope, working); }, [scope, working]);
  const shown = recent.filter((address) => address.toLowerCase() !== value.trim().toLowerCase());
  if (!shown.length) return null;
  return (
    <div aria-label="Recent addresses" className={`flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-ink-muted ${className}`} data-testid={`recent-${scope}`} role="group">
      <span>Recent:</span>
      {shown.map((address) => (
        <button
          aria-label={`Use ${address}`}
          className="min-h-8 touch-manipulation rounded-ds-lg border border-edge-medium bg-surface-panel px-2 font-mono text-ink-body can-hover:border-ink-muted can-hover:text-ink disabled:opacity-50"
          disabled={disabled}
          key={address}
          onClick={() => onPick(address)}
          type="button"
        >
          {address}
        </button>
      ))}
    </div>
  );
}
