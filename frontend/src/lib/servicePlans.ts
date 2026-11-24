import { dateWords } from "./dateWords";
// Dates for the service date picker. A plan's date is a calendar date ("2026-11-22"), never an instant, so it is
// carried as that string and only formatted here. No date library: Intl does the words, and the configured Planning
// Center time zone decides what "today" is.

import { describeError } from "../errorText";

/** A calendar date as `YYYY-MM-DD`. */
export type IsoDate = string;

/** How many days back the backend lists services (`PICKER_PAST_DAYS`): a service that has gone by can be loaded to review it. */
export const PICKER_PAST_DAYS = 20;

const ISO = /^(\d{4})-(\d{2})-(\d{2})$/;

export function parseIso(value: IsoDate): { year: number; month: number; day: number } | null {
  const match = ISO.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const check = new Date(Date.UTC(year, month - 1, day));
  if (check.getUTCFullYear() !== year || check.getUTCMonth() !== month - 1 || check.getUTCDate() !== day) return null;
  return { year, month, day };
}

export function toIso(year: number, month: number, day: number): IsoDate {
  const date = new Date(Date.UTC(year, month - 1, day));
  return `${String(date.getUTCFullYear()).padStart(4, "0")}-${String(date.getUTCMonth() + 1).padStart(2, "0")}-${String(date.getUTCDate()).padStart(2, "0")}`;
}

/** Whole days after `value` (negative for before). */
export function addDays(value: IsoDate, days: number): IsoDate {
  const parts = parseIso(value);
  if (!parts) return value;
  return toIso(parts.year, parts.month, parts.day + days);
}

/** `months` after the month of `value`, keeping the day of the month where the month is long enough. */
export function addMonths(value: IsoDate, months: number): IsoDate {
  const parts = parseIso(value);
  if (!parts) return value;
  const first = new Date(Date.UTC(parts.year, parts.month - 1 + months, 1));
  const lastDay = new Date(Date.UTC(first.getUTCFullYear(), first.getUTCMonth() + 1, 0)).getUTCDate();
  return toIso(first.getUTCFullYear(), first.getUTCMonth() + 1, Math.min(parts.day, lastDay));
}

/** 0 = Sunday. */
export function weekdayOf(value: IsoDate): number {
  const parts = parseIso(value);
  return parts ? new Date(Date.UTC(parts.year, parts.month - 1, parts.day)).getUTCDay() : 0;
}

export function monthOf(value: IsoDate): string {
  return value.slice(0, 7);
}

function format(value: IsoDate, options: Intl.DateTimeFormatOptions): string {
  const parts = parseIso(value);
  if (!parts) return value;
  // The date is a calendar date: format it as the same calendar date, whatever zone this computer is in.
  return new Intl.DateTimeFormat("en-US", { ...options, timeZone: "UTC" })
    .format(new Date(Date.UTC(parts.year, parts.month - 1, parts.day, 12)));
}

/** `Sun 22 Nov` (the app's one date style, `lib/dateWords.ts`). */
export const formatLongDate = (value: IsoDate) => dateWords(value);

/** `Sun 29 Nov`: the same style; there is no shorter one. */
export const formatShortDate = (value: IsoDate) => dateWords(value);

/** `29 Nov` */
export const formatMonthDay = (value: IsoDate) => {
  const words = dateWords(value);
  return /^\w{3} \d+ \w{3}$/.test(words) ? words.slice(4) : words;
};

/** `Sunday Nov 29` (what a screen reader reads for a day). */
export function formatSpokenDay(value: IsoDate): string {
  return `${format(value, { weekday: "long" })} ${format(value, { month: "short", day: "numeric" })}`;
}

/** `November 2026` */
export const formatMonthTitle = (value: IsoDate) => format(value, { month: "long", year: "numeric" });

export const WEEKDAYS: ReadonlyArray<{ short: string; long: string }> = [0, 1, 2, 3, 4, 5, 6].map((index) => {
  const sunday = "2026-11-22"; // a Sunday
  const date = addDays(sunday, index);
  return { short: format(date, { weekday: "narrow" }), long: format(date, { weekday: "long" }) };
});

/** Today's date in `timeZone` (the Planning Center time zone), falling back to this computer's zone if the name is not valid. */
export function todayIn(timeZone: string | null | undefined, now: Date = new Date()): IsoDate {
  const read = (zone: string | undefined) => {
    const parts = new Intl.DateTimeFormat("en-US", { timeZone: zone, year: "numeric", month: "2-digit", day: "2-digit" })
      .formatToParts(now);
    const pick = (type: string) => parts.find((part) => part.type === type)?.value ?? "";
    return `${pick("year")}-${pick("month")}-${pick("day")}`;
  };
  try {
    return read(timeZone || undefined);
  } catch {
    return read(undefined);
  }
}

/** The weeks of a month, Sunday first, padded with the days of the neighbouring months. */
export function monthGrid(month: string): IsoDate[][] {
  const first = `${month}-01`;
  const start = addDays(first, -weekdayOf(first));
  const weeks: IsoDate[][] = [];
  for (let week = 0; week < 6; week += 1) {
    const row = Array.from({ length: 7 }, (_, day) => addDays(start, week * 7 + day));
    if (week >= 4 && monthOf(row[0] ?? "") !== month) break;
    weeks.push(row);
  }
  return weeks;
}

/** The message an operator reads for a failed pick or list: the server's own words for a refusal or an outage. */
export function pickerErrorText(cause: unknown, fallback: string): string {
  if (cause instanceof Error && typeof (cause as { status?: unknown }).status === "number") {
    const status = (cause as Error & { status: number }).status;
    if ((status === 409 || status === 503) && !/^StagePilot API returned/.test(cause.message)) return cause.message;
  }
  return describeError(cause, fallback);
}
