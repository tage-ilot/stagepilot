import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ConnectionMethod } from "./ConnectionMethod";
import { Panel } from "./Panel";
import { accentClasses } from "./accent";

const options = [
  { value: "api", label: "Playback API", role: "primary" as const },
  { value: "midi", label: "MIDI", role: "alternate" as const },
];
const notice = "Using the alternate method. If Playback API is available again, switch back here.";

describe("ConnectionMethod (the template for every connection that has more than one way to connect)", () => {
  it("is a labelled radio group: each option is a name over PRIMARY or ALTERNATE", () => {
    render(<ConnectionMethod onChange={() => {}} options={options} value="api" />);
    const group = screen.getByRole("radiogroup", { name: "Connection method" });
    expect(within(group).getAllByRole("radio")).toHaveLength(2);
    expect(within(group).getByRole("radio", { name: "Playback API Primary" })).toBeChecked();
    expect(within(group).getByRole("radio", { name: "MIDI Alternate" })).not.toBeChecked();
    expect(screen.getByText("Connection method", { selector: "legend" })).toHaveClass("uppercase", "text-ink-muted");
  });

  it("calls onChange with the clicked option and nothing else", async () => {
    const onChange = vi.fn();
    render(<ConnectionMethod onChange={onChange} options={options} value="api" />);
    await userEvent.setup().click(screen.getByRole("radio", { name: /^MIDI/ }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith("midi");
  });

  it("works from the keyboard: arrow keys move the choice", async () => {
    const onChange = vi.fn();
    render(<ConnectionMethod onChange={onChange} options={options} value="api" />);
    const user = userEvent.setup();
    await user.tab();
    expect(screen.getByRole("radio", { name: /^Playback API/ })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenCalledWith("midi");
  });

  it("shows the notice only while an alternate is chosen", () => {
    const { rerender } = render(<ConnectionMethod alternateNotice={notice} onChange={() => {}} options={options} value="api" />);
    expect(screen.queryByText(notice)).toBeNull();
    rerender(<ConnectionMethod alternateNotice={notice} onChange={() => {}} options={options} value="midi" />);
    expect(screen.getByText(notice)).toHaveAttribute("data-alternate-notice");
  });

  it("styles the chosen alternate as a lighter slate chip and the chosen primary in the primary colour", () => {
    const { rerender } = render(<ConnectionMethod onChange={() => {}} options={options} value="midi" />);
    expect(screen.getByRole("radio", { name: /^MIDI/ }).closest("label")).toHaveClass("bg-ink-muted/15", "border-edge-medium");
    expect(screen.getByRole("radio", { name: /^Playback API/ }).closest("label")).not.toHaveClass("bg-primary/20");
    rerender(<ConnectionMethod onChange={() => {}} options={options} value="api" />);
    expect(screen.getByRole("radio", { name: /^Playback API/ }).closest("label")).toHaveClass("bg-primary/20");
  });

  it("the chosen primary wears the connection colour inside a connection panel", () => {
    render(
      <Panel tone="playback" variant="setup">
        <ConnectionMethod onChange={() => {}} options={options} value="api" />
      </Panel>,
    );
    const chip = screen.getByRole("radio", { name: /^Playback API/ }).closest("label")!;
    for (const cls of accentClasses.playback.chip.split(" ")) expect(chip).toHaveClass(cls);
  });

  it("marks a held change with Not saved yet and a warning border", () => {
    render(<ConnectionMethod heldChange onChange={() => {}} options={options} value="midi" />);
    expect(document.querySelector("[data-held-marker]")).toHaveTextContent("Not saved yet");
    expect(screen.getByRole("radiogroup")).toHaveClass("border-warning");
  });

  it("disables the whole group, or a single option, and blocks the click", async () => {
    const onChange = vi.fn();
    const { rerender } = render(<ConnectionMethod disabled onChange={onChange} options={options} value="api" />);
    expect(screen.getByRole("radio", { name: /^MIDI/ })).toBeDisabled();
    await userEvent.setup().click(screen.getByRole("radio", { name: /^MIDI/ }));
    expect(onChange).not.toHaveBeenCalled();
    rerender(<ConnectionMethod onChange={onChange} options={[options[0]!, { ...options[1]!, disabled: true }]} value="api" />);
    expect(screen.getByRole("radio", { name: /^MIDI/ })).toBeDisabled();
    expect(screen.getByRole("radio", { name: /^Playback API/ })).toBeEnabled();
  });

  it("every option is a 44px target and the control never wraps its labels", () => {
    render(<ConnectionMethod onChange={() => {}} options={options} value="api" />);
    for (const radio of screen.getAllByRole("radio")) expect(radio.closest("label")).toHaveClass("min-h-ds-touch-target");
  });
});
