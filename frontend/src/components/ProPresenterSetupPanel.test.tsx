import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ProPresenterSettingsInput, ProPresenterStatusResponse } from "../types";
import type { ProPresenterScanController } from "../hooks/useProPresenterScan";
import { ProPresenterSetupPanel } from "./ProPresenterSetupPanel";
import type { ScanState } from "./sections";

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
  it("saves validated session settings and exposes the refresh action", async () => {
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
    await user.click(screen.getByRole("button", { name: "Refresh timers and Looks" }));

    expect(onSave).toHaveBeenCalledWith({
      host: "192.168.4.40",
      port: 1025,
      timer_name: "Song Countdown",
      look_id: "look-default",
      request_timeout_seconds: 3,
    });
    expect(onTest).not.toHaveBeenCalled();
    expect(onRefreshTimers).toHaveBeenCalledOnce();
    expect(screen.getAllByText("Song Countdown").length).toBeGreaterThan(0);
  });

  it("remembers a saved host, and the arrow in the Host box offers the recent ones so nobody retypes", async () => {
    window.localStorage.clear();
    const user = userEvent.setup();
    const props = { error: null, message: null, onRefreshTimers: vi.fn(), onSave: vi.fn(), onTest: vi.fn(), pendingOperation: null, propresenter: status } as const;
    const { unmount } = render(<ProPresenterSetupPanel {...props} />);
    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "192.168.4.40");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    unmount();
    render(<ProPresenterSetupPanel {...props} />);
    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "10.1.1.1");
    expect(screen.queryByRole("button", { name: "Use 192.168.4.40" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Show recent and found addresses" }));
    await user.click(screen.getByRole("option", { name: "192.168.4.40" }));
    expect(screen.getByLabelText("Host")).toHaveValue("192.168.4.40");
    expect(screen.queryByRole("listbox")).toBeNull();
    window.localStorage.clear();
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

    expect(screen.getByText("Timer")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Timer" })).toHaveValue("Song Countdown");
    expect(screen.queryByText("Detected timers")).not.toBeInTheDocument();
    expect(screen.queryByText("Detected timer")).not.toBeInTheDocument();
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
      screen.getByRole("combobox", { name: "Timer" }),
      "Alternate Timer",
    );
    expect(screen.getByRole("combobox", { name: "Timer" })).toHaveValue(
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

    await user.selectOptions(screen.getByRole("combobox", { name: "Look" }), "look-worship");
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
    for (const name of ["Search", "Connect", "Refresh timers and Looks"]) {
      const button = screen.getByRole("button", { name });
      expect(button).toHaveAttribute("data-variant", "tinted");
      expect(button).toHaveAttribute("data-tone", "presentation");
    }
    expect(screen.getByLabelText("Look")).toHaveValue("");
    expect(screen.getByLabelText("Look")).toHaveDisplayValue("Default (current)");
    expect(screen.queryByLabelText("Timeout (seconds)")).toBeNull();
  });

  it("shows the anatomy copy: kicker, Presentation title, one sentence and a visible Look label", () => {
    render(
      <ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} propresenter={status} />,
    );
    expect(screen.getByText("Connection configuration")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Presentation" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "General configuration" })).not.toBeInTheDocument();
    const look = screen.getByRole("combobox", { name: "Look" });
    expect(document.querySelector(`label[for="${look.id}"]`)).toHaveTextContent(/^Look$/);
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
    await user.selectOptions(screen.getByRole("combobox", { name: "Look" }), "look-worship");
    expect(onSave).not.toHaveBeenCalled();
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "held");
    expect(screen.getByText(/2 changes/)).toBeInTheDocument();
    expect(screen.getAllByText("Not saved yet").length).toBeGreaterThanOrEqual(2);
    // The Host box (in the Live Link block) and the Look field both show the marker; the Host box also carries the warning border.
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(2);
    expect(document.querySelectorAll('[data-field][data-state="held"]')).toHaveLength(1);
    expect(screen.getByLabelText("Host")).toHaveClass("!border-warning");
    expect(screen.getByLabelText("Host")).toHaveAccessibleDescription("Not saved yet");
  });

  it("a changed countdown timer is a held change and is sent with Save", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    renderPanel({
      onSave,
      propresenter: { ...status, timers: [...status.timers, { id: "t2", name: "Alternate Timer", index: 1, is_countdown: true, state: "stopped" }] },
    });
    await user.selectOptions(screen.getByRole("combobox", { name: "Timer" }), "Alternate Timer");
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
    await user.clear(screen.getByLabelText("ProPresenter API port"));
    await user.type(screen.getByLabelText("ProPresenter API port"), "1026");
    await user.selectOptions(screen.getByRole("combobox", { name: "Look" }), "");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(onSave.mock.calls[0]?.[0])).toBe(
      JSON.stringify({ host: "192.0.2.10", port: 1026, timer_name: "Song Countdown", look_id: null, request_timeout_seconds: 3 }),
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
    await user.clear(screen.getByLabelText("ProPresenter API port"));
    await user.type(screen.getByLabelText("ProPresenter API port"), "99999");
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

  it("has no restart wording in any state and shows Connecting while a save applies", () => {
    const states: Array<Partial<React.ComponentProps<typeof ProPresenterSetupPanel>>> = [
      { propresenter: null },
      { propresenter: { ...status, enabled: false, connection_status: "disconnected" } },
      { propresenter: status },
      { propresenter: status, pendingOperation: "save" },
      { propresenter: status, error: "Host unreachable" },
      { propresenter: status, message: "ProPresenter session settings applied." },
    ];
    for (const props of states) {
      const view = renderPanel(props);
      expect(view.container.textContent ?? "").not.toMatch(/restart/i);
      expect(screen.queryByText(/then restart/i)).toBeNull();
      if (props.pendingOperation === "save") {
        expect(screen.getByText("Connecting to Presentation…")).toBeInTheDocument();
      } else {
        expect(screen.queryByText("Connecting to Presentation…")).toBeNull();
      }
      view.unmount();
    }
  });

  it("keeps Connect and Refresh as actions outside the bar and keeps the Look helper text", () => {
    renderPanel();
    const bar = document.querySelector("[data-save-bar]") as HTMLElement;
    expect(bar.contains(screen.getByRole("button", { name: "Connect" }))).toBe(false);
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
      propresenter={{ ...status, timers: [...status.timers, { id: "t2", name: "Alternate Timer", index: 1, is_countdown: true, state: "stopped" }] }}
    />,
  );
  const bar = () => document.querySelector("[data-save-bar]")!;
  const held: Array<{ label: string; value: string; saved: string; payload: Record<string, unknown> }> = [
    { label: "Host", value: "192.0.2.10", saved: "127.0.0.1", payload: { host: "192.0.2.10" } },
    { label: "ProPresenter API port", value: "1100", saved: "1025", payload: { port: 1100 } },
    { label: "Look", value: "look-worship", saved: "", payload: { look_id: "look-worship" } },
    { label: "Timer", value: "Alternate Timer", saved: "Song Countdown", payload: { timer_name: "Alternate Timer" } },
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

  it("Refresh is an explicit action that says it uses the saved settings while edits are waiting", () => {
    const onRefreshTimers = vi.fn();
    render(<ProPresenterSetupPanel error={null} message={null} onRefreshTimers={onRefreshTimers} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} propresenter={status} />);
    expect(screen.queryByText(/uses the saved settings/)).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "192.0.2.10" } });
    expect(screen.getByText(/Refresh uses the saved settings/)).toBeInTheDocument();
    expect(screen.queryByText(/Test connection/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh timers and Looks" }));
    expect(onRefreshTimers).toHaveBeenCalledOnce();
  });
});

describe("Presentation Connect and Scan (the pair that ends the Live Link row)", () => {
  const panel = (props: Partial<React.ComponentProps<typeof ProPresenterSetupPanel>> = {}) => render(
    <ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={vi.fn()} onTest={vi.fn()} pendingOperation={null} propresenter={status} {...props} />,
  );
  const scanner = (state: ScanState, over: Partial<ProPresenterScanController> = {}): ProPresenterScanController => ({ scan: state, error: null, starting: false, start: vi.fn(), cancel: vi.fn(), ...over });

  it("Scan then Connect end the row, in that order, and nothing is called Test connection", () => {
    panel();
    const row = screen.getByTestId("propresenter-live-link-line");
    // (The little arrow inside the Host box is not a tab stop and has no words: the row ENDS with Scan, then Connect.)
    expect(within(row).getAllByRole("button").filter((button) => button.tabIndex !== -1).map((button) => button.textContent)).toEqual(["Search", "Connect"]);
    expect(screen.queryByRole("button", { name: /Test connection/ })).toBeNull();
  });

  it("with ProPresenter saved and nothing held in Host or API, Connect only checks again (the request the old test button made) and says so", async () => {
    const onTest = vi.fn();
    const onSave = vi.fn();
    panel({ onTest, onSave });
    await userEvent.setup().click(screen.getByRole("button", { name: "Connect" }));
    expect(onTest).toHaveBeenCalledOnce();
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByTestId("propresenter-recheck")).toHaveTextContent("Checked again.");
  });

  it("with Host or API held, Connect saves the connection the way Save does and leaves a held Look held", async () => {
    const onTest = vi.fn();
    const onSave = vi.fn();
    const user = userEvent.setup();
    panel({ onSave, onTest });
    await user.clear(screen.getByLabelText("Host"));
    await user.type(screen.getByLabelText("Host"), "  192.0.2.10  ");
    await user.clear(screen.getByLabelText("ProPresenter API port"));
    await user.type(screen.getByLabelText("ProPresenter API port"), "11025");
    fireEvent.change(screen.getByLabelText("Look"), { target: { value: "look-worship" } });
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect(onTest).not.toHaveBeenCalled();
    expect(onSave).toHaveBeenCalledExactlyOnceWith({ host: "192.0.2.10", port: 11025, timer_name: "Song Countdown", look_id: "look-default", request_timeout_seconds: 3 });
    expect(screen.queryByTestId("propresenter-recheck")).toBeNull();
    // The Save bar still holds what Connect did not take.
    expect(screen.getByLabelText("Look")).toHaveValue("look-worship");
  });

  it("before ProPresenter is saved, Connect saves what the boxes hold (it is how the first computer is saved), the payload Save sends", async () => {
    const onSave = vi.fn();
    panel({ onSave, propresenter: { ...status, enabled: false, connection_status: "disconnected" } });
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
    await userEvent.setup().click(screen.getByRole("button", { name: "Connect" }));
    expect(onSave).toHaveBeenCalledExactlyOnceWith({ host: "127.0.0.1", port: 1025, timer_name: "Song Countdown", look_id: "look-default", request_timeout_seconds: 3 });
  });

  it("a bad API port is said in words and nothing is sent", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    panel({ onSave });
    await user.clear(screen.getByLabelText("ProPresenter API port"));
    await user.type(screen.getByLabelText("ProPresenter API port"), "99999");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("Enter the API port from 1 to 65535.");
  });

  it("Connect says Connecting… while its own request runs, and both buttons wait while the page is busy", () => {
    const { unmount } = panel({ pendingOperation: "test" });
    expect(screen.getByRole("button", { name: "Connecting…" })).toBeDisabled();
    unmount();
    panel({ pendingOperation: "refresh" });
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Search" })).toBeDisabled();
  });

  it("Scan asks the scanner, shows the progress, and cancel stops it", async () => {
    const start = vi.fn();
    const cancel = vi.fn();
    const user = userEvent.setup();
    const view = panel({ scanner: scanner({ state: "idle", candidates: [], current: 0, total: 0 }, { start, cancel }) });
    await user.click(screen.getByRole("button", { name: "Search" }));
    expect(start).toHaveBeenCalledOnce();
    view.unmount();
    panel({ scanner: scanner({ state: "scanning", candidates: [], current: 10, total: 255, elapsed: 1 }, { start, cancel }) });
    expect(screen.getByTestId("scan-progress")).toHaveTextContent("Checked 10 of 255 addresses");
    await user.click(screen.getByRole("button", { name: "Searching… (cancel)" }));
    expect(cancel).toHaveBeenCalledOnce();
  });

  it("one computer found fills Host and API (held, not saved); Connect then saves them", async () => {
    const onSave = vi.fn();
    panel({ onSave, scanner: scanner({ state: "found", candidates: [{ host: "192.0.2.21", port: 11025, label: "Front MacBook" }], current: 255, total: 255 }) });
    await waitFor(() => expect(screen.getByLabelText("Host")).toHaveValue("192.0.2.21"));
    expect(screen.getByLabelText("ProPresenter API port")).toHaveValue("11025");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(2);
    expect(onSave).not.toHaveBeenCalled();
    await userEvent.setup().click(screen.getByRole("button", { name: "Connect" }));
    expect(onSave).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ host: "192.0.2.21", port: 11025 }));
  });

  it("several found: choosing one fills Host and API and nothing else", async () => {
    const onSave = vi.fn();
    panel({ onSave, scanner: scanner({ state: "found", candidates: [{ host: "192.0.2.21", port: 1025, label: "Front MacBook" }, { host: "192.0.2.34", port: 11025, label: "Stage Mac" }], current: 255, total: 255 }) });
    expect(screen.getByLabelText("Host")).toHaveValue("127.0.0.1");
    await userEvent.setup().click(screen.getByRole("button", { name: /Stage Mac/ }));
    expect(screen.getByLabelText("Host")).toHaveValue("192.0.2.34");
    expect(screen.getByLabelText("ProPresenter API port")).toHaveValue("11025");
    expect(onSave).not.toHaveBeenCalled();
  });

  it("a scanned computer is in the Host box's list; choosing it fills Host and API (held, not saved); it can be edited before Connect saves", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    panel({ onSave, scanner: scanner({ state: "found", candidates: [{ host: "192.0.2.21", port: 1026, label: "Front MacBook" }, { host: "192.0.2.34", port: 11025, label: "Stage Mac" }], current: 255, total: 255 }) });
    await user.click(screen.getByRole("button", { name: "Show recent and found addresses" }));
    expect(within(screen.getByRole("group", { name: "Found" })).getAllByRole("option").map((option) => option.textContent)).toEqual(["Front MacBook (192.0.2.21)", "Stage Mac (192.0.2.34)"]);
    await user.click(screen.getByRole("option", { name: /Front MacBook/ }));
    expect(screen.getByLabelText("Host")).toHaveValue("192.0.2.21");
    expect(screen.getByLabelText("ProPresenter API port")).toHaveValue("1026");
    expect(onSave).not.toHaveBeenCalled();
    await user.type(screen.getByLabelText("Host"), "0");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect(onSave).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ host: "192.0.2.210", port: 1026 }));
  });

  it("no scan result, no Scanned group: a scan that found nothing or is still running lists only what worked", async () => {
    window.localStorage.setItem("stagepilot.recent-addresses.presentation", JSON.stringify(["10.9.9.9"]));
    const user = userEvent.setup();
    for (const state of ["scanning", "not_found", "idle"] as const) {
      const { unmount } = panel({ scanner: scanner({ state, candidates: [], current: 0, total: 0 }) });
      await user.click(screen.getByRole("button", { name: "Show recent and found addresses" }));
      expect(screen.queryByRole("group", { name: "Found" })).toBeNull();
      expect(screen.getByRole("option", { name: "10.9.9.9" })).toBeInTheDocument();
      unmount();
    }
  });

  it("Enter in the Host box with the list closed is Connect; with the list open it chooses and does not connect", async () => {
    const onSave = vi.fn();
    const user = userEvent.setup();
    panel({ onSave, scanner: scanner({ state: "found", candidates: [{ host: "192.0.2.21", port: 1025, label: "Front MacBook" }, { host: "192.0.2.34", port: 11025, label: "Stage Mac" }], current: 255, total: 255 }) });
    await user.click(screen.getByLabelText("Host"));
    await user.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(screen.getByLabelText("Host")).toHaveValue("192.0.2.34");
    expect(onSave).not.toHaveBeenCalled();
    await user.keyboard("{Enter}");
    expect(onSave).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ host: "192.0.2.34", port: 11025 }));
  });

  it("while the page is busy the Host box and its arrow wait", () => {
    panel({ pendingOperation: "refresh" });
    expect(screen.getByLabelText("Host")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Show recent and found addresses" })).toBeDisabled();
  });
});
