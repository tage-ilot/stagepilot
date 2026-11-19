import { useEffect, useRef, type ReactNode } from "react";

import { REMOTE_ACCESS_OPEN_KEY } from "../components/BackendSetupPanel";
import { LIGHTING_CUE_BOARD_OPEN_KEY, LIGHTING_TIMELINE_OPEN_KEY } from "../components/LightingSetupPanel";
import { SONG_ORDER_OPEN_KEY } from "../components/playbackStatus";

/**
 * One thing a person does on a page: type into a field by its label, pick an option, or press a button by its words. The gallery runs
 * these on the REAL page after it is drawn, so a held change or an error is reached the way a person reaches it, not painted on.
 */
export type Step = { type: [label: string, value: string] } | { pick: [label: string, value: string] } | { press: string };

const nativeValue = (element: HTMLInputElement | HTMLSelectElement, value: string) => {
  const proto = element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value")?.set?.call(element, value);
  element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
};

const controlOf = (root: HTMLElement, label: string) => {
  const byAria = Array.from(root.querySelectorAll<HTMLInputElement | HTMLSelectElement>("input, select")).find((control) => control.getAttribute("aria-label") === label);
  if (byAria) return byAria;
  const caption = Array.from(root.querySelectorAll("label")).find((node) => node.textContent?.trim().startsWith(label));
  return caption?.htmlFor ? (document.getElementById(caption.htmlFor) as HTMLInputElement | HTMLSelectElement | null) : null;
};

function runStep(root: HTMLElement, step: Step): void {
  if ("press" in step) {
    Array.from(root.querySelectorAll("button")).find((button) => button.textContent?.replace(/[\u25B8\u25BE]/g, "").trim().startsWith(step.press) && !button.disabled)?.click();
    return;
  }
  const [label, value] = "type" in step ? step.type : step.pick;
  const control = controlOf(root, label);
  if (control) nativeValue(control, value);
}

/** Draws its children, then does the steps one after another (a beat between them, so each change is drawn before the next). */
export function Scripted({ steps, children }: { steps: Step[]; children: ReactNode }) {
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const timers: number[] = [];
    steps.forEach((step, index) => timers.push(window.setTimeout(() => { if (root.current) runStep(root.current, step); }, 30 * (index + 1))));
    return () => timers.forEach((timer) => window.clearTimeout(timer));
    // The steps are fixed for a sample: they run once, when it is drawn.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return <div ref={root}>{children}</div>;
}

/** Makes a sample start from what a new install does: the optional sections' remembered open/closed flags are put back (or set) before it is drawn. */
export function Fresh({ flags = {}, children }: { flags?: Record<string, boolean>; children: ReactNode }) {
  const seeded = useRef(false);
  if (!seeded.current) {
    seeded.current = true;
    try {
      for (const key of [REMOTE_ACCESS_OPEN_KEY, SONG_ORDER_OPEN_KEY, LIGHTING_TIMELINE_OPEN_KEY, LIGHTING_CUE_BOARD_OPEN_KEY]) window.localStorage.removeItem(key);
      for (const key of Object.keys(window.localStorage)) if (key.startsWith("gallery.optional.")) window.localStorage.removeItem(key);
      for (const [key, value] of Object.entries(flags)) window.localStorage.setItem(key, String(value));
    } catch { /* storage unavailable: the defaults apply */ }
  }
  return <>{children}</>;
}
