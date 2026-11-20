import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ServicePlanChoice } from "../hooks/useServicePlanChoice";
import { PLANS, list, stateFor } from "../test/servicePlanFixtures";
import type { ApplicationState, UpcomingServicePlans } from "../types";
import { ServicePlanPicker } from "./ServicePlanPicker";

const choiceOf = (overrides: Partial<ServicePlanChoice> = {}): ServicePlanChoice => ({
  pending: false,
  error: null,
  announcement: "",
  pick: vi.fn(),
  back: vi.fn(),
  ...overrides,
});

async function show(
  state: ApplicationState,
  { choice = choiceOf(), answer = list(), canOperate = true }: { choice?: ServicePlanChoice; answer?: UpcomingServicePlans | Error; canOperate?: boolean } = {},
) {
  const loadPlans = vi.fn(() => (answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer)));
  const view = render(<ServicePlanPicker canOperate={canOperate} choice={choice} loadPlans={loadPlans} state={state} timezone="America/Los_Angeles" />);
  await waitFor(() => expect(loadPlans).toHaveBeenCalled());
  await act(async () => { await Promise.resolve(); });
  return { choice, loadPlans, ...view };
}

const previous = () => screen.getByRole("button", { name: "Previous service" });
const next = () => screen.getByRole("button", { name: "Next service" });
const calendar = () => screen.getByRole("button", { name: "Calendar" });

describe("ServicePlanPicker", () => {
  it("shows the loaded service's date and title with the Upcoming service tag, and the left arrow rests", async () => {
    await show(stateFor(PLANS[0]!, false));
    expect(screen.getByTestId("picker-selected")).toHaveTextContent("Sun 22 Nov");
    expect(screen.getByTestId("picker-selected")).toHaveTextContent("Sunday Service");
    expect(screen.getByText("Upcoming service")).toBeInTheDocument();
    expect(previous()).toBeDisabled();
    expect(previous()).toHaveAccessibleDescription("This is the first service.");
    expect(previous().parentElement).toHaveAttribute("title", "This is the first service.");
    expect(next()).toBeEnabled();
    expect(next()).not.toHaveAttribute("aria-describedby");
    expect(screen.queryByTestId("not-upcoming")).not.toBeInTheDocument();
  });

  it("steps to the next service, not the next day", async () => {
    const { choice } = await show(stateFor(PLANS[0]!, false));
    await userEvent.click(next());
    expect(choice.pick).toHaveBeenCalledWith("p-nov-29");
  });

  it("steps back and forth in the middle of the list", async () => {
    const { choice } = await show(stateFor(PLANS[1]!, true), { answer: list({ pinned_id: "p-nov-29", pinned_date: "2026-11-29" }) });
    expect(screen.queryByText("Upcoming service")).not.toBeInTheDocument();
    expect(previous()).toBeEnabled();
    expect(next()).toBeEnabled();
    await userEvent.click(previous());
    expect(choice.pick).toHaveBeenLastCalledWith("p-nov-22");
    await userEvent.click(next());
    expect(choice.pick).toHaveBeenLastCalledWith("p-dec-06");
  });

  it("disables the right arrow on the last service", async () => {
    await show(stateFor(PLANS[3]!, true));
    expect(next()).toBeDisabled();
    expect(previous()).toBeEnabled();
  });

  it("rests the left arrow on the upcoming service even when it is not first in the list", async () => {
    // Two services today; the second is the upcoming one (a preferred time). Going left would leave it.
    await show(stateFor(PLANS[1]!, false), { answer: list({ upcoming_id: "p-nov-29" }) });
    expect(screen.getByText("Upcoming service")).toBeInTheDocument();
    expect(previous()).toBeDisabled();
  });

  it("shows the pill and Back to upcoming while another service is picked", async () => {
    const { choice } = await show(stateFor(PLANS[2]!, true));
    expect(screen.getByTestId("not-upcoming-pill")).toHaveTextContent("Current upcoming service not selected");
    await userEvent.click(screen.getByRole("button", { name: "Current Upcoming" }));
    expect(choice.back).toHaveBeenCalledTimes(1);
  });

  it("says plainly when there are no upcoming services and offers no calendar", async () => {
    await show(stateFor(PLANS[0]!, false), { answer: list({ plans: [], upcoming_id: null }) });
    expect(screen.getByText("No upcoming services were found.")).toBeInTheDocument();
    expect(calendar()).toBeDisabled();
    expect(next()).toBeDisabled();
  });

  it("says no service is loaded and still lets a service be picked from the calendar", async () => {
    const state = stateFor(PLANS[0]!, false, { plan: null, selection: undefined, upcoming: undefined });
    const { choice } = await show(state, { answer: list({ upcoming_id: null }) });
    expect(screen.getByText("No service loaded")).toBeInTheDocument();
    expect(previous()).toBeDisabled();
    expect(next()).toBeDisabled();
    await userEvent.click(calendar());
    await userEvent.click(screen.getByRole("button", { name: "Sunday Nov 29: Advent Sunday" }));
    expect(choice.pick).toHaveBeenCalledWith("p-nov-29");
  });

  it("shows why the list could not load and tries again", async () => {
    const { loadPlans } = await show(stateFor(PLANS[0]!, false), { answer: Object.assign(new Error("Planning Center did not answer in time."), { status: 503 }) });
    expect(screen.getByText("Planning Center did not answer in time.")).toBeInTheDocument();
    expect(previous()).toBeDisabled();
    expect(calendar()).toBeDisabled();
    loadPlans.mockResolvedValueOnce(list());
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.queryByText("Planning Center did not answer in time.")).not.toBeInTheDocument());
    expect(next()).toBeEnabled();
  });

  it("waits while a change is loading: controls disabled and the section says it is busy", async () => {
    const { container } = await show(stateFor(PLANS[1]!, true), { choice: choiceOf({ pending: true }) });
    expect(container.querySelector("section")).toHaveAttribute("aria-busy", "true");
    expect(previous()).toBeDisabled();
    expect(next()).toBeDisabled();
    expect(calendar()).toBeDisabled();
    expect(screen.getByRole("button", { name: "Current Upcoming" })).toBeDisabled();
    expect(screen.getByText("Loading the service…")).toBeInTheDocument();
  });

  it("is not busy when nothing is loading", async () => {
    const { container } = await show(stateFor(PLANS[1]!, true));
    expect(container.querySelector("section")).not.toHaveAttribute("aria-busy");
  });

  it("shows a failed change inline, where the operator is looking", async () => {
    await show(stateFor(PLANS[0]!, false), { choice: choiceOf({ error: "Planning Center did not answer in time." }) });
    expect(screen.getByRole("alert")).toHaveTextContent("Planning Center did not answer in time.");
  });

  it("announces a change politely", async () => {
    await show(stateFor(PLANS[1]!, true), { choice: choiceOf({ announcement: "Loaded Advent Sunday, Nov 29" }) });
    const region = screen.getByRole("status");
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveTextContent("Loaded Advent Sunday, Nov 29");
  });

  it("changes nothing for a session that can only look", async () => {
    await show(stateFor(PLANS[1]!, true), { canOperate: false });
    expect(previous()).toBeDisabled();
    expect(next()).toBeDisabled();
    expect(calendar()).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Current Upcoming" })).not.toBeInTheDocument();
    expect(screen.getByTestId("not-upcoming-pill")).toBeInTheDocument();
  });

  it("opens the calendar, picks a day, and puts focus back on the Calendar button", async () => {
    const { choice } = await show(stateFor(PLANS[0]!, false));
    await userEvent.click(calendar());
    expect(calendar()).toHaveAttribute("aria-expanded", "true");
    const dialog = screen.getByRole("dialog", { name: "Choose a service date" });
    // December is another month: step there first.
    await userEvent.click(within(dialog).getByRole("button", { name: "Next month" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Sunday Dec 6: Communion Sunday" }));
    expect(choice.pick).toHaveBeenCalledWith("p-dec-06");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(calendar()).toHaveFocus();
  });

  it("does not ask again when the loaded service is picked", async () => {
    const { choice } = await show(stateFor(PLANS[0]!, false));
    await userEvent.click(calendar());
    await userEvent.click(screen.getByRole("button", { name: "Sunday Nov 22: Sunday Service" }));
    expect(choice.pick).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("closes the calendar with Escape and returns focus to the button", async () => {
    await show(stateFor(PLANS[0]!, false));
    await userEvent.click(calendar());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(calendar()).toHaveFocus();
    expect(calendar()).toHaveAttribute("aria-expanded", "false");
  });

  it("asks for the list again after another service is loaded", async () => {
    const first = stateFor(PLANS[0]!, false);
    const loadPlans = vi.fn(() => Promise.resolve(list()));
    const choice = choiceOf();
    const { rerender } = render(<ServicePlanPicker canOperate choice={choice} loadPlans={loadPlans} state={first} timezone="UTC" />);
    await waitFor(() => expect(loadPlans).toHaveBeenCalledTimes(1));
    rerender(<ServicePlanPicker canOperate choice={choice} loadPlans={loadPlans} state={stateFor(PLANS[2]!, true)} timezone="UTC" />);
    await waitFor(() => expect(loadPlans).toHaveBeenCalledTimes(2));
    // The same state again is not a reason to ask.
    rerender(<ServicePlanPicker canOperate choice={choice} loadPlans={loadPlans} state={{ ...stateFor(PLANS[2]!, true), revision: 99 }} timezone="UTC" />);
    await act(async () => { await Promise.resolve(); });
    expect(loadPlans).toHaveBeenCalledTimes(2);
  });

  it("marks today in the calendar using the configured time zone", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-11-18T07:30:00Z")); // still Nov 17 in Los Angeles
    try {
      await show(stateFor(PLANS[0]!, false));
      await userEvent.click(calendar());
      expect(screen.getByRole("button", { name: "Tuesday Nov 17" })).toHaveAttribute("data-today", "true");
      expect(screen.getByRole("button", { name: "Wednesday Nov 18" })).not.toHaveAttribute("data-today");
    } finally {
      vi.useRealTimers();
    }
  });
});
