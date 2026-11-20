import { useEffect, useRef, type ReactNode } from "react";

import { REMOTE_ACCESS_OPEN_KEY } from "../components/BackendSetupPanel";
import { LIGHTING_CUE_BOARD_OPEN_KEY, LIGHTING_TIMELINE_OPEN_KEY } from "../components/LightingSetupPanel";
import { SONG_ORDER_OPEN_KEY } from "../components/playbackStatus";

/**
 * One thing a person does on a page: type into a field by its label, pick an option, or press a button by its words. The gallery runs
 * these on the REAL page after it is drawn, so a held change or an error is reached the way a person reaches it, not painted on.
 */
export type Step = { type: [label: string, value: string] } | { pick: [label: string, value: string] } | { press: string } | { toggle: string } | { key: [label: string, key: string] };

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

/** Does the step if it can be done now. `true`: done; `false`: what it needs (a control, an option, an enabled button) is not drawn yet. */
function runStep(root: HTMLElement, step: Step): boolean {
  if ("key" in step) {
    // A key pressed in a control, found by its label (ArrowDown in an address box opens its list).
    const target = controlOf(root, step.key[0]);
    if (!target) return false;
    target.dispatchEvent(new KeyboardEvent("keydown", { key: step.key[1], bubbles: true, cancelable: true }));
    return true;
  }
  if ("toggle" in step) {
    // A tick box, found by the words of its label.
    const caption = Array.from(root.querySelectorAll("label")).find((node) => node.textContent?.trim() === step.toggle);
    const box = caption?.htmlFor ? (document.getElementById(caption.htmlFor) as HTMLInputElement | null) : null;
    if (!box || box.disabled) return false;
    box.click();
    return true;
  }
  if ("press" in step) {
    const button = Array.from(root.querySelectorAll("button")).find((candidate) => candidate.textContent?.replace(/[\u25B8\u25BE]/g, "").trim().startsWith(step.press) && !candidate.disabled);
    button?.click();
    return Boolean(button);
  }
  const [label, value] = "type" in step ? step.type : step.pick;
  const control = controlOf(root, label);
  if (!control) return false;
  if (control instanceof HTMLSelectElement && !Array.from(control.options).some((option) => option.value === value)) return false;
  nativeValue(control, value);
  return true;
}

/**
 * Draws its children, then does the steps one after another (a beat between them, so each change is drawn before the next). `gap` is the
 * beat in milliseconds. `patient`: a step whose control, option or button is not there yet (it comes from a request that has not answered)
 * is tried again every beat, for up to ten seconds, before the next one starts; so a slow computer reaches the same state.
 */
export function Scripted({ steps, children, gap = 30, patient = false }: { steps: Step[]; children: ReactNode; gap?: number; patient?: boolean }) {
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let stopped = false;
    let timer = 0;
    const next = (index: number, tries: number) => {
      if (stopped || index >= steps.length) return;
      timer = window.setTimeout(() => {
        const done = root.current ? runStep(root.current, steps[index]!) : true;
        if (done || !patient || tries * Math.max(gap, 30) > 10_000) next(index + 1, 0);
        else next(index, tries + 1);
      }, Math.max(gap, 30));
    };
    next(0, 0);
    return () => { stopped = true; window.clearTimeout(timer); };
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
