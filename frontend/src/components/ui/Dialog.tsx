import { useEffect, useId, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";

import { cx } from "./cx";

export type DialogLayer = "confirm" | "update" | "fatal" | "boundary";
export type DialogTone = "default" | "danger";

export interface DialogProps {
  open: boolean;
  /** Called on Escape and backdrop click, only when `dismissible`. */
  onClose?: () => void;
  /** Default true. Set false for confirmations that guard a live action or a locked update. */
  dismissible?: boolean;
  title: ReactNode;
  description?: ReactNode;
  /** Only sets data-tone. The caller picks the primary action's Button variant. */
  tone?: DialogTone;
  /** DESIGN.md stacking order: confirm 50, update 100, fatal 110, boundary 120 (full-screen error boundary). */
  layer?: DialogLayer;
  children?: ReactNode;
  /** Footer buttons. Stacked full width below 481px, a row from 481px. Put the safe option first. */
  actions?: ReactNode;
  className?: string;
}

// Class strings are written out in full so Tailwind can see them.
const layers: Record<DialogLayer, string> = {
  confirm: "z-50",
  update: "z-[100]",
  fatal: "z-[110]",
  boundary: "z-[120]",
};

const FOCUSABLE = 'a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

export function Dialog({
  open,
  onClose,
  dismissible = true,
  title,
  description,
  tone = "default",
  layer = "confirm",
  children,
  actions,
  className,
}: DialogProps) {
  const id = useId();
  const titleId = `${id}-title`;
  const descId = `${id}-desc`;
  const card = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [shown, setShown] = useState(false);
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (!open) return;
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const first = card.current?.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? card.current)?.focus({ preventScroll: true });
    const frame = requestAnimationFrame(() => setShown(true));
    return () => {
      cancelAnimationFrame(frame);
      setShown(false);
      opener.current?.focus();
      opener.current = null;
    };
  }, [open]);

  useEffect(() => {
    if (!open || !dismissible) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      close.current?.();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, dismissible]);

  if (!open) return null;

  const trap = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab" || !card.current) return;
    const items = [...card.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
    if (!items.length) {
      event.preventDefault();
      card.current.focus();
      return;
    }
    const first = items[0]!;
    const last = items[items.length - 1]!;
    const active = document.activeElement;
    if (event.shiftKey && (active === first || active === card.current)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const onBackdrop = (event: MouseEvent<HTMLDivElement>) => {
    if (dismissible && event.target === event.currentTarget) close.current?.();
  };

  return (
    <div
      data-dialog-backdrop
      data-layer={layer}
      onClick={onBackdrop}
      onKeyDown={trap}
      className={cx(
        "fixed inset-0 grid place-items-center overscroll-contain bg-scrim p-4 pt-[max(1rem,env(safe-area-inset-top))] pb-[max(1rem,env(safe-area-inset-bottom))] touch-manipulation transition-opacity duration-[140ms] motion-reduce:transition-none",
        layers[layer],
        shown ? "opacity-100" : "opacity-0",
      )}
    >
      <div
        ref={card}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descId : undefined}
        tabIndex={-1}
        data-dialog
        data-tone={tone}
        className={cx(
          "max-h-full w-full max-w-lg space-y-4 overflow-y-auto overscroll-contain rounded-ds-xl border border-edge bg-surface-950 p-5 shadow-elevation-overlay touch-manipulation min-[481px]:w-auto min-[481px]:min-w-[24rem]",
          className,
        )}
      >
        <h2 id={titleId} className="break-words text-lg font-bold text-ink">
          {title}
        </h2>
        {description && (
          <p id={descId} className="break-words text-ink-body">
            {description}
          </p>
        )}
        {children && <div className="break-words text-ink-body">{children}</div>}
        {actions && (
          <div
            data-dialog-actions
            className="flex flex-col gap-2 [&>*]:w-full min-[481px]:flex-row min-[481px]:justify-end min-[481px]:[&>*]:w-auto"
          >
            {actions}
          </div>
        )}
      </div>
    </div>
  );
}
