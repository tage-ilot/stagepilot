import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ProPresenterSettingsInput, ProPresenterStatusResponse } from "../types";
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
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
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
    await user.click(screen.getByRole("button", { name: "Save configuration" }));

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

    await user.click(screen.getByRole("button", { name: "Save configuration" }));
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
    expect(screen.getByRole("button", { name: "Save configuration" })).toHaveAttribute("data-variant", "primary");
    expect(screen.queryByRole("button", { name: "Save settings" })).not.toBeInTheDocument();
    for (const name of ["Test connection", "Refresh timers and Looks"]) {
      const button = screen.getByRole("button", { name });
      expect(button).toHaveAttribute("data-variant", "tinted");
      expect(button).toHaveAttribute("data-tone", "presentation");
    }
    expect(screen.getByLabelText("ProPresenter Look")).toHaveValue("look-default");
    expect(screen.getByLabelText("Timeout (seconds)")).toHaveValue("3");
  });

  it("shows the anatomy copy: kicker, Presentation title, one sentence and a visible Look label", () => {
    render(
      <ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} propresenter={status} />,
    );
    expect(screen.getByText("Connection configuration")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Presentation" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "General configuration" })).not.toBeInTheDocument();
    const look = screen.getByRole("combobox", { name: "ProPresenter Look" });
    expect(document.querySelector(`label[for="${look.id}"]`)).toHaveTextContent(/^ProPresenter Look$/);
  });

  const renderPanel = (props: Partial<React.ComponentProps<typeof ProPresenterSetupPanel>> = {}) =>
    render(
      <ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} propresenter={status} {...props} />,
    );

  it("starts clean with a dimmed Save configuration and a reason beside it", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });
    const bar = document.querySelector("[data-save-bar]");
    expect(bar).toHaveAttribute("data-state", "clean");
    expect(screen.getByText("All changes saved")).toBeInTheDocument();
    const save = screen.getByRole("button", { name: "Save configuration" });
    expect(save).toHaveAttribute("aria-disabled", "true");
    await user.click(save);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("holds edits: count, held markers on the edited fields, nothing sent until Save", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });
    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "192.0.2.10");
    await user.selectOptions(screen.getByRole("combobox", { name: "ProPresenter Look" }), "look-worship");
    expect(onSave).not.toHaveBeenCalled();
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "held");
    expect(screen.getByText(/2 changes/)).toBeInTheDocument();
    expect(screen.getAllByText("Not saved yet").length).toBeGreaterThanOrEqual(2);
    expect(document.querySelectorAll('[data-field][data-state="held"]')).toHaveLength(2);
  });

  it("a changed countdown timer is a held change and is sent with Save", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({
      onSave,
      propresenter: { ...status, timers: [...status.timers, { id: "t2", name: "Alternate Timer", index: 1, is_countdown: true, state: "stopped" }] },
    });
    await user.selectOptions(screen.getByRole("combobox", { name: /Detected timer/i }), "Alternate Timer");
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "held");
    expect(screen.getByText(/1 change/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ timer_name: "Alternate Timer" }));
  });

  it("sends a payload byte-identical to the old Save settings payload", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });
    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "  192.0.2.10  ");
    await user.clear(screen.getByLabelText("Port"));
    await user.type(screen.getByLabelText("Port"), "1026");
    await user.clear(screen.getByLabelText("Timeout (seconds)"));
    await user.type(screen.getByLabelText("Timeout (seconds)"), "4.5");
    await user.selectOptions(screen.getByRole("combobox", { name: "ProPresenter Look" }), "");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(onSave.mock.calls[0]?.[0])).toBe(
      JSON.stringify({ host: "192.0.2.10", port: 1026, timer_name: "Song Countdown", look_id: null, request_timeout_seconds: 4.5 }),
    );
  });

  it("Discard restores the saved values and clears the held state", async () => {
    const user = userEvent.setup();
    renderPanel();
    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "192.0.2.10");
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(screen.getByLabelText("Host")).toHaveValue("127.0.0.1");
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "clean");
  });

  it("explains an invalid edit and does not send", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({ onSave });
    await user.clear(screen.getByLabelText("Port"));
    await user.type(screen.getByLabelText("Port"), "99999");
    expect(screen.getByText(/port from 1 to 65535/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).not.toHaveBeenCalled();
  });

  it("shows saving, then Couldn't save with Try again that re-sends the same payload", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    const base = {
      error: null as string | null, message: null, onRefreshTimers: vi.fn(), onSave, onTest: vi.fn(), propresenter: status,
    };
    const { rerender } = render(<ProPresenterSetupPanel {...base} pendingOperation={null} />);
    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "192.0.2.10");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    rerender(<ProPresenterSetupPanel {...base} pendingOperation="save" />);
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "saving");
    expect(screen.getByText("Saving…")).toBeInTheDocument();
    rerender(<ProPresenterSetupPanel {...base} error="Request failed" pendingOperation={null} />);
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "error");
    expect(screen.getByText("Couldn't save")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(onSave).toHaveBeenCalledTimes(2);
    expect(onSave.mock.calls[1]?.[0]).toEqual(onSave.mock.calls[0]?.[0]);
  });

  it("keeps Test connection and Refresh as actions outside the bar and keeps the Look helper text", () => {
    renderPanel();
    const bar = document.querySelector("[data-save-bar]") as HTMLElement;
    expect(bar.contains(screen.getByRole("button", { name: "Test connection" }))).toBe(false);
    expect(bar.contains(screen.getByRole("button", { name: "Refresh timers and Looks" }))).toBe(false);
    expect(screen.getByText("Selecting a Look does not change Presentation until you save these settings.")).toBeInTheDocument();
  });

  it("shows one plain alert with Try again when the load failed, and recovers when the load works", async () => {
    const onRetryLoad = vi.fn();
    const props = { message: null, onRefreshTimers: vi.fn(), onSave: vi.fn(), onTest: vi.fn(), pendingOperation: null, onRetryLoad } as const;
    const view = render(
      <ProPresenterSetupPanel {...props} error="Can't reach StagePilot. Check that it is running, then try again." propresenter={null} />,
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByRole("alert")).toHaveTextContent("Can't reach StagePilot");
    expect(screen.queryByText("Loading Presentation configuration…")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(onRetryLoad).toHaveBeenCalledOnce();
    view.rerender(<ProPresenterSetupPanel {...props} error={null} propresenter={status} />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Try again" })).not.toBeInTheDocument();
  });

  it("keeps the form and does not offer a load retry when a save failed", () => {
    render(
      <ProPresenterSetupPanel error="StagePilot hit an error. Try again." message={null} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} onRetryLoad={vi.fn()} pendingOperation={null} propresenter={status} />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("StagePilot hit an error. Try again.");
    expect(screen.queryByRole("button", { name: "Try again" })).not.toBeInTheDocument();
    expect(screen.getByDisplayValue("1025")).toBeInTheDocument();
  });
});

describe("Presentation hold-until-saved conformance (guard)", () => {
  const mount = (extra: { onSave?: (settings: ProPresenterSettingsInput) => void; onDirtyChange?: (dirty: boolean) => void; onTest?: () => void } = {}) => render(
    <ProPresenterSetupPanel
      error={null}
      message={null}
      onDirtyChange={extra.onDirtyChange}
      onRefreshTimers={vi.fn()}
      onSave={extra.onSave ?? vi.fn()}
      onTest={extra.onTest ?? vi.fn()}
      pendingOperation={null}
      propresenter={status}
    />,
  );
  const bar = () => document.querySelector("[data-save-bar]")!;
  const held: Array<{ label: string; value: string; saved: string; payload: Record<string, unknown> }> = [
    { label: "Host", value: "192.0.2.10", saved: "127.0.0.1", payload: { host: "192.0.2.10" } },
    { label: "Port", value: "1100", saved: "1025", payload: { port: 1100 } },
    { label: "Timeout (seconds)", value: "7", saved: "3", payload: { request_timeout_seconds: 7 } },
    { label: "ProPresenter Look", value: "look-worship", saved: "look-default", payload: { look_id: "look-worship" } },
  ];
  it.each(held)("$label is held: marked, counted, not saved on blur, flagged dirty, restored by Discard, saved only by Save configuration", ({ label, value, saved, payload }) => {
    const onSave = vi.fn();
    const onDirtyChange = vi.fn();
    mount({ onSave, onDirtyChange });
    const control = screen.getByLabelText(label);
    fireEvent.change(control, { target: { value } });
    fireEvent.blur(control);
    expect(onSave).not.toHaveBeenCalled();
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(1);
    expect(bar()).toHaveTextContent("1 change waiting. Not saved yet.");
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);
    fireEvent.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining(payload));
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect((screen.getByLabelText(label) as HTMLInputElement).value).toBe(saved);
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(0);
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });

  it("Test connection and Refresh are explicit actions and say they use the saved settings while edits are waiting", () => {
    const onTest = vi.fn();
    mount({ onTest });
    expect(screen.queryByText(/use the saved settings/)).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "192.0.2.10" } });
    expect(screen.getByText(/Test connection and Refresh use the saved settings/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    expect(onTest).toHaveBeenCalledOnce();
  });
});
