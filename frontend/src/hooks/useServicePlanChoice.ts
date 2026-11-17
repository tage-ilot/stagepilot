import { useCallback, useEffect, useRef, useState } from "react";

import { pinServicePlan, unpinServicePlan } from "../api";
import { formatMonthDay, pickerErrorText } from "../lib/servicePlans";
import type { ApplicationState } from "../types";

export interface ServicePlanChoice {
  /** A change is loading: the controls wait. */
  pending: boolean;
  /** Why the last change did not happen, in words for the operator. */
  error: string | null;
  /** What a screen reader hears after a change ("Loaded Sunday Service, Nov 29"); empty before the first change. */
  announcement: string;
  /** Load this service and keep it until the operator changes it (picking the upcoming one is going back). */
  pick: (planId: string) => void;
  /** Back to the upcoming service. */
  back: () => void;
}

/**
 * Picking a service other than the upcoming one. Choices never overlap: while one is loading, a newer choice
 * waits (and replaces any choice that was waiting), so the LAST choice wins and Planning Center is asked once at a time.
 */
export function useServicePlanChoice(canOperate: boolean, applyState: (state: ApplicationState) => void): ServicePlanChoice {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const wanted = useRef<{ planId: string | null } | null>(null);
  /** What the request on its way is for (`null` inside means Back to upcoming); nothing when no request is on its way. */
  const inFlight = useRef<{ planId: string | null } | null>(null);
  const running = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  const run = useCallback(async () => {
    if (running.current) return;
    running.current = true;
    if (mounted.current) setPending(true);
    try {
      while (wanted.current) {
        const { planId } = wanted.current;
        inFlight.current = { planId };
        wanted.current = null;
        try {
          const response = await (planId === null ? unpinServicePlan() : pinServicePlan(planId));
          applyState(response.state);
          if (!mounted.current) continue;
          setError(null);
          const plan = response.state.plan;
          setAnnouncement(plan ? `Loaded ${plan.title}, ${formatMonthDay(plan.date)}` : "Back to the upcoming service");
        } catch (cause) {
          if (mounted.current) {
            setError(pickerErrorText(cause, "The service could not be changed. Try again."));
            setAnnouncement("");
          }
        }
      }
    } finally {
      inFlight.current = null;
      running.current = false;
      if (mounted.current) setPending(false);
    }
  }, [applyState]);

  const choose = useCallback((planId: string | null) => {
    if (!canOperate) return;
    if (running.current && inFlight.current?.planId === planId) {
      // The same choice again (a double click): what is loading is already what is wanted, so nothing waits behind it.
      wanted.current = null;
      return;
    }
    wanted.current = { planId };
    if (mounted.current) setError(null);
    void run();
  }, [canOperate, run]);

  const pick = useCallback((planId: string) => choose(planId), [choose]);
  const back = useCallback(() => choose(null), [choose]);
  return { pending, error, announcement, pick, back };
}
