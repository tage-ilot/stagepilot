import { useCallback, useEffect, useId, useRef, useState } from "react";

import { getUpcomingServicePlans } from "../api";
import type { ServicePlanChoice } from "../hooks/useServicePlanChoice";
import { formatLongDate, pickerErrorText, todayIn } from "../lib/servicePlans";
import type { ApplicationState, UpcomingServicePlans } from "../types";
import { NotUpcomingPill } from "./NotUpcomingPill";
import { ServicePlanCalendar } from "./ServicePlanCalendar";
import { Badge } from "./ui/Badge";
import { Button } from "./ui/Button";
import { ReasonButton } from "./sections";
import { InlineAlert } from "./ui/InlineAlert";

/**
 * Which service StagePilot has loaded, and a way to load another one: `‹ Sun, Nov 22, 2026 – Sunday Service ›` and a
 * calendar. The arrows step to the previous and next service (not day) among the services from today on; the left arrow
 * rests on the upcoming service. Picking any other service keeps it until it is changed, goes by, or is gone from
 * Planning Center (the backend decides; this only asks and shows). The calendar also reaches the services of the past 20
 * days, to review one; the arrows never go there, and the automatic return to the upcoming service stays.
 */
export function ServicePlanPicker({
  state,
  timezone,
  canOperate,
  choice,
  loadPlans = getUpcomingServicePlans,
}: {
  state: ApplicationState;
  /** The configured Planning Center time zone: it decides what today is. */
  timezone: string;
  canOperate: boolean;
  choice: ServicePlanChoice;
  loadPlans?: () => Promise<UpcomingServicePlans>;
}) {
  const headingId = useId();
  const calendarId = useId();
  const calendarButton = useRef<HTMLButtonElement>(null);
  const [list, setList] = useState<UpcomingServicePlans | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [loadingList, setLoadingList] = useState(true);
  const [attempt, setAttempt] = useState(0);
  const [open, setOpen] = useState(false);

  // The list is asked for again whenever another service gets loaded or the pick changes.
  const loaded = state.plan?.id ?? "";
  const pinned = Boolean(state.selection?.pinned);
  const loadStatus = state.service_load.status;
  useEffect(() => {
    let cancelled = false;
    setLoadingList(true);
    loadPlans()
      .then((answer) => {
        if (cancelled) return;
        setList(answer);
        setListError(null);
      })
      .catch((cause) => {
        if (!cancelled) setListError(pickerErrorText(cause, "The list of services could not be loaded."));
      })
      .finally(() => {
        if (!cancelled) setLoadingList(false);
      });
    return () => { cancelled = true; };
  }, [loadPlans, loaded, pinned, loadStatus, attempt]);

  const today = todayIn(timezone);
  const plans = list?.plans ?? [];
  // The arrows step only among the services from today on; the calendar alone reaches the days gone by.
  const stepping = plans.filter((candidate) => candidate.date >= today);
  const plan = state.plan;
  const index = plan ? stepping.findIndex((candidate) => candidate.id === plan.id) : -1;
  const reviewing = Boolean(plan) && plan !== null && plan.date < today && plans.some((candidate) => candidate.id === plan.id);
  const upcomingId = list?.upcoming_id ?? state.upcoming?.plan_id ?? null;
  const isUpcoming = Boolean(plan) && plan?.id === upcomingId && !pinned;
  const busy = choice.pending || !canOperate;
  const previous = index > 0 && stepping[index]?.id !== upcomingId ? (stepping[index - 1] ?? null) : null;
  // From a service that has gone by, forward goes to the first service from today on.
  const next = reviewing ? (stepping[0] ?? null) : index >= 0 ? (stepping[index + 1] ?? null) : null;

  const closeCalendar = useCallback(() => {
    setOpen(false);
    calendarButton.current?.focus();
  }, []);

  return (
    <section aria-busy={choice.pending || undefined} aria-labelledby={headingId} className="service-plan-picker">
      <h3 className="sr-only" id={headingId}>Service date</h3>
      <div className="relative flex flex-wrap items-center gap-2">
        <div className="flex min-w-0 max-w-full items-center gap-1 rounded-ds-lg border border-edge-medium bg-surface-900/60 p-1">
          <ReasonButton
            aria-label="Previous service"
            className="text-xl leading-none"
            disabled={busy}
            onClick={() => previous && choice.pick(previous.id)}
            reason={previous ? undefined : "This is the first service."}
            size="touch"
          >
            ‹
          </ReasonButton>
          <p className="min-w-0 break-words px-2 text-ink [overflow-wrap:anywhere]" data-testid="picker-selected">
            {plan ? (
              <>
                <span className="font-semibold tabular-nums">{formatLongDate(plan.date)}</span>
                <span aria-hidden="true"> – </span>
                <span className="sr-only">, </span>
                <span>{plan.title}</span>
              </>
            ) : (
              <span className="text-ink-muted">No service loaded</span>
            )}
            {isUpcoming && <Badge className="ml-2 align-middle" tone="info">Upcoming service</Badge>}
            {reviewing && <Badge className="ml-2 align-middle" tone="idle">Past service</Badge>}
          </p>
          <ReasonButton
            aria-label="Next service"
            className="text-xl leading-none"
            disabled={busy}
            onClick={() => next && choice.pick(next.id)}
            reason={next ? undefined : "This is the last service."}
            size="touch"
          >
            ›
          </ReasonButton>
        </div>
        <div className="relative">
          <ReasonButton
            aria-controls={calendarId}
            aria-expanded={open}
            aria-haspopup="dialog"
            data-service-calendar-button=""
            disabled={busy}
            onClick={() => setOpen((was) => !was)}
            reason={plans.length === 0 ? "There are no services to show." : undefined}
            ref={calendarButton}
            size="touch"
          >
            Calendar
          </ReasonButton>
          {open && (
            <div id={calendarId}>
              <ServicePlanCalendar
                disabled={busy}
                onClose={closeCalendar}
                onPick={(planId) => {
                  setOpen(false);
                  calendarButton.current?.focus();
                  if (planId !== plan?.id) choice.pick(planId);
                }}
                plans={plans}
                selectedId={plan?.id ?? null}
                today={today}
              />
            </div>
          )}
        </div>
        {pinned && plan && <NotUpcomingPill canOperate={canOperate} date={plan.date} size="touch" onBack={choice.back} pending={choice.pending} />}
      </div>

      {!loadingList && !listError && stepping.length === 0 && (
        <p className="mt-2 text-sm text-ink-body">No upcoming services were found.</p>
      )}
      {choice.pending && <p className="mt-2 text-sm text-ink-muted">Loading the service…</p>}
      {choice.error && <p className="mt-2 text-sm text-danger-text" role="alert">{choice.error}</p>}
      {listError && (
        <InlineAlert className="mt-2" tone="warning">
          <p>{listError}</p>
          <Button className="mt-2" disabled={loadingList} onClick={() => setAttempt((count) => count + 1)} variant="secondary">
            Try again
          </Button>
        </InlineAlert>
      )}
      <p aria-live="polite" className="sr-only" role="status">{choice.announcement}</p>
    </section>
  );
}
