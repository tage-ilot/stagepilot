import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { Disclosure } from "./Disclosure";

const body = <input aria-label="inner field" />;

describe("Disclosure", () => {
  it("toggles by click and wires aria-expanded/aria-controls", async () => {
    const user = userEvent.setup();
    render(<Disclosure variant="advanced" tone="services" title="Advanced">{body}</Disclosure>);
    const button = screen.getByRole("button", { name: /Advanced/ });
    expect(button).toHaveAttribute("type", "button");
    expect(button).toHaveAttribute("aria-expanded", "false");
    const region = document.getElementById(button.getAttribute("aria-controls") as string);
    expect(region).not.toBeNull();
    expect(region).toContainElement(screen.getByLabelText("inner field", { selector: "input" }) as HTMLElement);
    await user.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    await user.click(button);
    expect(button).toHaveAttribute("aria-expanded", "false");
  });

  it.each(["{Enter}", " "])("toggles from the keyboard with %j (native button)", async (key) => {
    const user = userEvent.setup();
    render(<Disclosure variant="activity" title="Recent activity">x</Disclosure>);
    await user.tab();
    expect(screen.getByRole("button")).toHaveFocus();
    await user.keyboard(key);
    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "true");
  });

  it("keeps the closed body out of the tab order and the accessibility tree", async () => {
    const user = userEvent.setup();
    render(<Disclosure variant="advanced" tone="playback" title="Advanced">{body}</Disclosure>);
    // jsdom does not compute the accessibility tree for `inert`, so assert the attribute on the ancestor of the field.
    expect(screen.getByLabelText("inner field").closest("[inert]")).not.toBeNull();
    // Real tab-order behaviour of inert is proven in Chromium (see PR body); user-event does not model inert.
    await user.click(screen.getByRole("button"));
    expect(document.querySelector("[inert]")).toBeNull();
    expect(screen.getByLabelText("inner field").closest("[inert]")).toBeNull();
    await user.tab();
    expect(screen.getByRole("textbox", { name: "inner field" })).toHaveFocus();
  });

  it("supports defaultOpen and onOpenChange in uncontrolled mode", async () => {
    const onOpenChange = vi.fn();
    render(<Disclosure variant="advanced" tone="lights" title="A" defaultOpen onOpenChange={onOpenChange}>x</Disclosure>);
    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "true");
    await userEvent.click(screen.getByRole("button"));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "false");
  });

  it("controlled mode follows the prop and only reports the request", async () => {
    const onOpenChange = vi.fn();
    const { rerender } = render(<Disclosure variant="activity" title="A" open={false} onOpenChange={onOpenChange}>x</Disclosure>);
    await userEvent.click(screen.getByRole("button"));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "false");
    rerender(<Disclosure variant="activity" title="A" open onOpenChange={onOpenChange}>x</Disclosure>);
    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "true");
  });

  it("works when a parent owns the state", async () => {
    function Host() {
      const [open, setOpen] = useState(false);
      return <Disclosure variant="advanced" tone="services" title="A" open={open} onOpenChange={setOpen}>x</Disclosure>;
    }
    render(<Host />);
    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "true");
  });

  it("shows the summary only while closed, truncating, with full text in title", async () => {
    render(<Disclosure variant="advanced" tone="services" title="Advanced" summary="Port 8080 · Channel 1">x</Disclosure>);
    const summary = screen.getByText("Port 8080 · Channel 1");
    expect(summary).toHaveClass("min-w-0", "truncate");
    expect(summary).toHaveAttribute("title", "Port 8080 · Channel 1");
    await userEvent.click(screen.getByRole("button"));
    expect(screen.queryByText("Port 8080 · Channel 1")).toBeNull();
  });

  it("renders a tabular-nums count badge for activity and pluralises", () => {
    const { rerender } = render(<Disclosure variant="activity" title="Recent activity" count={12}>x</Disclosure>);
    const badge = screen.getByText("12 events");
    expect(badge).toHaveClass("tabular-nums");
    rerender(<Disclosure variant="activity" title="Recent activity" count={1}>x</Disclosure>);
    expect(screen.getByText("1 event")).toBeInTheDocument();
    rerender(<Disclosure variant="advanced" tone="services" title="Advanced" count={3}>x</Disclosure>);
    expect(screen.queryByText("3 events")).toBeNull();
  });

  it("advanced has the 3px tone bar and no dashed rule; activity the reverse and no tone", () => {
    const { rerender } = render(<Disclosure variant="advanced" tone="playback" title="A" defaultOpen>x</Disclosure>);
    const bar = document.querySelector("[data-part='bar']");
    expect(bar).toHaveClass("border-l-[3px]", "border-advanced-playback");
    expect(document.querySelector("[data-part='rule']")).toBeNull();
    expect(screen.getByRole("button")).toHaveClass("text-advanced-playback");
    rerender(<Disclosure variant="activity" title="A" defaultOpen>x</Disclosure>);
    const rule = document.querySelector("[data-part='rule']");
    expect(rule).toHaveClass("border-t", "border-dashed", "border-edge-subtle");
    expect(document.querySelector("[data-part='bar']")).toBeNull();
    expect(screen.getByRole("button")).toHaveClass("text-ink-muted", "border-edge-subtle");
    expect(screen.getByRole("button").className).not.toMatch(/advanced-/);
    expect(document.querySelector("[data-variant='activity']")).not.toHaveAttribute("data-tone");
  });

  it.each(["services", "presentation", "playback", "lights"] as const)("advanced %s uses its own colour", (tone) => {
    render(<Disclosure variant="advanced" tone={tone} title="A">x</Disclosure>);
    expect(screen.getByRole("button")).toHaveClass(`text-advanced-${tone}`);
    expect(document.querySelector("[data-part='bar']")).toHaveClass(`border-advanced-${tone}`);
  });

  it("header is 44px, rounded, bordered, with listed transitions and reduced-motion classes", () => {
    render(<Disclosure variant="advanced" tone="services" title="A">x</Disclosure>);
    expect(screen.getByRole("button")).toHaveClass("min-h-ds-touch-target", "w-full", "rounded-ds-lg", "border", "border-edge", "touch-manipulation");
    const region = document.querySelector("[data-part='region']") as HTMLElement;
    expect(region).toHaveClass("transition-[grid-template-rows,opacity]", "duration-500", "motion-reduce:transition-none");
    expect(region.className).not.toContain("transition-all");
    const chevron = document.querySelector("[data-part='chevron']") as Element;
    expect(chevron.getAttribute("class")).toContain("transition-transform");
    expect(chevron.getAttribute("class")).toContain("motion-reduce:transition-none");
  });

  it("chevron rotates with state", async () => {
    render(<Disclosure variant="activity" title="A">x</Disclosure>);
    const chevron = () => document.querySelector("[data-part='chevron']")?.getAttribute("class") ?? "";
    expect(chevron()).not.toContain("rotate-90");
    await userEvent.click(screen.getByRole("button"));
    expect(chevron()).toContain("rotate-90");
  });
});
