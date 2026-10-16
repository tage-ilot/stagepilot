import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Panel } from "./Panel";
import { Switch } from "./Switch";
import { accentClasses } from "./accent";

describe("Switch", () => {
  it("is a real switch with the word beside it and a 44px target", () => {
    render(<Switch label="Skip" aria-label="Skip So Much" onChange={() => {}} />);
    const control = screen.getByRole("switch", { name: "Skip So Much" });
    expect(control).not.toBeChecked();
    expect(screen.getByText("Skip")).toBeVisible();
    expect(control.closest("label")).toHaveClass("min-h-ds-touch-target");
  });

  it("toggles from a click and from the keyboard", async () => {
    const onChange = vi.fn();
    render(<Switch label="Skip" onChange={onChange} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("switch"));
    expect(onChange).toHaveBeenCalledTimes(1);
    // The click left it focused, so Space is the keyboard path: it must toggle again.
    expect(screen.getByRole("switch")).toHaveFocus();
    await user.keyboard(" ");
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it("does nothing when disabled", async () => {
    const onChange = vi.fn();
    render(<Switch disabled label="Skip" onChange={onChange} />);
    await userEvent.setup().click(screen.getByRole("switch"));
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("switch")).toBeDisabled();
  });

  it("wears the connection colour inside a panel and the primary outside one", () => {
    const { container, rerender } = render(<Switch label="Skip" onChange={() => {}} />);
    expect(container.querySelector("span[aria-hidden]")).toHaveClass("peer-checked:bg-primary");
    rerender(<Panel tone="playback" variant="setup"><Switch label="Skip" onChange={() => {}} /></Panel>);
    for (const cls of accentClasses.playback.switchOn.split(" ")) expect(container.querySelector("span[aria-hidden]")).toHaveClass(cls);
  });
});
