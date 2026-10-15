import { render, screen, waitFor, within } from "@testing-library/react";
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

  const pill = () => document.querySelector("[data-method-pill]") as HTMLElement;
  const selected = (name: RegExp) => screen.getByRole("radio", { name }).closest("label")!;

  it("the chosen highlight is one box that wears the same tint whether primary or alternate", () => {
    const { rerender } = render(<ConnectionMethod onChange={() => {}} options={options} value="api" />);
    expect(selected(/^Playback API/)).toHaveAttribute("data-selected", "true");
    expect(document.querySelectorAll("[data-method-pill]")).toHaveLength(1);
    expect(pill()).toHaveClass("bg-primary/45", "border-primary");
    rerender(<ConnectionMethod onChange={() => {}} options={options} value="midi" />);
    expect(selected(/^MIDI/)).toHaveAttribute("data-selected", "true");
    expect(selected(/^Playback API/)).toHaveAttribute("data-selected", "false");
    expect(document.querySelectorAll("[data-method-pill]")).toHaveLength(1);
    expect(pill()).toHaveClass("bg-primary/45", "border-primary");
    expect(pill().className).not.toMatch(/bg-ink-muted/);
  });

  it("the box is the same element after a change, so it can slide instead of disappearing", async () => {
    const { rerender } = render(<ConnectionMethod onChange={() => {}} options={options} value="api" />);
    const before = pill();
    // The slide is switched on one frame after mount, so the first paint never animates.
    await waitFor(() => expect(pill().className).toContain("duration-[220ms]"));
    rerender(<ConnectionMethod onChange={() => {}} options={options} value="midi" />);
    expect(pill()).toBe(before);
    expect(pill().className).toContain("duration-[220ms]");
    expect(pill().className).toContain("motion-reduce:transition-none");
  });

  it("inside a connection panel the chosen box wears that connection's tint, primary or alternate", () => {
    const { rerender } = render(
      <Panel tone="playback" variant="setup">
        <ConnectionMethod onChange={() => {}} options={options} value="api" />
      </Panel>,
    );
    for (const cls of accentClasses.playback.chip.split(" ")) expect(pill()).toHaveClass(cls);
    rerender(
      <Panel tone="playback" variant="setup">
        <ConnectionMethod onChange={() => {}} options={options} value="midi" />
      </Panel>,
    );
    for (const cls of accentClasses.playback.chip.split(" ")) expect(pill()).toHaveClass(cls);
  });

  it("an option with its own chip class gives the box that tint only while it is chosen", () => {
    const tinted = [options[0]!, { ...options[1]!, chip: "border-integration-playback-midi bg-integration-playback-midi/45" }];
    const { rerender } = render(<ConnectionMethod onChange={() => {}} options={tinted} value="api" />);
    expect(pill()).toHaveClass("bg-primary/45");
    rerender(<ConnectionMethod onChange={() => {}} options={tinted} value="midi" />);
    expect(pill()).toHaveClass("bg-integration-playback-midi/45", "border-integration-playback-midi");
    expect(pill()).not.toHaveClass("bg-primary/45");
  });

  it("the label row is as tall as the Not saved yet marker, so the switch does not move when a change is held", () => {
    const { rerender } = render(<ConnectionMethod onChange={() => {}} options={options} value="api" />);
    const legend = () => document.querySelector("legend")!;
    expect(legend()).toHaveClass("min-h-[18px]");
    rerender(<ConnectionMethod heldChange onChange={() => {}} options={options} value="api" />);
    expect(legend()).toHaveClass("min-h-[18px]");
    expect(document.querySelector("[data-held-marker]")).toHaveClass("mt-6");
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

describe("ConnectionMethod without ranked options (Backend Connection Access)", () => {
  it("draws no PRIMARY/ALTERNATE line when an option has no role, and names the radio by its label alone", () => {
    render(
      <ConnectionMethod
        label="Connection Access"
        onChange={() => undefined}
        options={[{ value: "host", label: "Host" }, { value: "network", label: "Local network" }, { value: "internet", label: "Internet" }]}
        value="host"
      />,
    );
    expect(screen.getByRole("radiogroup", { name: "Connection Access" })).toBeInTheDocument();
    expect(screen.queryByText(/primary|alternate/i)).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Local network" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Host" })).toBeChecked();
  });
});
