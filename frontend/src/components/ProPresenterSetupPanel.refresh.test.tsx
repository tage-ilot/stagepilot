import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { presentation } from "../test/propresenterLinkFixtures";
import { ProPresenterSetupPanel } from "./ProPresenterSetupPanel";

// F14: the Refresh button sits right under the Timer box, inside the Timer field, and the panel draws no Actions row of its own (no empty band).

const draw = (over: Partial<Parameters<typeof ProPresenterSetupPanel>[0]> = {}) => render(
  <ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} propresenter={presentation({ looks: [{ id: "l1", name: "Default", index: 0 }], current_look_id: "l1" })} {...over} />,
);
const fieldOf = (name: string) => screen.getByRole("combobox", { name }).closest<HTMLElement>("[data-field]")!;
const refresh = () => screen.getByRole("button", { name: "Refresh timers and Looks" });

describe("Presentation: Refresh sits right under the Timer box", () => {
  it("is inside the Timer column and not inside the Look column, whose hint stays under the Look box", () => {
    draw();
    expect(fieldOf("Timer")).toContainElement(refresh());
    expect(fieldOf("Look")).not.toContainElement(refresh());
    const hint = screen.getByText(/Selecting a Look does not change Presentation/);
    expect(fieldOf("Look")).toContainElement(hint);
    expect(fieldOf("Timer")).not.toContainElement(hint);
  });

  it("follows the Timer box directly in its column (the Timer has no hint), so there is no blank band between them", () => {
    draw();
    const column = fieldOf("Timer");
    const action = column.querySelector("[data-part=action]")!;
    expect(column.lastElementChild).toBe(action);
    expect(action.previousElementSibling).toContainElement(screen.getByRole("combobox", { name: "Timer" }));
  });

  it("draws no Actions row of its own: the panel's actions slot has nothing left, so there is no empty band", () => {
    const { container } = draw();
    const body = container.querySelector("[data-panel-body]")!;
    expect(Array.from(body.children).map((part) => part.getAttribute("data-section"))).not.toContain("actions");
    // The only action row on the page is the one inside the Timer column.
    expect(container.querySelectorAll("[data-section=actions]")).toHaveLength(1);
    expect(fieldOf("Timer").querySelector("[data-section=actions]")).not.toBeNull();
  });

  it("keeps the note next to the button, in the Timer column, and points the button at it", () => {
    draw();
    expect(screen.queryByText(/Refresh uses the saved settings/)).toBeNull();
    expect(refresh()).not.toHaveAttribute("aria-describedby");
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "192.0.2.10" } });
    const note = screen.getByText("Refresh uses the saved settings. Save configuration first to try the new ones.");
    expect(note).toHaveAttribute("id", "propresenter-test-saved-note");
    expect(within(fieldOf("Timer")).getByText(/Refresh uses the saved settings/)).toBe(note);
    expect(refresh().getAttribute("aria-describedby")).toContain("propresenter-test-saved-note");
  });

  it("keeps its look and its busy and disabled behaviour", () => {
    const onRefreshTimers = vi.fn();
    const { unmount } = draw({ onRefreshTimers });
    expect(refresh()).toHaveAttribute("data-tone", "presentation");
    expect(refresh()).toHaveAttribute("data-variant", "tinted");
    fireEvent.click(refresh());
    expect(onRefreshTimers).toHaveBeenCalledTimes(1);
    unmount();
    draw({ pendingOperation: "refresh" });
    expect(screen.getByRole("button", { name: "Refreshing…" })).toBeDisabled();
  });
});
