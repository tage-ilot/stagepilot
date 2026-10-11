import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { Button } from "./Button";
import { Dialog } from "./Dialog";

const has = (el: Element, token: string) => el.className.split(/\s+/).includes(token);
const backdrop = (c: HTMLElement) => c.querySelector<HTMLElement>("[data-dialog-backdrop]")!;

const actions = (
  <>
    <Button size="touch" variant="secondary">Cancel</Button>
    <Button size="touch">Confirm</Button>
  </>
);

describe("Dialog", () => {
  it("renders nothing when closed", () => {
    const { container } = render(<Dialog open={false} title="T" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("wires role and aria", () => {
    render(<Dialog open title="Set up?" description="Details here" tone="danger" />);
    const d = screen.getByRole("dialog");
    expect(d).toHaveAttribute("aria-modal", "true");
    expect(d).toHaveAccessibleName("Set up?");
    expect(d).toHaveAccessibleDescription("Details here");
    expect(d).toHaveAttribute("data-tone", "danger");
  });

  it("omits aria-describedby without a description", () => {
    render(<Dialog open title="T" />);
    expect(screen.getByRole("dialog")).not.toHaveAttribute("aria-describedby");
  });

  it("closes on Escape and backdrop when dismissible, not on card click", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    const { container } = render(<Dialog open title="T" onClose={onClose}>body</Dialog>);
    await user.click(screen.getByText("body"));
    expect(onClose).not.toHaveBeenCalled();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
    await user.click(backdrop(container));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("ignores Escape and backdrop when not dismissible", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    const { container } = render(<Dialog open dismissible={false} title="T" onClose={onClose} />);
    await user.keyboard("{Escape}");
    await user.click(backdrop(container));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("moves focus in on open and returns it to the opener on close", async () => {
    const user = userEvent.setup();
    function Host() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)}>Open</button>
          <Dialog open={open} onClose={() => setOpen(false)} title="T" actions={actions} />
        </>
      );
    }
    render(<Host />);
    const opener = screen.getByRole("button", { name: "Open" });
    await user.click(opener);
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(opener).toHaveFocus();
  });

  it("focuses the card when there is nothing focusable", () => {
    render(<Dialog open title="T" />);
    expect(screen.getByRole("dialog")).toHaveFocus();
  });

  it("traps Tab and Shift+Tab", async () => {
    const user = userEvent.setup();
    render(<Dialog open title="T" actions={actions} />);
    const cancel = screen.getByRole("button", { name: "Cancel" });
    const confirm = screen.getByRole("button", { name: "Confirm" });
    expect(cancel).toHaveFocus();
    await user.tab();
    expect(confirm).toHaveFocus();
    await user.tab();
    expect(cancel).toHaveFocus();
    await user.tab({ shift: true });
    expect(confirm).toHaveFocus();
  });

  it("stacks actions below 481px and rows from 481px", () => {
    render(<Dialog open title="T" actions={actions} />);
    const a = document.querySelector<HTMLElement>("[data-dialog-actions]")!;
    for (const t of ["flex-col", "[&>*]:w-full", "min-[481px]:flex-row"]) expect(has(a, t)).toBe(true);
    expect(a.firstElementChild).toHaveTextContent("Cancel");
    expect(screen.getByRole("button", { name: "Confirm" })).toHaveAttribute("data-size", "touch");
  });

  it.each([
    ["confirm", "z-50"],
    ["update", "z-[100]"],
    ["fatal", "z-[110]"],
  ] as const)("layer %s maps to %s", (layer, cls) => {
    const { container } = render(<Dialog open title="T" layer={layer} />);
    expect(has(backdrop(container), cls)).toBe(true);
  });

  it("defaults to the confirm layer", () => {
    const { container } = render(<Dialog open title="T" />);
    expect(has(backdrop(container), "z-50")).toBe(true);
  });

  it("uses the scrim, surface, radius, scroll and touch classes", () => {
    const { container } = render(<Dialog open title="T" />);
    const b = backdrop(container);
    const d = screen.getByRole("dialog");
    for (const t of ["fixed", "inset-0", "bg-scrim", "overscroll-contain", "touch-manipulation", "transition-opacity", "motion-reduce:transition-none"]) {
      expect(has(b, t)).toBe(true);
    }
    for (const t of ["bg-surface-950", "rounded-ds-xl", "border-edge", "shadow-elevation-overlay", "p-5", "overflow-y-auto", "overscroll-contain", "touch-manipulation"]) {
      expect(has(d, t)).toBe(true);
    }
  });
});
