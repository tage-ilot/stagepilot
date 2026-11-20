import { useCallback, useEffect, useRef, useState } from "react";

import { getProPresenterLinkStatus } from "../api";
import { describeError } from "../errorText";
import type { LinkStatus } from "../propresenterLink";
import { PRESENTER_LINK_NAME } from "../components/proPresenterLinkState";

/** How often an open window asks again. The backend asks the ProPresenter computer at most every 30 seconds however many windows there are. */
export const LINK_REFRESH_MS = 30_000;
/** While a setup box waits for the Presenter Link to answer, the window looks this often. */
export const LINK_WATCH_MS = 3_000;

export interface ProPresenterLinkController {
  status: LinkStatus | null;
  error: string | null;
  pending: "check" | null;
  /** The Check button: reads the Link now. */
  check: () => void;
  /** A setup box is waiting for the Link: look every few seconds instead of every 30. */
  watch: (on: boolean) => void;
}

/**
 * The optional Presenter Link (`GET propresenter-link/status`). It asks only while a ProPresenter computer is saved (`enabled`) and
 * a window that uses the answer is on screen (`wanted`), and only while the page is visible. Read-only; a Link that is not there is a
 * quiet state (the backend answers `not_installed`), never an error. The answer never feeds the header's verdict.
 */
export function useProPresenterLink(enabled: boolean, wanted: boolean): ProPresenterLinkController {
  const active = enabled && wanted;
  const [status, setStatus] = useState<LinkStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<"check" | null>(null);
  const [watching, setWatching] = useState(false);
  const generation = useRef(0);

  const read = useCallback(async (check: boolean, current: number) => {
    try {
      const next = await getProPresenterLinkStatus(check);
      if (generation.current !== current) return;
      setStatus(next);
      setError(null);
    } catch (cause) {
      if (generation.current === current) setError(describeError(cause, `The ${PRESENTER_LINK_NAME} could not be read.`));
    }
  }, []);

  // Another computer, or none: what was held belongs to the old one.
  useEffect(() => {
    if (!enabled) { setStatus(null); setError(null); }
  }, [enabled]);

  useEffect(() => {
    const current = ++generation.current;
    if (!active) return undefined;
    const gap = watching ? LINK_WATCH_MS : LINK_REFRESH_MS;
    let timer: number | undefined;
    const tick = async () => {
      if (typeof document === "undefined" || document.visibilityState !== "hidden") await read(false, current);
      if (generation.current === current) timer = window.setTimeout(() => void tick(), gap);
    };
    void tick();
    return () => {
      generation.current = current + 1;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [active, watching, read]);

  const check = useCallback(() => {
    if (!active) return;
    setPending("check");
    void read(true, generation.current).finally(() => setPending(null));
  }, [active, read]);

  const watch = useCallback((on: boolean) => setWatching(on), []);

  return { status, error, pending, check, watch };
}
