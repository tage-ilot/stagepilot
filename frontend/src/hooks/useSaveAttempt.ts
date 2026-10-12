import { useCallback, useEffect, useState } from "react";

/**
 * After a Save is refused because a field is invalid, show the field errors and move focus to the
 * first invalid control inside the panel. Errors only appear after an attempt, so a half-typed
 * field is not shouted at. `attempted` resets after a successful save or Discard.
 */
export function useSaveAttempt(panelId: string) {
  const [attempts, setAttempts] = useState(0);
  const [attempted, setAttempted] = useState(false);

  const refuse = useCallback(() => {
    setAttempted(true);
    setAttempts((count) => count + 1);
  }, []);
  const reset = useCallback(() => setAttempted(false), []);

  useEffect(() => {
    if (attempts === 0) return;
    const first = document.getElementById(panelId)?.querySelector<HTMLElement>('[aria-invalid="true"]');
    first?.focus();
  }, [attempts, panelId]);

  return { attempted, refuse, reset };
}
