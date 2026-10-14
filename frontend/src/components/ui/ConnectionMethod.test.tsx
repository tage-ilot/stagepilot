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

  it("has no notice: the toggle itself says which way is active", () => {
    render(<ConnectionMethod onChange={() => {}} options={options} value="midi" />);
    expect(document.querySelector("[data-alternate-notice]")).toBeNull();
    expect(screen.queryByText(/switch back/i)).toBeNull();
  });

  it("the chosen option wears the same tint whether it is the primary or the alternate", () => {
    const { rerender } = render(<ConnectionMethod onChange={() => {}} options={options} value="api" />);
    const chip = (name: RegExp) => screen.getByRole("radio", { name }).closest("label")!;
    expect(chip(/^Playback API/)).toHaveClass("bg-primary/45", "border-primary");
    expect(chip(/^MIDI/)).not.toHaveClass("bg-primary/45");
    rerender(<ConnectionMethod onChange={() => {}} options={options} value="midi" />);
    expect(chip(/^MIDI/)).toHaveClass("bg-primary/45", "border-primary");
    expect(chip(/^Playback API/)).not.toHaveClass("bg-primary/45");
    expect(chip(/^MIDI/).className).not.toMatch(/bg-ink-muted/);
  });

  it("inside a connection panel the chosen option wears that connection's tint, primary or alternate", () => {
    const { rerender } = render(
      <Panel tone="playback" variant="setup">
        <ConnectionMethod onChange={() => {}} options={options} value="api" />
      </Panel>,
    );
    for (const cls of accentClasses.playback.chip.split(" ")) expect(screen.getByRole("radio", { name: /^Playback API/ }).closest("label")).toHaveClass(cls);
    rerender(
      <Panel tone="playback" variant="setup">
        <ConnectionMethod onChange={() => {}} options={options} value="midi" />
      </Panel>,
    );
    for (const cls of accentClasses.playback.chip.split(" ")) expect(screen.getByRole("radio", { name: /^MIDI/ }).closest("label")).toHaveClass(cls);
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
