import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";

import { Button } from "./Button";
import { Toast } from "./Toast";

const has = (el: Element, token: string) => el.className.split(/\s+/).includes(token);
const root = (c: HTMLElement) => c.querySelector<HTMLElement>("[data-toast]")!;

describe("Toast", () => {
  it("is a non-modal assertive alertdialog named by title and described by description", () => {
    render(<Toast side="left" title="StagePilot ran into a problem" description="This did not stop the app." />);
    const d = screen.getByRole("alertdialog");
    expect(d).toHaveAttribute("aria-modal", "false");
    expect(d).toHaveAttribute("aria-live", "assertive");
    expect(d).toHaveAttribute("tabindex", "-1");
    expect(d).toHaveAccessibleName("StagePilot ran into a problem");
    expect(d).toHaveAccessibleDescription("This did not stop the app.");
  });

  it("omits aria-describedby without a description and has no scrim or trap", () => {
    const { container } = render(<Toast side="right" title="T" />);
    expect(screen.getByRole("alertdialog")).not.toHaveAttribute("aria-describedby");
    expect(has(root(container), "inset-0")).toBe(false);
    expect(container.querySelector("[data-dialog-backdrop]")).toBeNull();
  });

  it("defaults to danger and exposes data-tone and data-side", () => {
    const { container, rerender } = render(<Toast side="left" title="T" />);
    expect(root(container)).toHaveAttribute("data-tone", "danger");
    expect(root(container)).toHaveAttribute("data-side", "left");
    expect(has(root(container), "left-6")).toBe(true);
    expect(has(root(container), "border-danger-soft/30")).toBe(true);
    rerender(<Toast side="right" tone="warning" title="T" />);
    expect(root(container)).toHaveAttribute("data-tone", "warning");
    expect(has(root(container), "right-6")).toBe(true);
    expect(has(root(container), "border-warning/30")).toBe(true);
  });

  it("uses z 110, 16px radius, overlay elevation and a 320px-safe width", () => {
    const { container } = render(<Toast side="left" title="T" />);
    for (const t of ["fixed", "bottom-6", "z-[110]", "rounded-ds-2xl", "shadow-elevation-overlay", "bg-surface-950", "w-full", "max-w-[min(24rem,calc(100vw-3rem))]"]) {
      expect(has(root(container), t)).toBe(true);
    }
  });

  it("forwards its ref to the alertdialog element", () => {
    const ref = createRef<HTMLDivElement>();
    render(<Toast ref={ref} side="right" title="T" />);
    expect(ref.current).toBe(screen.getByRole("alertdialog"));
  });

  it("renders a 44px Dismiss first when onDismiss is given, then the actions", async () => {
    const onDismiss = vi.fn();
    const user = userEvent.setup();
    const { container } = render(<Toast side="left" title="T" onDismiss={onDismiss} actions={<Button size="touch" variant="destructive-soft">Send logs</Button>} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual(["Dismiss", "Send logs"]);
    expect(buttons[0]).toHaveAttribute("data-size", "touch");
    await user.click(buttons[0]!);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(container.querySelector("[data-toast-actions]")).not.toBeNull();
  });

  it("renders no action row when there are no actions", () => {
    const { container } = render(<Toast side="left" title="T" />);
    expect(container.querySelector("[data-toast-actions]")).toBeNull();
  });

  it("does not let a mousedown on the toast reach ancestors", async () => {
    const outer = vi.fn();
    const user = userEvent.setup();
    render(<div onMouseDown={outer}><Toast side="left" title="Hit me" /></div>);
    await user.click(screen.getByText("Hit me"));
    expect(outer).not.toHaveBeenCalled();
  });

  it("does not move focus or trap Tab", async () => {
    const user = userEvent.setup();
    render(<><button>Before</button><Toast side="left" title="T" onDismiss={() => {}} /><button>After</button></>);
    expect(document.body).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Before" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Dismiss" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "After" })).toHaveFocus();
  });
});
