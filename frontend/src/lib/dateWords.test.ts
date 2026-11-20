import { describe, expect, it } from "vitest";

import { clockWords, clocksWords, dateWords, timeWords, whenWords } from "./dateWords";

describe("the one date and time style", () => {
  it.each([
    ["2026-11-19", "Thu 19 Nov"],
    ["2026-11-22", "Sun 22 Nov"],
    ["2026-01-01", "Thu 1 Jan"],
    ["2026-12-31T23:59:00Z", "Thu 31 Dec"],
    ["", ""],
    ["Not selected", "Not selected"],
    ["2026-13-40", "2026-13-40"],
  ])("dateWords(%j) is %j", (input, words) => expect(dateWords(input)).toBe(words));

  it("never moves a calendar date with the computer's zone", () => {
    expect(dateWords("2026-11-19")).toBe("Thu 19 Nov");
    expect(dateWords(null)).toBe("");
  });

  it.each([
    [new Date(2026, 10, 19, 14, 31, 42), "2:31 PM", "2:31:42 PM"],
    [new Date(2026, 10, 19, 0, 5, 9), "12:05 AM", "12:05:09 AM"],
    [new Date(2026, 10, 19, 12, 0, 0), "12:00 PM", "12:00:00 PM"],
    [new Date(2026, 10, 19, 9, 7, 0), "9:07 AM", "9:07:00 AM"],
  ])("timeWords(%s) is %s, and %s with seconds", (date, words, withSeconds) => {
    expect(timeWords(date)).toBe(words);
    expect(timeWords(date, { seconds: true })).toBe(withSeconds);
    expect(timeWords(date.toISOString())).toBe(words);
  });

  it("says the fallback for no time or a bad one", () => {
    expect(timeWords(null, { fallback: "Not yet" })).toBe("Not yet");
    expect(timeWords("nope", { fallback: "No activity yet" })).toBe("No activity yet");
    expect(timeWords(undefined)).toBe("");
  });

  it.each([["09:00", "9:00 AM"], ["18:30", "6:30 PM"], ["00:15", "12:15 AM"], ["12:00", "12:00 PM"], ["25:00", "25:00"], ["after the service", "after the service"]])("service time %j reads %j", (input, words) => {
    expect(clockWords(input)).toBe(words);
  });

  it("joins service times", () => expect(clocksWords(["09:00", "11:00"])).toBe("9:00 AM, 11:00 AM"));

  it("a moment is the time alone today and carries its day otherwise", () => {
    const now = new Date(2026, 10, 19, 15, 0, 0);
    expect(whenWords(new Date(2026, 10, 19, 14, 31), now)).toBe("2:31 PM");
    expect(whenWords(new Date(2026, 10, 18, 14, 31), now)).toBe("Wed 18 Nov, 2:31 PM");
    expect(whenWords(null, now, "Not yet")).toBe("Not yet");
  });
});
