import { useEffect, useRef, useState } from "react";
import { getRemoteStatus } from "../api";

/** How long Remote Access may stay not-connected before the dashboard says so. The tunnel retries on its own with a backoff that
 * tops out at 30 s, and it needs a few seconds after every launch, so a shorter wait would flag normal start-ups and blips. */
export const REMOTE_OFFLINE_GRACE_MS = 120_000;
const POLL_MS = 5_000;

/**
 * True when the user has turned Remote Access on and it has been unreachable, through the tunnel's own retries, for
 * REMOTE_OFFLINE_GRACE_MS. Never true while Remote Access is off, while it is connected, or on a failed status request (an
 * unreachable StagePilot is reported by its own "StagePilot disconnected" check). Only the local operator needs this, so pass
 * `active = false` for Viewers and remote sessions: nothing is requested then.
 */
export function useRemoteOffline(active: boolean): boolean {
  const [offline, setOffline] = useState(false);
  const downSince = useRef<number | null>(null);

  useEffect(() => {
    downSince.current = null;
    setOffline(false);
    if (!active) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const status = await getRemoteStatus();
        if (cancelled) return;
        if (status.enabled && status.state !== "connected") {
          downSince.current ??= Date.now();
          setOffline(Date.now() - downSince.current >= REMOTE_OFFLINE_GRACE_MS);
        } else {
          downSince.current = null;
          setOffline(false);
        }
      } catch {
        // Keep the last answer: a failed request says nothing about the tunnel.
      }
      if (!cancelled) timer = setTimeout(() => void poll(), POLL_MS);
    };
    void poll();
    return () => { cancelled = true; clearTimeout(timer); };
  }, [active]);

  return offline;
}
