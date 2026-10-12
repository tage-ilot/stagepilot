import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { DashboardWidgetFrame } from "./DashboardWidgetFrame";

describe("DashboardWidgetFrame", () => {
  it("hides all movement controls outside edit mode", () => {
    render(
      <DashboardWidgetFrame editing={false} first id="service-plan" label="Service plan" last={false} onMove={vi.fn()}>
        <button type="button">Widget action</button>
      </DashboardWidgetFrame>,
    );
    expect(screen.queryByRole("button", { name: /Move Service plan/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Widget action" })).toBeEnabled();
  });

  it("exposes an accessible dedicated drag handle and keyboard ordering controls", async () => {
    const onMove = vi.fn();
    const user = userEvent.setup();
    render(
      <DashboardWidgetFrame editing first={false} id="service-plan" label="Service plan" last={false} onMove={onMove}>
        <div>Service content</div>
      </DashboardWidgetFrame>,
    );
    expect(screen.getByRole("button", { name: "Drag Service plan to a new dashboard position" })).toHaveClass("dashboard-widget-handle");
    await user.click(screen.getByRole("button", { name: "Move Service plan earlier" }));
    await user.click(screen.getByRole("button", { name: "Move Service plan later" }));
    expect(onMove).toHaveBeenNthCalledWith(1, "service-plan", -1);
    expect(onMove).toHaveBeenNthCalledWith(2, "service-plan", 1);
  });

  it("disables boundary movement", () => {
    render(
      <DashboardWidgetFrame editing first id="events" label="Recent Event Stream" last onMove={vi.fn()}>
        Events
      </DashboardWidgetFrame>,
    );
    expect(screen.getByRole("button", { name: "Move Recent Event Stream earlier" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move Recent Event Stream later" })).toBeDisabled();
  });

  it("fails safe on touch: the drag handle owns its gesture and every control has a 44px coarse target", () => {
    render(
      <DashboardWidgetFrame editing first={false} id="service-plan" label="Service plan" last={false} onMove={vi.fn()}>
        <div>Service content</div>
      </DashboardWidgetFrame>,
    );
    const handle = screen.getByRole("button", { name: "Drag Service plan to a new dashboard position" });
    expect(handle).toHaveClass("touch-none", "coarse:h-ds-touch-target");
    for (const name of ["Move Service plan earlier", "Move Service plan later"]) {
      const arrow = screen.getByRole("button", { name });
      expect(arrow).toHaveClass("touch-manipulation", "coarse:h-ds-touch-target", "coarse:w-ds-touch-target", "motion-reduce:transition-none");
    }
  });
});
