import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ApplicationState, ProPresenterStatusResponse, SettingsResponse } from "../types";
import { BackendSetupPanel } from "./BackendSetupPanel";
import { ProPresenterSetupPanel } from "./ProPresenterSetupPanel";

const save = () => screen.getByRole("button", { name: "Save configuration" });

describe("Backend panel: failed save", () => {
  const saved = { timezone: "America/Los_Angeles", log_level: "INFO", server_port: 8765, lan_access: false, web_dashboard_pin_enabled: true };
  const build = (onSave = vi.fn()) => (
    <BackendSetupPanel state={{ plugins: {} } as unknown as ApplicationState} health={null} live={false} onClose={vi.fn()} error={null} message={null}
      pending={false} onSave={onSave} settings={{ settings: saved } as unknown as SettingsResponse} />
  );

  it("an invalid port: nothing sent, error under the field, field focused, input kept, error announced", () => {
    const onSave = vi.fn();
    render(build(onSave));
    const port = screen.getByLabelText("Server port");
    fireEvent.change(port, { target: { value: "70000" } });
    fireEvent.click(save());
    expect(onSave).not.toHaveBeenCalled();
    expect(port).toHaveAttribute("aria-invalid", "true");
    expect(port).toHaveFocus();
    expect(port).toHaveValue("70000");
    expect(screen.getByText("Enter a port from 1 to 65535.")).toBeInTheDocument();
  });

  it("focuses the FIRST invalid field in page order when two are invalid", () => {
    render(build());
    fireEvent.change(screen.getByLabelText("Timezone"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("Server port"), { target: { value: "0" } });
    fireEvent.click(save());
    expect(screen.getByLabelText("Timezone")).toHaveFocus();
  });

  it("no error is shown while typing, only after a refused save, and Discard clears it", () => {
    render(build());
    fireEvent.change(screen.getByLabelText("Server port"), { target: { value: "x" } });
    expect(screen.queryByText("Enter a port from 1 to 65535.")).not.toBeInTheDocument();
    fireEvent.click(save());
    expect(screen.getByText("Enter a port from 1 to 65535.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(screen.queryByText("Enter a port from 1 to 65535.")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Server port")).toHaveValue("8765");
  });
});

describe("Presentation panel: failed save", () => {
  const status = {
    enabled: true, host: "127.0.0.1", port: 1025, timer_name: "Song Countdown", look_id: null, request_timeout_seconds: 3,
    connection_status: "connected", detail: null, timers: [{ id: "t", name: "Song Countdown", index: 0, is_countdown: true, state: "stopped" }],
    selected_timer_id: "t", timer_found: true, looks: [], current_look_id: null, look_found: false, last_checked_at: null,
  } as unknown as ProPresenterStatusResponse;

  it("empty host and bad timeout: first (Host) focused, both errors under their fields, values kept, no API call", () => {
    const onSave = vi.fn();
    render(<ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={onSave} onTest={vi.fn()} pendingOperation={null} propresenter={status} />);
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("Timeout (seconds)"), { target: { value: "99" } });
    fireEvent.click(save());
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Host")).toHaveFocus();
    expect(screen.getByText("Enter a host.")).toBeInTheDocument();
    expect(screen.getByText("Enter a timeout above 0 and up to 60 seconds.")).toBeInTheDocument();
    expect(screen.getByLabelText("Timeout (seconds)")).toHaveValue("99");
  });

  it("a double click on a valid save sends one request while pending", () => {
    const onSave = vi.fn();
    const { rerender } = render(<ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={onSave} onTest={vi.fn()} pendingOperation={null} propresenter={status} />);
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "1026" } });
    fireEvent.click(save());
    rerender(<ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={onSave} onTest={vi.fn()} pendingOperation="save" propresenter={status} />);
    fireEvent.click(save());
    expect(onSave).toHaveBeenCalledTimes(1);
  });
});
