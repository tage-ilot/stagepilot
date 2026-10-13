import { describe, expect, it } from "vitest";

import type { ApplicationState, ErrorSummary } from "../../types";
import { latestActiveError } from "./dashboardActiveError";

function error(component: string, message: string, timestamp: string): ErrorSummary {
  return { component, event_id: null, message, timestamp };
}

function state(overrides: Partial<ApplicationState> = {}): ApplicationState {
  return {
    application_status: "running",
    current_song: null,
    current_song_index: null,
    playing_song: null, playing_song_index: null, up_next_song: null,
    last_action: null,
    last_successful_plan_reload_at: null,
    lights_status: "connected",
    midi_status: "connected",
    next_song: null,
    plan: null,
    planning_center_status: "connected",
    plugins: {},
    propresenter_status: "connected",
    recent_errors: [],
    recent_events: [],
    revision: 1,
    service_load: {
      status: "idle",
      target_date: null,
      message: null,
      is_stale: false,
      last_attempt_at: null,
      skipped_items: [],
      candidates: [],
    },
    timer: {
      duration_seconds: null,
      last_error: null,
      started_at: null,
      status: "stopped",
    },
    updated_at: "2026-07-29T20:00:00Z",
    ...overrides,
  };
}

describe("latestActiveError", () => {
  it("returns only the newest repeated error", () => {
    const older = error("propresenter", "Could not connect.", "2026-07-29T20:00:00Z");
    const newer = error("propresenter", "Could not connect.", "2026-07-29T20:00:10Z");

    expect(
      latestActiveError(
        state({
          propresenter_status: "error",
          recent_errors: [older, newer],
        }),
      ),
    ).toEqual(newer);
  });

  it("unpins a ProPresenter error after the connection recovers", () => {
    expect(
      latestActiveError(
        state({
          propresenter_status: "connected",
          recent_errors: [
            error("propresenter", "Could not connect.", "2026-07-29T20:00:00Z"),
          ],
        }),
      ),
    ).toBeNull();
  });

  it("can pin an active timer or plugin error", () => {
    const pluginError = error("custom-output", "Output failed.", "2026-07-29T20:00:10Z");
    expect(
      latestActiveError(
        state({
          plugins: {
            "custom-output": {
              last_activity_at: null,
              last_error: "Output failed.",
              name: "custom-output",
              status: "error",
              version: "1.0.0",
            },
          },
          recent_errors: [
            error("timer", "Timer failed.", "2026-07-29T20:00:00Z"),
            pluginError,
          ],
          timer: {
            duration_seconds: null,
            last_error: "Timer failed.",
            started_at: null,
            status: "error",
          },
        }),
      ),
    ).toEqual(pluginError);
  });
});
