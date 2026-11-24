import type { ApplicationState, ServicePlan, UpcomingServicePlan, UpcomingServicePlans } from "../types";

export const plan = (id: string, date: string, title: string, extra: Partial<UpcomingServicePlan> = {}): UpcomingServicePlan => ({
  id,
  date,
  title,
  service_type_id: "42",
  service_type_name: "Weekend Services",
  ...extra,
});

/** Today is Tue Nov 17, 2026; services follow on Sundays. */
export const PLANS: UpcomingServicePlan[] = [
  plan("p-nov-22", "2026-11-22", "Sunday Service"),
  plan("p-nov-29", "2026-11-29", "Advent Sunday"),
  plan("p-dec-06", "2026-12-06", "Communion Sunday"),
  plan("p-dec-13", "2026-12-13", "Family Sunday"),
];

/** Services of the days gone by (the backend lists the past 20 days): the Sundays of Nov 8 and Nov 15. */
export const PAST_PLANS: UpcomingServicePlan[] = [
  plan("p-nov-08", "2026-11-08", "Past Sunday"),
  plan("p-nov-15", "2026-11-15", "Last Sunday"),
];

export const list = (overrides: Partial<UpcomingServicePlans> = {}): UpcomingServicePlans => ({
  plans: PLANS,
  upcoming_id: "p-nov-22",
  pinned_id: null,
  pinned_date: null,
  ...overrides,
});

export function loadedPlan(from: UpcomingServicePlan): ServicePlan {
  return {
    id: from.id,
    title: from.title,
    date: from.date,
    service_type: from.service_type_name,
    service_type_id: from.service_type_id,
    service_times: ["09:00"],
    duration_source: "Planning Center scheduled item length",
    songs: [],
  };
}

/** The application state with `from` loaded, and the pick the backend reports. */
export function stateFor(from: UpcomingServicePlan, pinned: boolean, extra: Partial<ApplicationState> = {}): ApplicationState {
  const upcoming = PLANS[0]!;
  return {
    revision: 5,
    updated_at: "2026-11-17T16:00:00Z",
    application_status: "running",
    plan: loadedPlan(from),
    current_song: null,
    next_song: null,
    current_song_index: null,
    playing_song: null,
    playing_song_index: null,
    up_next_song: null,
    planning_center_status: "connected",
    midi_status: "disconnected",
    propresenter_status: "disconnected",
    lights_status: "disconnected",
    service_load: {
      status: "loaded",
      target_date: from.date,
      candidates: [],
      skipped_items: [],
      message: null,
      is_stale: false,
      last_attempt_at: null,
    },
    timer: { status: "idle", duration_seconds: null, started_at: null, last_error: null },
    plugins: {
      planning_center: { name: "planning_center", version: "0.1.0", status: "running", last_error: null, last_activity_at: null },
    },
    recent_events: [],
    recent_errors: [],
    last_successful_plan_reload_at: null,
    last_action: null,
    selection: { pinned, plan_id: from.id, date: from.date },
    upcoming: { plan_id: upcoming.id, date: upcoming.date, title: upcoming.title },
    ...extra,
  };
}
