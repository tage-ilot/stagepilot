import { describe, expect, it } from "vitest";

import { ApiError } from "../api";
import {
  WEEKDAYS,
  addDays,
  addMonths,
  formatLongDate,
  formatMonthDay,
  formatMonthTitle,
  formatShortDate,
  formatSpokenDay,
  monthGrid,
  pickerErrorText,
  todayIn,
  weekdayOf,
} from "./servicePlans";

describe("service plan dates", () => {
  it("formats a calendar date as the same calendar date, whatever zone this computer is in", () => {
    expect(formatLongDate("2026-11-22")).toBe("Sun, Nov 22, 2026");
    expect(formatShortDate("2026-11-29")).toBe("Sun, Nov 29");
    expect(formatMonthDay("2026-11-29")).toBe("Nov 29");
    expect(formatSpokenDay("2026-11-29")).toBe("Sunday Nov 29");
    expect(formatMonthTitle("2026-11-01")).toBe("November 2026");
  });

  it("does arithmetic on calendar dates across month and year ends and leap days", () => {
    expect(addDays("2026-11-30", 1)).toBe("2026-12-01");
    expect(addDays("2026-01-01", -1)).toBe("2025-12-31");
    expect(addDays("2028-02-28", 1)).toBe("2028-02-29");
    expect(addMonths("2026-01-31", 1)).toBe("2026-02-28");
    expect(addMonths("2026-12-15", 1)).toBe("2027-01-15");
    expect(addMonths("2026-01-15", -1)).toBe("2025-12-15");
    expect(weekdayOf("2026-11-22")).toBe(0);
    expect(weekdayOf("2026-11-17")).toBe(2);
  });

  it("leaves a value that is not a date alone", () => {
    expect(addDays("soon", 1)).toBe("soon");
    expect(formatLongDate("2026-02-30")).toBe("2026-02-30");
  });

  it("knows today in the Planning Center time zone, not this computer's", () => {
    // 07:30 UTC on Nov 18 is still the evening of Nov 17 in Los Angeles and already the afternoon in Tokyo.
    const instant = new Date("2026-11-18T07:30:00Z");
    expect(todayIn("America/Los_Angeles", instant)).toBe("2026-11-17");
    expect(todayIn("Asia/Tokyo", instant)).toBe("2026-11-18");
    expect(todayIn("UTC", instant)).toBe("2026-11-18");
    // The day turns at local midnight: 23:59 and 00:00 in Los Angeles (UTC-8 in November).
    expect(todayIn("America/Los_Angeles", new Date("2026-11-18T07:59:00Z"))).toBe("2026-11-17");
    expect(todayIn("America/Los_Angeles", new Date("2026-11-18T08:00:00Z"))).toBe("2026-11-18");
  });

  it("falls back to this computer's zone for a name Intl does not know", () => {
    expect(todayIn("Mars/Olympus", new Date("2026-11-18T12:00:00Z"))).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(todayIn(null, new Date("2026-11-18T12:00:00Z"))).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it("lays a month out in weeks starting on Sunday", () => {
    const weeks = monthGrid("2026-11");
    expect(weeks[0]?.[0]).toBe("2026-11-01"); // Nov 1, 2026 is a Sunday
    expect(weeks).toHaveLength(5);
    expect(weeks.at(-1)?.at(-1)).toBe("2026-12-05");
    const february = monthGrid("2026-02");
    expect(february[0]?.[0]).toBe("2026-02-01");
    expect(february).toHaveLength(4);
    expect(monthGrid("2026-10")[0]?.[0]).toBe("2026-09-27");
    expect(WEEKDAYS.map((d) => d.long)[0]).toBe("Sunday");
    expect(WEEKDAYS).toHaveLength(7);
  });

  it("shows the server's own words for a refusal or an outage and plain words otherwise", () => {
    expect(pickerErrorText(new ApiError("That service is not in the list of upcoming services.", 409), "x"))
      .toBe("That service is not in the list of upcoming services.");
    expect(pickerErrorText(new ApiError("Planning Center did not answer in time.", 503), "x"))
      .toBe("Planning Center did not answer in time.");
    expect(pickerErrorText(new ApiError("StagePilot API returned 503.", 503), "x")).not.toMatch(/API returned/);
    expect(pickerErrorText(new ApiError("boom", 500), "x")).not.toBe("boom");
    expect(pickerErrorText(new TypeError("Failed to fetch"), "x")).toMatch(/Can't reach StagePilot/);
    expect(pickerErrorText("odd", "Try again.")).toBe("Try again.");
  });
});
