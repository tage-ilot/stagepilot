import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { SegmentedControl, type SegmentedOption } from "./SegmentedControl";
import type { Tone } from "./types";

const main: SegmentedOption = { value: "api", label: "Playback connection", caption: "Main" };
const alt: SegmentedOption = { value: "midi", label: "MIDI", caption: "Alternate" };
const options: SegmentedOption[] = [main, alt];

function Harness({ initial = "api", opts = options, tone = "playback" as Tone, onChange }: { initial?: string; opts?: SegmentedOption[]; tone?: Tone; onChange?: (v: string) => void }) {
  const [value, setValue] = useState(initial);
  return <SegmentedControl legend="Connection method" value={value} onChange={(v) => { setValue(v); onChange?.(v); }} options={opts} tone={tone} />;
}

describe("SegmentedControl", () => {
  it("groups native radios in a fieldset named by the legend", () => {
    render(<Harness />);
    const group = screen.getByRole("group", { name: "Connection method" });
    expect(group.tagName).toBe("FIELDSET");
    const radios = screen.getAllByRole("radio");
    expect(radios).toHaveLength(2);
    radios.forEach((r) => expect(r).toHaveAttribute("type", "radio"));
    expect(new Set(radios.map((r) => r.getAttribute("name"))).size).toBe(1);
    expect(screen.getByRole("radio", { name: /Playback connection/ })).toBeChecked();
  });

  it("selects by click on the visible label", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    await user.click(screen.getByText("MIDI"));
    expect(onChange).toHaveBeenCalledWith("midi");
    expect(screen.getByRole("radio", { name: /MIDI/ })).toBeChecked();
  });

  it("selects with arrow keys (native radio behaviour)", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.tab();
    expect(screen.getByRole("radio", { name: /Playback connection/ })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("radio", { name: /MIDI/ })).toBeChecked();
    await user.keyboard("{ArrowLeft}");
    expect(screen.getByRole("radio", { name: /Playback connection/ })).toBeChecked();
  });

  it("is controlled: value prop decides, onChange only reports", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<SegmentedControl legend="Method" value="api" onChange={onChange} options={options} tone="playback" />);
    await user.click(screen.getByText("MIDI"));
    expect(onChange).toHaveBeenCalledWith("midi");
    expect(screen.getByRole("radio", { name: /Playback connection/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: /MIDI/ })).not.toBeChecked();
  });

  it("does not select a disabled option by click or arrows", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Harness onChange={onChange} opts={[main, { ...alt, disabled: true }]} />);
    const midi = screen.getByRole("radio", { name: /MIDI/ });
    expect(midi).toBeDisabled();
    await user.click(screen.getByText("MIDI"));
    await user.tab();
    await user.keyboard("{ArrowRight}");
    expect(onChange).not.toHaveBeenCalled();
    expect(midi).not.toBeChecked();
    expect(midi.closest("[data-segment]")?.querySelector("label")).toHaveClass("peer-disabled:opacity-40", "peer-disabled:cursor-not-allowed");
  });

  it("disables every segment when the group is disabled", () => {
    render(<SegmentedControl legend="M" value="api" onChange={() => {}} options={options} tone="playback" disabled />);
    screen.getAllByRole("radio").forEach((r) => expect(r).toBeDisabled());
    expect(screen.getByRole("group")).toBeDisabled();
  });

  it("renders Main and Alternate captions", () => {
    render(<Harness />);
    expect(screen.getByText("Main")).toBeInTheDocument();
    expect(screen.getByText("Alternate")).toBeInTheDocument();
  });

  it("marks selection with more than colour: check glyph, bold, native checked", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const segs = () => [...document.querySelectorAll<HTMLElement>("[data-segment]")];
    expect(segs()[0]!.querySelector("[data-check]")).not.toBeNull();
    expect(segs()[0]!.querySelector("label")).toHaveClass("font-bold");
    expect(segs()[1]!.querySelector("[data-check]")).toBeNull();
    await user.click(screen.getByText("MIDI"));
    expect(segs()[1]!.querySelector("[data-check]")).not.toBeNull();
    expect(segs()[0]!.querySelector("[data-check]")).toBeNull();
  });

  it.each<[Tone, string, string]>([
    ["services", "border-integration-services", "text-integration-services-text"],
    ["presentation", "border-integration-presentation", "text-integration-presentation-text"],
    ["playback", "border-integration-playback", "text-integration-playback-text"],
    ["lights", "border-integration-lights", "text-info-text"],
  ])("applies %s tone to the selected segment only", (tone, border, text) => {
    render(<Harness tone={tone} />);
    const [first, second] = [...document.querySelectorAll("[data-segment] label")] as Element[];
    expect(first as Element).toHaveClass(border, text, `bg-${border.slice(7)}/15`);
    expect(second).not.toHaveClass(border);
    expect(document.querySelector("[data-segmented]")).toHaveAttribute("data-tone", tone);
  });

  it("is 44px tall, touch-friendly, hover-gated and ring-on-focus", () => {
    render(<Harness />);
    const label = document.querySelectorAll("[data-segment] label")[1] as Element;
    expect(label).toHaveClass("min-h-ds-touch-target", "touch-manipulation", "can-hover:border-edge-medium", "peer-focus-visible:outline-primary-focus");
    expect(label.className).not.toMatch(/(^|\s)hover:/);
  });

  it("shows an error and wires it to the group", () => {
    render(<SegmentedControl legend="M" value="api" onChange={() => {}} options={options} tone="playback" error="Pick a method" />);
    const alert = screen.getByRole("alert");
    expect(screen.getByRole("group")).toHaveAttribute("aria-describedby", alert.id);
    expect(screen.getByRole("group")).toHaveAttribute("data-state", "error");
  });

  it("supports 3 and 4 options", () => {
    render(<Harness opts={[{ value: "a", label: "A" }, { value: "b", label: "B" }, { value: "c", label: "C" }, { value: "d", label: "D" }]} initial="a" />);
    expect(screen.getAllByRole("radio")).toHaveLength(4);
  });
});
