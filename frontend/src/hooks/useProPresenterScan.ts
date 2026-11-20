import { useCallback, useEffect, useRef, useState } from "react";

import { cancelProPresenterScan, getProPresenterScan, startProPresenterScan } from "../api";
import { proPresenterScanState } from "../components/proPresenterScan";
import type { ScanState } from "../components/sections";
import { describeError } from "../errorText";
import type { ProPresenterScan } from "../propresenterScan";

/** How often a running scan is asked how far it is. */
export const SCAN_POLL_MS = 750;

export interface ProPresenterScanController {
  /** The scan in the one shape every block's scan uses. */
  scan: ScanState;
  error: string | null;
  /** Scan was pressed and the backend has not answered yet. */
  starting: boolean;
  start: () => void;
  cancel: () => void;
}

/**
 * The ProPresenter network scan (`POST /api/v1/propresenter/scan`, then `GET` while it runs). It asks nothing until Scan is pressed. The
 * scan saves nothing: the block puts the chosen computer in its own boxes and Connect saves, as the panel's save does.
 */
export function useProPresenterScan(): ProPresenterScanController {
  const [raw, setRaw] = useState<ProPresenterScan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const generation = useRef(0);
  const timer = useRef<number | undefined>(undefined);

  const stopPolling = useCallback(() => { window.clearTimeout(timer.current); timer.current = undefined; }, []);
  useEffect(() => () => { generation.current += 1; window.clearTimeout(timer.current); }, []);

  const poll = useCallback((current: number) => {
    stopPolling();
    timer.current = window.setTimeout(async () => {
      try {
        const next = await getProPresenterScan();
        if (generation.current !== current) return;
        setRaw(next);
        setError(null);
        if (next.state === "scanning") poll(current);
      } catch (cause) {
        if (generation.current !== current) return;
        setError(describeError(cause, "The search could not be read."));
      }
    }, SCAN_POLL_MS);
  }, [stopPolling]);

  const start = useCallback(() => {
    const current = ++generation.current;
    stopPolling();
    setError(null);
    setStarting(true);
    startProPresenterScan()
      .then((next) => {
        if (generation.current !== current) return;
        setRaw(next);
        if (next.state === "scanning") poll(current);
      })
      .catch((cause) => { if (generation.current === current) setError(describeError(cause, "The search could not start.")); })
      .finally(() => { if (generation.current === current) setStarting(false); });
  }, [poll, stopPolling]);

  const cancel = useCallback(() => {
    const current = ++generation.current;
    stopPolling();
    setStarting(false);
    cancelProPresenterScan()
      .then((next) => { if (generation.current === current) setRaw(next); })
      .catch((cause) => { if (generation.current === current) setError(describeError(cause, "Could not cancel the search.")); });
  }, [stopPolling]);

  return { scan: proPresenterScanState(raw), error, starting, start, cancel };
}
