import { describe, expect, it } from "vitest";

import { ApiError } from "./api";
import { describeError, OFFLINE_SENTENCE } from "./errorText";

describe("describeError", () => {
  it("says the API is unreachable for fetch rejections and timeouts", () => {
    expect(describeError(new TypeError("Failed to fetch"), "x")).toBe(OFFLINE_SENTENCE);
    expect(describeError(new TypeError("Load failed"), "x")).toBe(OFFLINE_SENTENCE);
    expect(describeError(new DOMException("t", "TimeoutError"), "x")).toBe(OFFLINE_SENTENCE);
  });

  it("never shows a raw status line, a one-word server detail or undefined for 5xx and 4xx", () => {
    expect(describeError(new ApiError("StagePilot API returned 500.", 500), "x")).toBe("StagePilot hit an error. Try again.");
    expect(describeError(new ApiError("boom", 500), "x")).toBe("StagePilot hit an error. Try again.");
    expect(describeError(new ApiError("StagePilot API returned 422.", 422), "x"))
      .toBe("StagePilot could not do that. Check the values and try again.");
    expect(describeError(new ApiError("boom", 400), "x")).toBe("StagePilot could not do that. Check the values and try again.");
  });

  it("keeps readable server sentences and sign-in messages", () => {
    expect(describeError(new ApiError("That port is already in use.", 409), "x")).toBe("That port is already in use.");
    expect(describeError(new ApiError("Please sign in to continue.", 401), "x")).toBe("Please sign in to continue.");
  });

  it("falls back for non-errors and unreadable messages", () => {
    expect(describeError("nope", "Configuration unavailable.")).toBe("Configuration unavailable.");
    expect(describeError(new Error("[object Object]"), "Configuration unavailable.")).toBe("Configuration unavailable.");
    expect(describeError(new Error("undefined"), "Configuration unavailable.")).toBe("Configuration unavailable.");
  });
});
