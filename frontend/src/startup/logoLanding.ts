/**
 * The big logo on the loading screen flies to its place in the dashboard header (owner, 2026-10-16).
 *
 * Nothing is cloned: the loading screen notes where its logo's text is, and when the dashboard mounts, its own header logo starts
 * there (same centre, same size) and glides to its place. The rest of the dashboard is held back until it lands, then fades in.
 * It uses the individual `translate` and `scale` properties, which compose with the Tailwind `transform` the logo already has.
 */

interface TextBox {
  cx: number;
  cy: number;
  width: number;
}

const MOVE_MS = 720;
const FADE_MS = 200;
const EASE = "cubic-bezier(0.22, 1, 0.36, 1)";

let pending: TextBox | null = null;

/** The box of the element's text (not its whole block, which is the full container width for a heading). */
function textBox(element: HTMLElement): TextBox {
  const range = element.ownerDocument.createRange();
  range.selectNodeContents(element);
  const rect = typeof range.getBoundingClientRect === "function" ? range.getBoundingClientRect() : element.getBoundingClientRect();
  const usable = rect.width > 0 ? rect : element.getBoundingClientRect();
  return { cx: usable.left + usable.width / 2, cy: usable.top + usable.height / 2, width: usable.width };
}

/** Called by the loading screen just before the dashboard replaces it. */
export function rememberLoadingLogo(logo: HTMLElement | null): void {
  pending = logo ? textBox(logo) : null;
}

export function forgetLoadingLogo(): void {
  pending = null;
}

/**
 * Called when the dashboard mounts. Returns a cleanup. Does nothing when there is no remembered loading logo (a reload of an
 * already-loaded page, a test), when the user prefers reduced motion, or when nothing has a size yet.
 */
export function landLogo(logo: HTMLElement | null, root: HTMLElement | null): (() => void) | undefined {
  const from = pending;
  if (!logo || !root || !from) return undefined;
  pending = null;
  if (typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return undefined;

  const to = textBox(logo);
  if (from.width < 1 || to.width < 1) return undefined;

  const scale = from.width / to.width;
  // The logo's own transform (its -18px lift) sits inside the scale, so it is scaled too: allow for it.
  const own = new DOMMatrixReadOnly(getComputedStyle(logo).transform === "none" ? undefined : getComputedStyle(logo).transform);
  const dx = from.cx - to.cx + (1 - scale) * own.m41;
  const dy = from.cy - to.cy + (1 - scale) * own.m42;

  root.dataset.landing = "moving";
  logo.style.transition = "none";
  logo.style.translate = `${dx}px ${dy}px`;
  logo.style.scale = String(scale);
  void logo.getBoundingClientRect(); // commit the starting position before the transition is switched on

  let finished = false;
  let fadeTimer: number | undefined;
  const frame = window.requestAnimationFrame(() => {
    logo.style.transition = `translate ${MOVE_MS}ms ${EASE}, scale ${MOVE_MS}ms ${EASE}`;
    logo.style.translate = "0px 0px";
    logo.style.scale = "1";
  });
  const landed = window.setTimeout(() => {
    finished = true;
    logo.style.removeProperty("transition");
    logo.style.removeProperty("translate");
    logo.style.removeProperty("scale");
    root.dataset.landing = "done";
    fadeTimer = window.setTimeout(() => delete root.dataset.landing, FADE_MS + 100);
  }, MOVE_MS + 30);

  return () => {
    window.cancelAnimationFrame(frame);
    window.clearTimeout(landed);
    window.clearTimeout(fadeTimer);
    if (!finished) {
      // Unmounted mid-flight (React StrictMode runs effects twice in development): put everything back and let the next mount play it.
      logo.style.removeProperty("transition");
      logo.style.removeProperty("translate");
      logo.style.removeProperty("scale");
      delete root.dataset.landing;
      pending = from;
    }
  };
}
