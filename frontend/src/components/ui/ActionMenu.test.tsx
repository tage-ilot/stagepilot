import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { ActionMenu } from "./ActionMenu";

const has = (el: Element, token: string) => el.className.split(/\s+/).includes(token);
const items = [
  { value: "type", label: "Sunday Service" },
  { value: "youth", label: "Youth" },
  { value: "kids", label: "Kids" },
];

function Harness({ initial = "type", onSelect, disabled }: { initial?: string; onSelect?: (v: string) => void; disabled?: boolean }) {
  const [value, setValue] = useState(initial);
  return (
    <>
      <button>Outside</button>
      <ActionMenu label="Plan category" menuLabel="Plan categories" items={items} value={value} disabled={disabled} onSelect={(v) => { setValue(v); onSelect?.(v); }} />
    </>
  );
}
const trigger = () => screen.getByRole("button", { name: "Plan category" });

describe("ActionMenu", () => {
  it("renders a closed trigger with menu semantics and a chevron", () => {
    render(<Harness />);
    expect(trigger()).toHaveAttribute("aria-haspopup", "menu");
    expect(trigger()).toHaveAttribute("aria-expanded", "false");
    expect(trigger().querySelector("svg[aria-hidden='true']")).not.toBeNull();
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("opens on click, focuses the checked item, marks only it aria-checked, 44px items", async () => {
    const user = userEvent.setup();
    render(<Harness initial="youth" />);
    await user.click(trigger());
    expect(screen.getByRole("menu", { name: "Plan categories" })).toBeInTheDocument();
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
    const radios = screen.getAllByRole("menuitemradio");
    expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual(["false", "true", "false"]);
    expect(radios[1]).toHaveFocus();
    for (const r of radios) expect(has(r, "min-h-ds-touch-target")).toBe(true);
    expect(radios[1]).toHaveTextContent("✓");
  });

  it("opens with ArrowDown on the trigger and focuses the checked item", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    trigger().focus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menu")).toBeInTheDocument();
    expect(screen.getAllByRole("menuitemradio")[0]).toHaveFocus();
  });

  it("moves with arrows (wrapping), Home and End", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(trigger());
    const r = screen.getAllByRole("menuitemradio");
    await user.keyboard("{ArrowDown}");
    expect(r[1]).toHaveFocus();
    await user.keyboard("{ArrowUp}{ArrowUp}");
    expect(r[2]).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(r[0]).toHaveFocus();
    await user.keyboard("{End}");
    expect(r[2]).toHaveFocus();
    await user.keyboard("{Home}");
    expect(r[0]).toHaveFocus();
  });

  it("selects with Enter, closes and returns focus to the trigger", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<Harness onSelect={onSelect} />);
    await user.click(trigger());
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onSelect).toHaveBeenCalledWith("youth");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger()).toHaveFocus();
    await user.click(trigger());
    expect(screen.getAllByRole("menuitemradio")[1]).toHaveAttribute("aria-checked", "true");
  });

  it("closes on Escape and returns focus to the trigger without selecting", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<Harness onSelect={onSelect} />);
    await user.click(trigger());
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger()).toHaveFocus();
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("closes on Tab and on outside pointerdown, not on inside pointerdown", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(trigger());
    await user.keyboard("{Tab}");
    expect(screen.queryByRole("menu")).toBeNull();
    await user.click(trigger());
    await user.pointer({ keys: "[MouseLeft>]", target: screen.getByRole("menu") });
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Outside" }));
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("toggles closed when the trigger is clicked again", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(trigger());
    await user.click(trigger());
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("does not open when disabled", async () => {
    const user = userEvent.setup();
    render(<Harness disabled />);
    await user.click(trigger());
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("supports controlled open state", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const { rerender } = render(<ActionMenu label="L" menuLabel="M" items={items} open={false} onOpenChange={onOpenChange} onSelect={() => {}} />);
    await user.click(screen.getByRole("button", { name: "L" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    expect(screen.queryByRole("menu")).toBeNull();
    rerender(<ActionMenu label="L" menuLabel="M" items={items} open onOpenChange={onOpenChange} onSelect={() => {}} />);
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });

  it("uses popover tokens, scroll containment, touch and can-hover classes", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(trigger());
    const menu = screen.getByRole("menu");
    for (const t of ["bg-surface-950/95", "border-edge-medium", "rounded-ds-lg", "shadow-elevation-overlay", "overscroll-contain", "touch-manipulation"]) expect(has(menu, t)).toBe(true);
    expect(has(screen.getAllByRole("menuitemradio")[0]!, "can-hover:bg-surface-900")).toBe(true);
  });
});
