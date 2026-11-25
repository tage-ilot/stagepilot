import {
  useEffect,
  useRef,
  useState,
  type FocusEventHandler,
  type MouseEventHandler,
} from "react";

export function useDelayedHover(delayMs = 500) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearTimer = () => {
    if (timerRef.current === null) return;
    clearTimeout(timerRef.current);
    timerRef.current = null;
  };

  useEffect(() => clearTimer, []);

  const onMouseEnter: MouseEventHandler<HTMLDivElement> = () => {
    if (open || timerRef.current !== null) return;
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      setOpen(true);
    }, delayMs);
  };

  const onMouseLeave: MouseEventHandler<HTMLDivElement> = () => {
    clearTimer();
    setOpen(false);
  };

  const onFocus: FocusEventHandler<HTMLDivElement> = () => {
    clearTimer();
    setOpen(true);
  };

  const onClick: MouseEventHandler<HTMLDivElement> = () => {
    clearTimer();
    setOpen(true);
  };

  const onBlur: FocusEventHandler<HTMLDivElement> = (event) => {
    if (
      event.relatedTarget instanceof Node
      && containerRef.current?.contains(event.relatedTarget)
    ) {
      return;
    }
    clearTimer();
    setOpen(false);
  };

  return {
    containerRef,
    open,
    hoverProps: {
      onBlur,
      onClick,
      onFocus,
      onMouseEnter,
      onMouseLeave,
    },
  };
}

