import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ProPresenterStatusResponse } from "../types";
import { ProPresenterSetupPanel } from "./ProPresenterSetupPanel";

// Presentation settings: Timer and Look on one line, each listing the real choices and marking the current one (no "Current Look"
// option, no placeholder option), and no Timeout field (the saved timeout is kept and sent back unchanged).

const status: ProPresenterStatusResponse = {
  enabled: true,
  host: "127.0.0.1",
  port: 1025,
  timer_name: "Song Countdown",
  look_id: null,
  request_timeout_seconds: 7,
  connection_status: "connected",
  detail: null,
  timers: [
    { id: "t1", name: "Song Countdown", index: 0, is_countdown: true, state: "stopped" },
    { id: "t2", name: "Alternate Timer", index: 1, is_countdown: true, state: "stopped" },
    { id: "t3", name: "Clock", index: 2, is_countdown: false, state: "stopped" },
  ],
  selected_timer_id: "t1",
  timer_found: true,
  looks: [
    { id: "look-default", name: "Default", index: 0 },
    { id: "look-worship", name: "Worship", index: 1 },
  ],
  current_look_id: "look-default",
  look_found: true,
  last_checked_at: "2026-07-14T12:00:00Z",
};

const panel = (propresenter: ProPresenterStatusResponse = status, onSave = vi.fn()) => {
  render(<ProPresenterSetupPanel error={null} message={null} onRefreshTimers={vi.fn()} onSave={onSave} onTest={vi.fn()} pendingOperation={null} propresenter={propresenter} />);
  return {
    onSave,
    timer: screen.getByRole("combobox", { name: "Timer" }) as HTMLSelectElement,
    look: screen.getByRole("combobox", { name: "Look" }) as HTMLSelectElement,
  };
};
const optionTexts = (select: HTMLSelectElement) => Array.from(select.options).filter((option) => !option.hidden).map((option) => option.textContent);
const save = () => screen.getByRole("button", { name: "Save configuration" });

describe("Presentation settings: no Timeout field", () => {
  it("shows no Timeout field, caption or error anywhere", () => {
    panel();
    expect(screen.queryByLabelText(/timeout/i)).toBeNull();
    expect(screen.queryByText(/timeout/i)).toBeNull();
  });

  it("sends the saved timeout back unchanged when other fields are saved (a round trip)", async () => {
    const { onSave } = panel();
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "192.0.2.10" } });
    fireEvent.click(save());
    expect(onSave).toHaveBeenCalledExactlyOnceWith({ host: "192.0.2.10", port: 1025, timer_name: "Song Countdown", look_id: null, request_timeout_seconds: 7 });
  });

  it("sends a timeout that is not a whole number back as it was saved", async () => {
    const { onSave } = panel({ ...status, request_timeout_seconds: 4.5 });
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "192.0.2.10" } });
    fireEvent.click(save());
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ request_timeout_seconds: 4.5 }));
  });

  it("does not count the timeout as a change and never marks it held", () => {
    panel();
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "clean");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(0);
  });
});

describe("Presentation settings: Timer and Look on one line", () => {
  it("the two fields share one pair grid (side by side from 30rem of panel width, stacked below it, so stacked at 360 px)", () => {
    const { timer, look } = panel();
    const grid = timer.closest(".section-fields");
    expect(grid).toHaveAttribute("data-columns", "pair");
    expect(grid).toHaveClass("grid", "gap-4");
    expect(look.closest(".section-fields")).toBe(grid);
    expect(grid!.querySelectorAll("[data-field]")).toHaveLength(2);
    const css = readFileSync(resolve(__dirname, "../index.css"), "utf8");
    expect(css).toMatch(/@container \(min-width: 30rem\) \{\s*\.section-fields\[data-columns="pair"\] \{\s*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\);/);
  });

  it("the captions are exactly Timer and Look, Timer first", () => {
    const { timer, look } = panel();
    expect(document.querySelector(`label[for="${timer.id}"]`)).toHaveTextContent(/^Timer$/);
    expect(document.querySelector(`label[for="${look.id}"]`)).toHaveTextContent(/^Look$/);
    expect(timer.compareDocumentPosition(look) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.queryByText("Detected timer")).toBeNull();
    expect(screen.queryByText("ProPresenter Look")).toBeNull();
  });
});

describe("Presentation settings: Look", () => {
  it("lists the Looks by name, the one ProPresenter shows reads (current) and is selected, and there is no Current Look option", () => {
    const { look } = panel();
    expect(optionTexts(look)).toEqual(["Default (current)", "Worship"]);
    expect(look).toHaveDisplayValue("Default (current)");
    expect(screen.queryByRole("option", { name: "Current Look" })).toBeNull();
  });

  it("saving with the current Look selected saves look_id empty, exactly as Current Look did", () => {
    const { onSave } = panel();
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "192.0.2.10" } });
    fireEvent.click(save());
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ look_id: null }));
  });

  it("choosing another Look is a held change and saves that Look's id; choosing the current one again saves it empty", async () => {
    const user = userEvent.setup();
    const { onSave, look } = panel();
    await user.selectOptions(look, "look-worship");
    expect(onSave).not.toHaveBeenCalled();
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "held");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(1);
    expect(screen.getByText(/1 change/)).toBeInTheDocument();
    await user.click(save());
    expect(onSave).toHaveBeenLastCalledWith(expect.objectContaining({ look_id: "look-worship" }));
    await user.selectOptions(look, "Default (current)");
    expect(document.querySelectorAll("[data-held-marker]")).toHaveLength(0);
  });

  it("a saved Look that ProPresenter still has is the selected one, and (current) stays on the Look ProPresenter shows", () => {
    const { look } = panel({ ...status, look_id: "look-worship" });
    expect(look).toHaveDisplayValue("Worship");
    expect(optionTexts(look)).toEqual(["Default (current)", "Worship"]);
  });

  it("a saved Look that no longer exists reads <id> (unavailable) and is selected", () => {
    const { look } = panel({ ...status, look_id: "look-gone" });
    expect(look).toHaveDisplayValue("look-gone (unavailable)");
    expect(optionTexts(look)).toEqual(["look-gone (unavailable)", "Default (current)", "Worship"]);
  });

  it("with no Looks known the select is disabled and says Connect to ProPresenter first", () => {
    const { look } = panel({ ...status, connection_status: "disconnected", looks: [], current_look_id: null, look_found: false });
    expect(look).toBeDisabled();
    expect(look).toHaveAccessibleDescription(/Connect to ProPresenter first/);
    expect(screen.queryByText("Selecting a Look does not change Presentation until you save these settings.")).toBeNull();
  });

  it("keeps the hint that choosing a Look changes nothing until Save", () => {
    const { look } = panel();
    expect(look).toBeEnabled();
    expect(look).toHaveAccessibleDescription(/Selecting a Look does not change Presentation until you save these settings\./);
  });
});

describe("Presentation settings: Timer", () => {
  it("lists the detected timers by name; the saved one reads (current) and is selected; converted timers keep their note", () => {
    const { timer } = panel();
    expect(optionTexts(timer)).toEqual(["Song Countdown (current)", "Alternate Timer", "Clock (converted when cued)"]);
    expect(timer).toHaveDisplayValue("Song Countdown (current)");
  });

  it("choosing another timer is a held change and the (current) mark stays on the saved one", async () => {
    const user = userEvent.setup();
    const { timer, onSave } = panel();
    await user.selectOptions(timer, "Alternate Timer");
    expect(optionTexts(timer)).toContain("Song Countdown (current)");
    expect(screen.getByText(/1 change/)).toBeInTheDocument();
    await user.click(save());
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ timer_name: "Alternate Timer", request_timeout_seconds: 7 }));
  });

  it("a saved timer that is no longer found reads <name> (unavailable) and is selected", () => {
    const { timer } = panel({ ...status, timer_name: "Old Timer", timer_found: false, selected_timer_id: null });
    expect(timer).toHaveDisplayValue("Old Timer (unavailable)");
    expect(optionTexts(timer)).not.toContain("Old Timer (current)");
  });

  it("with no timer configured nothing is marked and the select shows the Choose a timer hint, which is not a choice", () => {
    const { timer } = panel({ ...status, timer_name: "" });
    expect(optionTexts(timer).some((text) => /\(current\)/.test(text ?? ""))).toBe(false);
    expect(timer).toHaveDisplayValue("Choose a timer");
    const hint = Array.from(timer.options).find((option) => option.textContent === "Choose a timer")!;
    expect(hint.disabled).toBe(true);
    expect(hint.hidden).toBe(true);
  });
});
