import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { changeCountText } from "./changeCount";
import { SaveBar, type SaveBarState } from "./SaveBar";

const states: SaveBarState[] = [
  { kind: "clean" },
  { kind: "auto", at: "9:41 AM" },
  { kind: "saving" },
  { kind: "held", count: 2 },
  { kind: "error", message: "Couldn't reach the service." },
];
const has = (el: HTMLElement, token: string) => el.className.split(/\s+/).includes(token);
const bar = (container: HTMLElement) => container.querySelector<HTMLElement>("[data-save-bar]")!;

describe("SaveBar", () => {
  it.each(states)("always renders the Save configuration button (%j)", (state) => {
    render(<SaveBar state={state} />);
    expect(screen.getByRole("button", { name: "Save configuration" })).toBeInTheDocument();
  });

  it("has no prop that hides the button", () => {
    render(<SaveBar state={{ kind: "clean" }} saveLabel="Save configuration" stickyOnPhone={false} />);
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });

  it("clean: wording, reason beside the button, aria-disabled but focusable and harmless", async () => {
    const onSave = vi.fn();
    const { container } = render(<SaveBar state={{ kind: "clean" }} onSave={onSave} />);
    expect(screen.getByText("All changes saved")).toBeInTheDocument();
    expect(screen.getByText("No changes to save")).toBeInTheDocument();
    const b = screen.getByRole("button", { name: "Save configuration" });
    expect(b).toHaveAttribute("aria-disabled", "true");
    expect(b).not.toBeDisabled();
    expect(has(b, "opacity-40")).toBe(true);
    expect(b.getAttribute("aria-describedby")).toBe(screen.getByText("No changes to save").id);
    b.focus();
    expect(b).toHaveFocus();
    await userEvent.click(b);
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(bar(container)).toHaveAttribute("data-state", "clean");
  });

  it("auto: check, wording, tabular-nums time, ready button", () => {
    const { container } = render(<SaveBar state={{ kind: "auto", at: "9:41 AM" }} />);
    expect(screen.getByText(/Saved automatically at/)).toHaveTextContent("Saved automatically at 9:41 AM");
    expect(has(screen.getByText("9:41 AM"), "tabular-nums")).toBe(true);
    expect(container.querySelector("svg")).toBeInTheDocument();
    const b = screen.getByRole("button", { name: "Save configuration" });
    expect(b).not.toHaveAttribute("aria-disabled");
    expect(b).toBeEnabled();
  });

  it("saving: Saving…, button loading, blocks double click", async () => {
    const onSave = vi.fn();
    render(<SaveBar state={{ kind: "saving" }} onSave={onSave} />);
    expect(screen.getByText("Saving…")).toBeInTheDocument();
    const b = screen.getByRole("button", { name: "Save configuration" });
    expect(b).toHaveAttribute("data-loading", "true");
    expect(b).toHaveAttribute("aria-busy", "true");
    await userEvent.dblClick(b);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("held: plural and singular wording with nbsp, both buttons, callbacks", async () => {
    const onSave = vi.fn();
    const onDiscard = vi.fn();
    const { container, rerender } = render(<SaveBar state={{ kind: "held", count: 2 }} onSave={onSave} onDiscard={onDiscard} />);
    expect(container.querySelector("[data-save-status]")?.textContent).toBe("2\u00a0changes waiting. Not saved yet.");
    expect(has(container.querySelector<HTMLElement>("[data-save-status] .tabular-nums")!, "tabular-nums")).toBe(true);
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual(["Discard", "Save configuration"]);
    const discard = screen.getByRole("button", { name: "Discard" });
    const save = screen.getByRole("button", { name: "Save configuration" });
    expect(discard).toHaveAttribute("data-variant", "secondary");
    expect(save).toHaveAttribute("data-variant", "primary");
    await userEvent.click(discard);
    await userEvent.click(save);
    expect(onDiscard).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(bar(container).className).toContain("border-warning");
    rerender(<SaveBar state={{ kind: "held", count: 1 }} />);
    expect(container.querySelector("[data-save-status]")?.textContent).toBe("1\u00a0change waiting. Not saved yet.");
  });

  it("error: message, Try again calls onRetry, red edge", async () => {
    const onRetry = vi.fn();
    const { container } = render(<SaveBar state={{ kind: "error", message: "Couldn't save." }} onRetry={onRetry} />);
    expect(screen.getByText("Couldn't save.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Save configuration" })).toBeInTheDocument();
    expect(bar(container).className).toContain("border-danger");
  });

  it("wraps status in one polite live region", () => {
    const { container } = render(<SaveBar state={{ kind: "held", count: 3 }} />);
    const live = container.querySelectorAll('[aria-live="polite"]');
    expect(live).toHaveLength(1);
    expect(live[0]).toContainElement(container.querySelector("[data-save-status]"));
  });

  it("is sticky below lg by default and optional", () => {
    const { container, rerender } = render(<SaveBar state={{ kind: "clean" }} />);
    for (const t of ["sticky", "bottom-0", "lg:static"]) expect(has(bar(container), t)).toBe(true);
    expect(bar(container).className).toContain("env(safe-area-inset-bottom)");
    rerender(<SaveBar state={{ kind: "clean" }} stickyOnPhone={false} />);
    expect(has(bar(container), "sticky")).toBe(false);
  });

  it("stacks full width below 480px in DOM order Discard then Save, no order-*", () => {
    const { container } = render(<SaveBar state={{ kind: "held", count: 2 }} />);
    expect(has(bar(container), "flex-col")).toBe(true);
    expect(bar(container).className).toContain("min-[481px]:flex-row");
    for (const b of screen.getAllByRole("button")) {
      expect(has(b, "w-full")).toBe(true);
      expect(b.className).toContain("min-[481px]:w-auto");
    }
    expect(container.innerHTML).not.toMatch(/\border-/);
  });

  it("changeCountText", () => {
    expect(changeCountText(1)).toBe("1\u00a0change");
    expect(changeCountText(0)).toBe("0\u00a0changes");
  });
});
