import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";

import {
  PICKER_PAST_DAYS,
  WEEKDAYS,
  addDays,
  addMonths,
  formatMonthTitle,
  formatSpokenDay,
  monthGrid,
  monthOf,
  parseIso,
  weekdayOf,
  type IsoDate,
} from "../lib/servicePlans";
import type { UpcomingServicePlan } from "../types";
import { accentClasses } from "./ui/accent";
import { Button } from "./ui/Button";
import { ReasonButton } from "./sections";
import { cx } from "./ui/cx";

export interface ServicePlanCalendarProps {
  plans: UpcomingServicePlan[];
  /** The loaded service, marked in the grid. */
  selectedId: string | null;
  /** Today's date in the Planning Center time zone, marked in the grid. */
  today: IsoDate;
  /** A change is loading: nothing can be picked. */
  disabled?: boolean;
  onPick: (planId: string) => void;
  /** Escape, or a click outside. The picker puts focus back on its calendar button. */
  onClose: () => void;
  className?: string;
}

const lastDayOfMonth = (month: string) => addDays(addMonths(`${month}-01`, 1), -1);

/**
 * A month grid of the days that have a service. Days with a service are marked and can be picked; the other days are
 * dimmed and cannot. Arrow keys move by day (up and down by week), PageUp and PageDown change the month, Home and End
 * go to the start and end of the week, Enter picks, Escape closes. Several services on one day ask which one.
 */
export function ServicePlanCalendar({ plans, selectedId, today, disabled = false, onPick, onClose, className }: ServicePlanCalendarProps) {
  const titleId = useId();
  const byDate = useMemo(() => {
    const map = new Map<IsoDate, UpcomingServicePlan[]>();
    for (const plan of plans) map.set(plan.date, [...(map.get(plan.date) ?? []), plan]);
    return map;
  }, [plans]);
  const selectedDate = plans.find((plan) => plan.id === selectedId)?.date ?? null;
  // It opens on the month of today; the grid reaches back as far as the services the backend lists.
  const first = monthOf(addDays(today, -PICKER_PAST_DAYS));
  const lastPlan = plans.at(-1);
  const lastPlanMonth = lastPlan ? monthOf(lastPlan.date) : first;
  const lastMonth = lastPlanMonth > first ? lastPlanMonth : first;
  const minDate = `${first}-01`;
  const maxDate = lastDayOfMonth(lastMonth);
  const clamp = (value: IsoDate) => (value < minDate ? minDate : value > maxDate ? maxDate : value);

  const start = clamp(selectedDate ?? plans.find((plan) => plan.date >= today)?.date ?? today);
  const [focusDate, setFocusDate] = useState<IsoDate>(start);
  const [month, setMonth] = useState(monthOf(start));
  const [choosing, setChoosing] = useState<IsoDate | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const cells = useRef(new Map<IsoDate, HTMLButtonElement>());
  const refocus = useRef(true);

  useEffect(() => {
    if (!refocus.current) return;
    refocus.current = false;
    cells.current.get(focusDate)?.focus();
  }, [focusDate, month]);

  // A click outside closes it like Escape does.
  useEffect(() => {
    const away = (event: MouseEvent | TouchEvent) => {
      const target = event.target as Node | null;
      if (!root.current || (target && root.current.contains(target))) return;
      // The button that opens it handles its own click (it toggles).
      if (target instanceof Element && target.closest("[data-service-calendar-button]")) return;
      onClose();
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("touchstart", away);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("touchstart", away);
    };
  }, [onClose]);

  const moveTo = (value: IsoDate) => {
    const next = clamp(value);
    refocus.current = true;
    setChoosing(null);
    setFocusDate(next);
    setMonth(monthOf(next));
  };

  const choose = (date: IsoDate) => {
    if (disabled) return;
    const onDay = byDate.get(date);
    if (!onDay?.length) return;
    const only = onDay[0];
    if (onDay.length === 1 && only) onPick(only.id);
    else setChoosing(date);
  };

  const onGridKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const keys: Record<string, () => IsoDate> = {
      ArrowLeft: () => addDays(focusDate, -1),
      ArrowRight: () => addDays(focusDate, 1),
      ArrowUp: () => addDays(focusDate, -7),
      ArrowDown: () => addDays(focusDate, 7),
      Home: () => addDays(focusDate, -weekdayOf(focusDate)),
      End: () => addDays(focusDate, 6 - weekdayOf(focusDate)),
      PageUp: () => addMonths(focusDate, event.shiftKey ? -12 : -1),
      PageDown: () => addMonths(focusDate, event.shiftKey ? 12 : 1),
    };
    const move = keys[event.key];
    if (move) {
      event.preventDefault();
      moveTo(move());
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      choose(focusDate);
    }
  };

  const showMonth = (next: string) => {
    const target = next < first ? first : next > lastMonth ? lastMonth : next;
    refocus.current = false;
    setChoosing(null);
    setMonth(target);
    setFocusDate(clamp(`${target}-${String(Math.min(parseIso(focusDate)?.day ?? 1, 28)).padStart(2, "0")}`));
  };

  const weeks = monthGrid(month);
  const chosen = choosing ? byDate.get(choosing) ?? [] : [];

  return (
    <div
      aria-label="Choose a service date"
      className={cx(
        // A full-width sheet on a phone, a popover under the button from 640 px up.
        "service-calendar fixed inset-x-0 bottom-0 z-50 max-h-[85vh] overflow-y-auto rounded-t-ds-xl border border-edge-medium bg-surface-950 p-4 shadow-elevation-overlay",
        "sm:absolute sm:inset-x-auto sm:bottom-auto sm:left-0 sm:top-full sm:mt-2 sm:w-[22rem] sm:max-w-[calc(100vw-2rem)] sm:rounded-ds-xl",
        className,
      )}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
        }
      }}
      ref={root}
      role="dialog"
    >
      <div className="flex items-center justify-between gap-2">
        <ReasonButton aria-label="Previous month" onClick={() => showMonth(monthOf(addMonths(`${month}-01`, -1)))} reason={month <= first ? "There are no earlier services." : undefined} size="touch">
          ‹
        </ReasonButton>
        <h3 aria-live="polite" className="text-center font-type-text-strong text-type-text-strong text-ink" id={titleId}>
          {formatMonthTitle(`${month}-01`)}
        </h3>
        <ReasonButton aria-label="Next month" onClick={() => showMonth(monthOf(addMonths(`${month}-01`, 1)))} reason={month >= lastMonth ? "There are no later services." : undefined} size="touch">
          ›
        </ReasonButton>
      </div>

      <div aria-labelledby={titleId} className="mt-3" onKeyDown={onGridKey} role="grid">
        <div className="grid grid-cols-7 text-center text-type-micro uppercase text-ink-muted" role="row">
          {WEEKDAYS.map((day) => (
            <span aria-label={day.long} className="py-1" key={day.long} role="columnheader" title={day.long}>
              {day.short}
            </span>
          ))}
        </div>
        {weeks.map((week) => (
          <div className="grid grid-cols-7" key={week[0]} role="row">
            {week.map((date) => {
              const onDay = byDate.get(date) ?? [];
              const hasService = onDay.length > 0;
              const inMonth = monthOf(date) === month;
              const isSelected = Boolean(selectedId) && onDay.some((plan) => plan.id === selectedId);
              const isToday = date === today;
              const isPast = date < today;
              const name = hasService
                ? `${formatSpokenDay(date)}: ${onDay.length === 1 ? onDay[0]?.title : `${onDay.length} services`}`
                : formatSpokenDay(date);
              return (
                <div aria-selected={hasService ? isSelected : undefined} className="grid place-items-center p-0.5" key={date} role="gridcell">
                  <button
                    aria-current={isToday ? "date" : undefined}
                    aria-disabled={!hasService || disabled ? true : undefined}
                    aria-label={name}
                    className={cx(
                      "relative grid h-10 w-10 coarse:h-11 coarse:w-11 touch-manipulation place-items-center rounded-ds-lg border font-type-label text-type-label tabular-nums transition-colors duration-[140ms] motion-reduce:transition-none",
                      isSelected
                        ? cx("font-bold", accentClasses.services.solid)
                        : hasService
                          ? isPast
                            ? "border-edge-medium font-medium text-ink-muted can-hover:text-ink"
                            : cx("font-bold", accentClasses.services.tinted, accentClasses.services.tintedHover)
                          : "border-transparent text-ink-muted opacity-60",
                      !inMonth && !hasService && "opacity-40",
                      isToday && "underline decoration-2 underline-offset-4",
                      isToday && !isSelected && "border-ink-muted",
                      (!hasService || disabled) && "cursor-not-allowed",
                    )}
                    data-date={date}
                    data-has-service={hasService ? "true" : undefined}
                    data-past={hasService && isPast ? "true" : undefined}
                    data-today={isToday ? "true" : undefined}
                    onClick={() => {
                      setFocusDate(date);
                      choose(date);
                    }}
                    ref={(element) => {
                      if (element) cells.current.set(date, element);
                      else cells.current.delete(date);
                    }}
                    tabIndex={date === focusDate ? 0 : -1}
                    type="button"
                  >
                    {parseIso(date)?.day}
                    {hasService && (
                      <span aria-hidden="true" className="absolute bottom-1 h-1 w-1 rounded-ds-full bg-current" />
                    )}
                  </button>
                </div>
              );
            })}
          </div>
        ))}
      </div>

      {choosing && (
        <div aria-label={`Services on ${formatSpokenDay(choosing)}`} className="mt-3 grid gap-2 border-t border-edge-subtle pt-3" role="group">
          {chosen.map((plan, index) => (
            <Button
              aria-label={`${formatSpokenDay(choosing)}: ${plan.title}`}
              autoFocus={index === 0}
              disabled={disabled}
              key={plan.id}
              onClick={() => onPick(plan.id)}
              variant={plan.id === selectedId ? "primary" : "secondary"}
            >
              {plan.title}
              <span className="ml-2 text-ink-muted">{plan.service_type_name}</span>
            </Button>
          ))}
        </div>
      )}
      <p className="mt-3 text-xs text-ink-muted">
        Days with a dot have a service. Arrow keys move, Page Up and Page Down change the month, Enter picks, Escape closes.
      </p>
    </div>
  );
}
