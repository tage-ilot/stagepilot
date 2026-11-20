/**
 * The ONE date and time style of the app (decided 2026-11-18, design audit): dates read `Thu 19 Nov`, times read `2:31 PM` (12-hour, no
 * leading zero). Every place that shows a date or a time (the plan line, the plan widget, Services, the Event stream, the save and check
 * lines) goes through here; nothing formats its own. Dates that are calendar dates (`2026-11-19`) are never moved by the computer's zone.
 */

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"] as const;

const ISO = /^(\d{4})-(\d{2})-(\d{2})/;

/** `2026-11-19` (or a timestamp that starts with it) read as the calendar date it names: `Thu 19 Nov`. Anything else comes back unchanged. */
export function dateWords(value: string | null | undefined): string {
  if (!value) return "";
  const match = ISO.exec(value);
  if (!match) return value;
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const date = new Date(Date.UTC(year, month - 1, day, 12));
  // 2026-02-30 is not a date: say it as it came rather than as another day.
  if (date.getUTCFullYear() !== year || date.getUTCMonth() !== month - 1 || date.getUTCDate() !== day) return value;
  return `${WEEKDAYS[date.getUTCDay()]} ${day} ${MONTHS[month - 1]}`;
}

const dateOf = (value: string | number | Date): Date | null => {
  const date = value instanceof Date ? value : new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
};

/** A moment on this computer's clock as `2:31 PM` (`2:31:42 PM` with `seconds`, for the Event stream). Not a time: `fallback`. */
export function timeWords(value: string | number | Date | null | undefined, options: { seconds?: boolean; fallback?: string } = {}): string {
  const date = value === null || value === undefined ? null : dateOf(value);
  if (!date) return options.fallback ?? "";
  const hours = date.getHours();
  const twelve = hours % 12 === 0 ? 12 : hours % 12;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${twelve}:${pad(date.getMinutes())}${options.seconds ? `:${pad(date.getSeconds())}` : ""} ${hours < 12 ? "AM" : "PM"}`;
}

/** A service time the plan carries as `HH:MM` (24-hour) read as `9:00 AM`; text that is not that is left as it is. */
export function clockWords(value: string): string {
  const match = /^(\d{1,2}):(\d{2})$/.exec(value.trim());
  if (!match) return value;
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  if (hours > 23 || minutes > 59) return value;
  return `${hours % 12 === 0 ? 12 : hours % 12}:${match[2]} ${hours < 12 ? "AM" : "PM"}`;
}

/** The times of a service, `9:00 AM, 11:00 AM`. */
export const clocksWords = (values: readonly string[]): string => values.map(clockWords).join(", ");

/** A moment as the time alone when it is today (`2:31 PM`), else with its day (`Wed 18 Nov, 2:31 PM`). */
export function whenWords(value: string | number | Date | null | undefined, now: Date = new Date(), fallback = ""): string {
  const date = value === null || value === undefined ? null : dateOf(value);
  if (!date) return fallback;
  const sameDay = date.getFullYear() === now.getFullYear() && date.getMonth() === now.getMonth() && date.getDate() === now.getDate();
  const iso = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
  return sameDay ? timeWords(date) : `${dateWords(iso)}, ${timeWords(date)}`;
}
