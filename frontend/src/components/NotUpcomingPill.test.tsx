import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { NotUpcomingPill } from "./NotUpcomingPill";

describe("NotUpcomingPill", () => {
  it("says the current upcoming service is not selected and offers Current Upcoming next to it", async () => {
    const onBack = vi.fn();
    render(<NotUpcomingPill canOperate date="2026-11-29" onBack={onBack} />);
    expect(screen.getByTestId("not-upcoming-pill")).toHaveTextContent("Current upcoming service not selected");
    const back = screen.getByRole("button", { name: "Current Upcoming" });
    expect(back.nextElementSibling).toBe(screen.getByTestId("not-upcoming-pill"));
    await userEvent.click(back);
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("is a warning, not an error, and keeps that colour inside a connection's panel", () => {
    render(<NotUpcomingPill canOperate date="2026-11-29" onBack={vi.fn()} />);
    const pill = screen.getByTestId("not-upcoming-pill");
    expect(pill).toHaveAttribute("data-tone", "warning");
    expect(pill.className).not.toMatch(/danger/);
  });

  it("can match the height of the Calendar button beside it (touch size)", () => {
    const { rerender } = render(<NotUpcomingPill canOperate date="2026-11-29" onBack={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Current Upcoming" })).toHaveAttribute("data-size", "control");
    rerender(<NotUpcomingPill canOperate date="2026-11-29" onBack={vi.fn()} size="touch" />);
    expect(screen.getByRole("button", { name: "Current Upcoming" })).toHaveAttribute("data-size", "touch");
  });

  it("waits while a change is loading", () => {
    render(<NotUpcomingPill canOperate date="2026-11-29" onBack={vi.fn()} pending />);
    expect(screen.getByRole("button", { name: "Current Upcoming" })).toBeDisabled();
  });

  it("has no button for a session that cannot operate", () => {
    render(<NotUpcomingPill canOperate={false} date="2026-11-29" onBack={vi.fn()} />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByTestId("not-upcoming-pill")).toBeInTheDocument();
  });
});
