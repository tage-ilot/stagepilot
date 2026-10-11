import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ProPresenterStatusResponse } from "../types";
import { ProPresenterSetupPanel } from "./ProPresenterSetupPanel";

const status: ProPresenterStatusResponse = {
  enabled: true,
  host: "127.0.0.1",
  port: 1025,
  timer_name: "Song Countdown",
  look_id: "look-default",
  request_timeout_seconds: 3,
  connection_status: "connected",
  detail: 'Connected; countdown timer "Song Countdown" is ready.',
  timers: [
    {
      id: "timer-uuid",
      name: "Song Countdown",
      index: 0,
      is_countdown: true,
      state: "stopped",
    },
  ],
  selected_timer_id: "timer-uuid",
  timer_found: true,
  looks: [
    { id: "look-default", name: "Default", index: 0 },
    { id: "look-worship", name: "Worship", index: 1 },
  ],
  current_look_id: "look-default",
  look_found: true,
  last_checked_at: "2026-07-14T12:00:00Z",
};

describe("ProPresenterSetupPanel", () => {
  it("saves validated session settings and exposes connection tools", async () => {
    const onSave = vi.fn();
    const onTest = vi.fn();
    const onRefreshTimers = vi.fn();
    const user = userEvent.setup();

    render(
      <ProPresenterSetupPanel
        error={null}
        message={null}
        onRefreshTimers={onRefreshTimers}
        onSave={onSave}
        onTest={onTest}
        pendingOperation={null}
        propresenter={status}
      />,
    );

    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "192.168.4.40");
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await user.click(screen.getByRole("button", { name: "Refresh timers and Looks" }));

    expect(onSave).toHaveBeenCalledWith({
      host: "192.168.4.40",
      port: 1025,
      timer_name: "Song Countdown",
      look_id: "look-default",
      request_timeout_seconds: 3,
    });
    expect(onTest).toHaveBeenCalledOnce();
    expect(onRefreshTimers).toHaveBeenCalledOnce();
    expect(screen.getAllByText("Song Countdown").length).toBeGreaterThan(0);
  });

  it("distinguishes an API connection from a missing timer", () => {
    render(
      <ProPresenterSetupPanel
        error={null}
        message={null}
        onRefreshTimers={vi.fn()}
        onSave={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        propresenter={{
          ...status,
          timer_found: false,
          selected_timer_id: null,
          detail: 'API connected, but timer "Song Countdown" was not found.',
        }}
      />,
    );

    expect(screen.getAllByText("Connected")).toHaveLength(2);
    expect(screen.getByText("Not found")).toBeInTheDocument();
    expect(screen.getByText(/API connected, but timer/i)).toBeInTheDocument();
  });

  it("uses the detected timer dropdown as the only timer input", () => {
    render(
      <ProPresenterSetupPanel
        error={null}
        message={null}
        onRefreshTimers={vi.fn()}
        onSave={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        propresenter={status}
      />,
    );

    expect(screen.getByText("Detected timer")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: /Detected timer/i })).toHaveValue("Song Countdown");
    expect(screen.queryByText("Detected timers")).not.toBeInTheDocument();
  });

  it("saves real connection settings before the plugin is running", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();

    render(
      <ProPresenterSetupPanel
        error={null}
        message={null}
        onRefreshTimers={vi.fn()}
        onSave={onSave}
        onTest={vi.fn()}
        pendingOperation={null}
        propresenter={{ ...status, enabled: false, connection_status: "disconnected" }}
      />,
    );

    expect(screen.queryByLabelText("Timer output")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save settings" }));

    expect(onSave).toHaveBeenCalledWith({
      host: "127.0.0.1",
      port: 1025,
      timer_name: "Song Countdown",
      look_id: "look-default",
      request_timeout_seconds: 3,
    });
  });

  it("allows a non-countdown timer to be selected for conversion when cued", async () => {
    const user = userEvent.setup();
    render(
      <ProPresenterSetupPanel
        error={null}
        message={null}
        onRefreshTimers={vi.fn()}
        onSave={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        propresenter={{
          ...status,
          timers: [
            ...status.timers,
            {
              id: "timer-other",
              name: "Alternate Timer",
              index: 1,
              is_countdown: false,
              state: "stopped",
            },
          ],
        }}
      />,
    );

    const option = screen.getByRole("option", {
      name: "Alternate Timer (converted when cued)",
    });
    expect(option).not.toBeDisabled();
    await user.selectOptions(
      screen.getByRole("combobox", { name: /Detected timer/i }),
      "Alternate Timer",
    );
    expect(screen.getByRole("combobox", { name: /Detected timer/i })).toHaveValue(
      "Alternate Timer",
    );
  });

  it("stages a Look change and applies it only when settings are saved", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    render(
      <ProPresenterSetupPanel
        error={null}
        message={null}
        onRefreshTimers={vi.fn()}
        onSave={onSave}
        onTest={vi.fn()}
        pendingOperation={null}
        propresenter={status}
      />,
    );

    await user.selectOptions(screen.getByRole("combobox", { name: "ProPresenter Look" }), "look-worship");
    expect(onSave).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Save settings" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ look_id: "look-worship" }));
  });

  it("renders on the setup panel primitives with tokenised buttons", () => {
    const { container } = render(
      <ProPresenterSetupPanel
        error={null}
        message={null}
        onRefreshTimers={vi.fn()}
        onSave={vi.fn()}
        onTest={vi.fn()}
        pendingOperation={null}
        propresenter={status}
      />,
    );

    expect(container.querySelector("#propresenter-configuration")).toHaveAttribute("data-variant", "setup");
    expect(screen.getByRole("button", { name: "Save settings" })).toHaveAttribute("data-variant", "primary");
    for (const name of ["Test connection", "Refresh timers and Looks"]) {
      const button = screen.getByRole("button", { name });
      expect(button).toHaveAttribute("data-variant", "tinted");
      expect(button).toHaveAttribute("data-tone", "presentation");
    }
    expect(screen.getByLabelText("ProPresenter Look")).toHaveValue("look-default");
    expect(screen.getByLabelText("Timeout (seconds)")).toHaveValue("3");
  });

  it("shows the anatomy copy: kicker, Presentation title, one sentence, Basic heading and a visible Look label", () => {
    render(
      <ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} propresenter={status} />,
    );
    expect(screen.getByText("Connection configuration")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Presentation" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Basic" })).toBeInTheDocument();
    const look = screen.getByRole("combobox", { name: "ProPresenter Look" });
    expect(document.querySelector(`label[for="${look.id}"]`)).toHaveTextContent(/^ProPresenter Look$/);
  });
});
