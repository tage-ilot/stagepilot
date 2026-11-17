import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { PLANS, plan } from "../test/servicePlanFixtures";
import { ServicePlanCalendar } from "./ServicePlanCalendar";

const TODAY = "2026-11-17";

function open(props: Partial<React.ComponentProps<typeof ServicePlanCalendar>> = {}) {
  const onPick = vi.fn();
  const onClose = vi.fn();
  render(<ServicePlanCalendar onClose={onClose} onPick={onPick} plans={PLANS} selectedId="p-nov-22" today={TODAY} {...props} />);
  return { onPick, onClose };
}

const day = (name: string | RegExp) => screen.getByRole("button", { name });
const focusedDate = () => (document.activeElement as HTMLElement | null)?.getAttribute("data-date");

describe("ServicePlanCalendar", () => {
  it("is a labelled dialog with a month grid, weekday headers and the month named", () => {
    open();
    const dialog = screen.getByRole("dialog", { name: "Choose a service date" });
    expect(within(dialog).getByRole("grid")).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "November 2026" })).toBeInTheDocument();
    expect(within(dialog).getAllByRole("columnheader")).toHaveLength(7);
    expect(within(dialog).getByRole("columnheader", { name: "Sunday" })).toBeInTheDocument();
  });

  it("names a day with a service by its day and title, and marks which day is the loaded one and which is today", () => {
    open();
    const sunday = day("Sunday Nov 22: Sunday Service");
    expect(sunday).toHaveAttribute("data-has-service", "true");
    expect(sunday).not.toHaveAttribute("aria-disabled");
    expect(sunday.closest("[role=gridcell]")).toHaveAttribute("aria-selected", "true");
    expect(day("Sunday Nov 29: Advent Sunday").closest("[role=gridcell]")).toHaveAttribute("aria-selected", "false");
    expect(day("Tuesday Nov 17")).toHaveAttribute("aria-current", "date");
    expect(day("Tuesday Nov 17")).toHaveAttribute("data-today", "true");
  });

  it("dims the days without a service so they cannot be picked", async () => {
    const { onPick } = open();
    const plain = day("Monday Nov 23");
    expect(plain).toHaveAttribute("aria-disabled", "true");
    expect(plain).not.toHaveAttribute("data-has-service");
    await userEvent.click(plain);
    expect(onPick).not.toHaveBeenCalled();
  });

  it("picks the service on a day when its day is clicked", async () => {
    const { onPick } = open();
    await userEvent.click(day("Sunday Nov 29: Advent Sunday"));
    expect(onPick).toHaveBeenCalledWith("p-nov-29");
  });

  it("starts with focus on the loaded service's day, and only that day is in the tab order", () => {
    open();
    expect(focusedDate()).toBe("2026-11-22");
    const stops = screen.getAllByRole("button").filter((b) => b.getAttribute("tabindex") === "0" && b.hasAttribute("data-date"));
    expect(stops).toHaveLength(1);
  });

  it("moves by day with the left and right arrows and by week with up and down", async () => {
    open();
    await userEvent.keyboard("{ArrowRight}");
    expect(focusedDate()).toBe("2026-11-23");
    await userEvent.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(focusedDate()).toBe("2026-11-21");
    await userEvent.keyboard("{ArrowDown}");
    expect(focusedDate()).toBe("2026-11-28");
    await userEvent.keyboard("{ArrowUp}{ArrowUp}");
    expect(focusedDate()).toBe("2026-11-14");
  });

  it("goes to the start and end of the week with Home and End", async () => {
    open();
    await userEvent.keyboard("{ArrowRight}{ArrowRight}{End}");
    expect(focusedDate()).toBe("2026-11-28");
    await userEvent.keyboard("{Home}");
    expect(focusedDate()).toBe("2026-11-22");
  });

  it("changes the month with PageDown and PageUp, keeping the day where it can, and follows an arrow across the month end", async () => {
    open();
    await userEvent.keyboard("{PageDown}");
    expect(screen.getByRole("heading", { name: "December 2026" })).toBeInTheDocument();
    expect(focusedDate()).toBe("2026-12-22");
    await userEvent.keyboard("{PageUp}");
    expect(screen.getByRole("heading", { name: "November 2026" })).toBeInTheDocument();
    expect(focusedDate()).toBe("2026-11-22");
    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    expect(focusedDate()).toBe("2026-12-06");
    expect(screen.getByRole("heading", { name: "December 2026" })).toBeInTheDocument();
  });

  it("does not go before this month or past the last month with a service", async () => {
    open();
    await userEvent.keyboard("{PageUp}{PageUp}");
    expect(screen.getByRole("heading", { name: "November 2026" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous month" })).toBeDisabled();
    await userEvent.keyboard("{PageDown}{PageDown}{PageDown}");
    expect(screen.getByRole("heading", { name: "December 2026" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next month" })).toBeDisabled();
  });

  it("changes the month with its buttons", async () => {
    open();
    await userEvent.click(screen.getByRole("button", { name: "Next month" }));
    expect(screen.getByRole("heading", { name: "December 2026" })).toBeInTheDocument();
    expect(day("Sunday Dec 6: Communion Sunday")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Previous month" }));
    expect(screen.getByRole("heading", { name: "November 2026" })).toBeInTheDocument();
  });

  it("picks with Enter on a day with a service and ignores Enter on a day without", async () => {
    const { onPick } = open();
    await userEvent.keyboard("{ArrowRight}{Enter}");
    expect(onPick).not.toHaveBeenCalled();
    await userEvent.keyboard("{ArrowLeft}{Enter}");
    expect(onPick).toHaveBeenCalledWith("p-nov-22");
    await userEvent.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}{ }");
    expect(onPick).toHaveBeenLastCalledWith("p-nov-29");
  });

  it("closes on Escape", async () => {
    const { onClose } = open();
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on a click outside it but not on a click inside", async () => {
    const { onClose } = open();
    await userEvent.click(screen.getByRole("heading", { name: "November 2026" }));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.mouseDown(document.body);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("asks which one when several services share a day, and picks by id", async () => {
    const plans = [plan("a", "2026-11-22", "Morning Service"), plan("b", "2026-11-22", "Evening Service", { service_type_name: "Youth" }), ...PLANS.slice(1)];
    const { onPick } = open({ plans, selectedId: "a" });
    const shared = day("Sunday Nov 22: 2 services");
    await userEvent.click(shared);
    expect(onPick).not.toHaveBeenCalled();
    const choice = screen.getByRole("group", { name: "Services on Sunday Nov 22" });
    expect(within(choice).getAllByRole("button")).toHaveLength(2);
    await userEvent.click(within(choice).getByRole("button", { name: "Sunday Nov 22: Evening Service" }));
    expect(onPick).toHaveBeenCalledWith("b");
  });

  it("cannot pick while a change is loading", async () => {
    const { onPick } = open({ disabled: true });
    await userEvent.click(day("Sunday Nov 29: Advent Sunday"));
    await userEvent.keyboard("{Enter}");
    expect(onPick).not.toHaveBeenCalled();
    expect(day("Sunday Nov 29: Advent Sunday")).toHaveAttribute("aria-disabled", "true");
  });

  it("opens on this month when nothing is loaded and uses the first service's month", () => {
    open({ selectedId: null, plans: [plan("far", "2027-01-10", "January Sunday")], today: "2026-12-30" });
    expect(screen.getByRole("heading", { name: "January 2027" })).toBeInTheDocument();
    expect(focusedDate()).toBe("2027-01-10");
  });
});
