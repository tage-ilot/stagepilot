// Made-up services for the Services page of the section gallery: the days are counted from today (the Planning Center time zone the
// made-up settings name), so the "past 20 days" look the same whenever the gallery is built. No backend is asked.
import { addDays, todayIn } from "../lib/servicePlans";
import { loadedPlan, plan } from "../test/servicePlanFixtures";
import type { ApplicationState, UpcomingServicePlans } from "../types";
import { madeUpState } from "./panelFixtures";

const TIMEZONE = "America/Los_Angeles";
const at = (days: number) => addDays(todayIn(TIMEZONE), days);

/** Two services that have gone by (within the 20 days) and three to come. */
export const galleryServicePlans = () => [
  plan("g-past-16", at(-16), "Sunday Service"),
  plan("g-past-9", at(-9), "Sunday Service"),
  plan("g-next-4", at(4), "Sunday Service"),
  plan("g-next-11", at(11), "Sunday Service"),
  plan("g-next-18", at(18), "Sunday Service"),
];

/** What `GET /planning-center/plans/upcoming` answers in the gallery. */
export const galleryServiceList = (): UpcomingServicePlans => {
  const plans = galleryServicePlans();
  return { plans, upcoming_id: "g-next-4", pinned_id: null, pinned_date: null };
};

/** The page's state with one of those services loaded: the upcoming one, or a past one picked in the calendar to review. */
export function galleryServiceState(id: "g-next-4" | "g-past-9"): ApplicationState {
  const plans = galleryServicePlans();
  const loaded = plans.find((candidate) => candidate.id === id)!;
  const upcoming = plans.find((candidate) => candidate.id === "g-next-4")!;
  return madeUpState({
    plan: loadedPlan(loaded),
    ...(id === "g-past-9"
      ? { selection: { pinned: true, plan_id: loaded.id, date: loaded.date }, upcoming: { plan_id: upcoming.id, date: upcoming.date, title: upcoming.title } }
      : {}),
  });
}
