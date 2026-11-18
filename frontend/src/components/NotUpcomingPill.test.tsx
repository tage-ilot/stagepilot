import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { NotUpcomingPill } from "./NotUpcomingPill";

describe("NotUpcomingPill", () => {
  it("reads the picked service's day and offers Back to upcoming next to it", async () => {
    const onBack = vi.fn();
    render(<NotUpcomingPill canOperate date="2026-11-29" onBack={onBack} />);
    expect(screen.getByTestId("not-upcoming-pill")).toHaveTextContent("Not the upcoming service: Sun, Nov 29");
    const back = screen.getByRole("button", { name: "Back to upcoming" });
    expect(back.previousElementSibling).toBe(screen.getByTestId("not-upcoming-pill"));
    await userEvent.click(back);
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("is a warning, not an error, and keeps that colour inside a connection's panel", () => {
    render(<NotUpcomingPill canOperate date="2026-11-29" onBack={vi.fn()} />);
    const pill = screen.getByTestId("not-upcoming-pill");
    expect(pill).toHaveAttribute("data-tone", "warning");
    expect(pill.className).not.toMatch(/danger/);
  });

  it("waits while a change is loading", () => {
    render(<NotUpcomingPill canOperate date="2026-11-29" onBack={vi.fn()} pending />);
    expect(screen.getByRole("button", { name: "Back to upcoming" })).toBeDisabled();
  });

  it("has no button for a session that cannot operate", () => {
    render(<NotUpcomingPill canOperate={false} date="2026-11-29" onBack={vi.fn()} />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByTestId("not-upcoming-pill")).toBeInTheDocument();
  });
});
