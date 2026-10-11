import { useEffect, useId, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";

import { Button, type ButtonProps } from "./Button";
import { cx } from "./cx";

export interface ActionMenuItem {
  value: string;
  label: string;
}

export interface ActionMenuProps {
  /** Trigger text. */
  label: string;
  /** Accessible name of the menu itself. */
  menuLabel: string;
  items: readonly ActionMenuItem[];
  /** Current item: shown with a check mark and aria-checked. */
  value?: string | null;
  /** Called with the chosen item's value. The menu then closes and focus returns to the trigger. */
  onSelect: (value: string) => void;
  /** Controlled open state (for menus that load their items first). Omit for internal state. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  disabled?: boolean;
  /** Passed to the trigger Button. */
  triggerVariant?: ButtonProps["variant"];
  triggerTone?: ButtonProps["tone"];
  className?: string;
}

/**
 * Button with a chevron that opens a card of single-choice items (DESIGN.md popover, 8px radius, elevation.overlay).
 * Keyboard (mirrors PlanningCenterLengths): ArrowDown on the trigger opens; ArrowUp/ArrowDown wrap, Home/End jump,
 * Escape closes and refocuses the trigger, Tab closes. Outside pointerdown closes. Items are `menuitemradio`, 44px.
 */
export function ActionMenu({
  label,
  menuLabel,
  items,
  value = null,
  onSelect,
  open: openProp,
  onOpenChange,
  disabled = false,
  triggerVariant = "secondary",
  triggerTone,
  className,
}: ActionMenuProps) {
  const [inner, setInner] = useState(false);
  const controlled = openProp !== undefined;
  const open = controlled ? openProp : inner;
  const setOpen = (next: boolean) => {
    if (!controlled) setInner(next);
    onOpenChange?.(next);
  };
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const buttons = menu.current?.querySelectorAll<HTMLButtonElement>("button");
    const current = menu.current?.querySelector<HTMLButtonElement>('button[aria-checked="true"]');
    (current ?? buttons?.[0])?.focus();
  }, [open, items]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!menu.current?.contains(target) && !trigger.current?.contains(target)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const onMenuKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const list = [...(menu.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    const index = list.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Escape") {
      event.preventDefault();
      setOpen(false);
      trigger.current?.focus();
    } else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = event.key === "Home" ? 0 : event.key === "End" ? list.length - 1 : (index + step + list.length) % list.length;
      list[next]?.focus();
    } else if (event.key === "Tab") {
      setOpen(false);
    }
  };

  return (
    <div data-action-menu data-state={open ? "open" : "closed"} className={cx("relative inline-block max-w-full", className)}>
      <Button
        ref={trigger}
        variant={triggerVariant}
        tone={triggerTone}
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" && !open) {
            event.preventDefault();
            trigger.current?.click();
          }
        }}
        onClick={() => setOpen(!open)}
        className="gap-2"
      >
        {label}
        <svg aria-hidden="true" viewBox="0 0 12 12" className={cx("size-3 shrink-0 transition-transform duration-[140ms] motion-reduce:transition-none", open && "rotate-180")} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M2.5 4.5 6 8l3.5-3.5" />
        </svg>
      </Button>
      {open && (
        <div
          ref={menu}
          id={id}
          role="menu"
          aria-label={menuLabel}
          onKeyDown={onMenuKey}
          className="absolute left-0 top-full z-20 mt-1 max-h-72 w-64 min-w-full max-w-[calc(100vw-2rem)] overflow-y-auto overscroll-contain rounded-ds-lg border border-edge-medium bg-surface-950/95 p-1 shadow-elevation-overlay touch-manipulation"
        >
          {items.map((item) => {
            const checked = item.value === value;
            return (
              <button
                key={item.value}
                type="button"
                role="menuitemradio"
                aria-checked={checked}
                tabIndex={-1}
                data-checked={checked ? "true" : undefined}
                onClick={() => {
                  onSelect(item.value);
                  setOpen(false);
                  trigger.current?.focus();
                }}
                className="flex min-h-ds-touch-target w-full items-center gap-2 break-words rounded-ds-md p-2 text-left font-type-body-md text-type-body-md text-ink touch-manipulation transition-colors duration-[140ms] motion-reduce:transition-none can-hover:bg-surface-900 focus:bg-surface-900"
              >
                <span aria-hidden="true" className="w-4 shrink-0 text-center">{checked ? "✓" : ""}</span>
                <span className="min-w-0">{item.label}</span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
