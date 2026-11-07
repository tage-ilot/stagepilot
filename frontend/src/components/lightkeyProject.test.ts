import { describe, expect, it } from "vitest";

import type { LightkeyStatus } from "../types";
import { helperIsOutdated, projectLine, projectMismatchText } from "./lightkeyProject";

const base: LightkeyStatus = { api_url: "http://h:8770", link: "connected", detail: null, active_count: null, recording: false, recording_song: null, replay: true, replay_blocked: null, replaying_song: null, message: null };

describe("projectLine", () => {
  it("names the project and counts the fixtures", () => {
    expect(projectLine({ ...base, project: "Show.lightkeyproj", fixtures: { link: "live", detail: null, fixtures: 66, lost_packets: 0, recording: false } })).toBe("Project: Show.lightkeyproj \u00B7 66 fixtures");
  });
  it("is plain when unknown", () => {
    expect(projectLine(base)).toBe("Project not reported yet");
    expect(projectLine({ ...base, project: "  " })).toBe("Project not reported yet");
  });
  it("names the project alone when no count is known", () => {
    expect(projectLine({ ...base, project: "Show" })).toBe("Project: Show");
  });
});

describe("projectMismatchText", () => {
  it("warns only when both projects are known and different", () => {
    expect(projectMismatchText("A", "B")).toBe("Captured with A. Lightkey has B open.");
    expect(projectMismatchText("A", "A")).toBeNull();
    expect(projectMismatchText(undefined, "B")).toBeNull();
    expect(projectMismatchText("A", null)).toBeNull();
    expect(projectMismatchText("", "B")).toBeNull();
  });
});

describe("helperIsOutdated", () => {
  it("is false until the helper has answered, true when it says so", () => {
    expect(helperIsOutdated(base)).toBe(false);
    expect(helperIsOutdated({ ...base, helper: { outdated: true } })).toBe(true);
    expect(helperIsOutdated({ ...base, helper: { outdated: false, version: "0.5.0" } })).toBe(false);
  });
});
