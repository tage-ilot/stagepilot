import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";

import {
  NOTCH_SIZE,
  SLIDE_EASING,
  SLIDE_MS,
  emergeFrames,
  emergeGeometry,
  type EmergeGeometry,
  fadeFrames,
  notchCentre,
  parseTranslateX,
  slideFrames,
  type EmergeDirection,
} from "../lib/panelEmerge";
import type { PresencePhase } from "../hooks/usePanelPresence";

export interface PanelRevealProps {
  phase: PresencePhase;
  durationMs: number;
  /** The connection card this panel belongs to; the panel emerges from, and returns into, it. */
  trigger: () => HTMLElement | null;
  /**
   * Identifies the card behind the panel. While the panel is open, a changed key makes the notch glide from the old
   * card's centre to the new one (the panel itself is not replayed).
   */
  triggerKey?: string;
  children: ReactNode;
}

/** Elements that move with the panel's lower edge: everything after the reveal in the flow. */
function elementsBelow(root: HTMLElement): HTMLElement[] {
  const found: HTMLElement[] = [];
  for (let element = root.nextElementSibling; element; element = element.nextElementSibling) {
    if (!(element instanceof HTMLElement)) continue;
    const position = getComputedStyle(element).position;
    if (position === "fixed" || position === "absolute") continue;
    found.push(element);
  }
  return found;
}

const canAnimate = (element: Element) => typeof (element as HTMLElement).animate === "function";
const matches = (query: string) => typeof window.matchMedia === "function" && window.matchMedia(query).matches;

interface Group {
  animations: Animation[];
  /** Which way the group currently plays (a mid-flight reverse flips it). */
  direction: EmergeDirection;
  /** The direction its keyframes were built for. */
  built: EmergeDirection;
  geometry: EmergeGeometry | null;
  /** The animations that move the content below the panel; they are refitted if the panel's height changes mid-flight. */
  below: Animation[];
  touched: HTMLElement[];
}

/**
 * Config panel open / close (DESIGN.md "Config panel reveal"). The panel is laid out at its final size at once and
 * only transform and opacity animate (Web Animations, one group): the outer wrapper scales the strip out of the card's
 * width, the inner wrapper grows it down from the card's bottom edge, the content below slides by the same amount (FLIP),
 * the content fades in late and a notch on the panel's top border points at the card. Nothing animates height, grid rows,
 * margins, clip-path or filters, so the dashboard is not laid out again per frame.
 */
export function PanelReveal({ phase, durationMs, trigger, triggerKey, children }: PanelRevealProps) {
  const root = useRef<HTMLDivElement>(null);
  const outer = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const notch = useRef<HTMLSpanElement>(null);
  const closing = phase === "closing";

  const group = useRef<Group | null>(null);
  const slide = useRef<Animation | null>(null);
  const placed = useRef<{ centre: number; accent: string } | null>(null);
  const latest = useRef({ phase, durationMs, trigger });
  latest.current = { phase, durationMs, trigger };
  const lastResize = useRef(0);
  const resizeWhileBusy = useRef(false);
  const shownKey = useRef<{ key: string | undefined } | null>(null);

  const panelElement = () => (content.current?.firstElementChild as HTMLElement | null) ?? content.current;

  const running = () => group.current !== null && group.current.animations.some((animation) => animation.playState === "running");

  /** Drops the whole animation group and its will-change hints. */
  const clearGroup = () => {
    const current = group.current;
    group.current = null;
    if (!current) return;
    for (const animation of current.animations) { animation.onfinish = null; animation.cancel(); }
    for (const element of current.touched) element.style.removeProperty("will-change");
  };

  /**
   * The panel's content can still settle after the animation started (data arriving, fonts). Its height is read from layout
   * (offsetHeight, unaffected by the transforms) and the FLIP offset of the content below is refitted in place, so the gap
   * under the panel stays constant and nothing jumps when the animation ends.
   */
  const refit = () => {
    const current = group.current;
    const panel = panelElement();
    if (!current?.geometry || !panel) return;
    const height = panel.offsetHeight;
    if (!(height > 0) || Math.abs(height - current.geometry.height) < 1) return;
    current.geometry = { ...current.geometry, height };
    const frames = emergeFrames(current.geometry, current.built);
    for (const animation of current.below) {
      const effect = animation.effect as KeyframeEffect | null;
      if (effect && typeof effect.setKeyframes === "function") effect.setKeyframes(frames.below);
    }
  };

  const cancelSlide = () => {
    const current = slide.current;
    slide.current = null;
    if (current) { current.onfinish = null; current.cancel(); }
  };

  /**
   * Puts the notch on the current card's centre. `glide` slides it there (from where it is now, even mid-slide);
   * otherwise it snaps. Uses the root's rect (never transformed) so it is safe while the panel animates.
   */
  const placeNotch = (glide: boolean) => {
    const element = notch.current;
    const host = root.current;
    const panel = panelElement();
    const card = latest.current.trigger();
    if (!element || !host || !panel || !card) return;
    const centre = notchCentre(card.getBoundingClientRect(), host.getBoundingClientRect());
    const styles = getComputedStyle(panel);
    const accent = styles.borderTopColor || "currentColor";
    const marginTop = Number.parseFloat(styles.marginTop) || 0;
    const previous = placed.current;
    host.style.setProperty("--emerge-top", `${marginTop}px`);
    // Already there (or on its way there): leave a running slide alone. A re-measure with the same answer, e.g. the
    // ResizeObserver firing because the swapped-in panel has another height, must not cut the glide short.
    if (previous && Math.abs(previous.centre - centre) <= 0.5 && previous.accent === accent) return;

    let fromCentre = previous?.centre ?? centre;
    let fromAccent = previous?.accent ?? accent;
    const gliding = glide && previous !== null && canAnimate(element) && (Math.abs(previous.centre - centre) > 0.5 || previous.accent !== accent);
    if (gliding && slide.current) {
      // Retarget from where the notch is right now, so interrupting a slide never jumps.
      const computed = getComputedStyle(element);
      const x = parseTranslateX(computed.transform);
      if (x !== null) fromCentre = x + NOTCH_SIZE / 2;
      fromAccent = computed.borderTopColor || fromAccent;
    }
    cancelSlide();
    element.style.setProperty("--notch-x", `${centre - NOTCH_SIZE / 2}px`);
    element.style.setProperty("--notch-accent", accent);
    placed.current = { centre, accent };
    if (!gliding) return;
    const animation = element.animate(slideFrames(fromCentre, centre, fromAccent, accent), { duration: SLIDE_MS, easing: SLIDE_EASING });
    slide.current = animation;
    animation.onfinish = () => { if (slide.current === animation) slide.current = null; };
  };

  /** Builds the animation group for one direction, from freshly measured geometry. */
  const start = (direction: EmergeDirection) => {
    clearGroup();
    const host = root.current;
    const wrapperOuter = outer.current;
    const wrapperInner = inner.current;
    const panel = panelElement();
    const card = latest.current.trigger();
    const element = notch.current;
    if (!host || !wrapperOuter || !wrapperInner || !panel || !card || !element || !canAnimate(wrapperOuter)) return;
    const duration = latest.current.durationMs;
    const animations: Animation[] = [];
    const belowAnimations: Animation[] = [];
    const touched: HTMLElement[] = [];
    let measured: EmergeGeometry | null = null;
    const add = (target: HTMLElement, frames: Keyframe[], easing: string, hint: string) => {
      target.style.setProperty("will-change", hint);
      touched.push(target);
      animations.push(target.animate(frames, { duration, easing, fill: "both" }));
    };

    if (matches("(prefers-reduced-motion: reduce)")) {
      add(wrapperOuter, fadeFrames(direction), "linear", "opacity");
      add(element, fadeFrames(direction), "linear", "opacity");
    } else {
      const geometry = emergeGeometry(card.getBoundingClientRect(), panel.getBoundingClientRect());
      if (!geometry) return;
      measured = geometry;
      const frames = emergeFrames(geometry, direction);
      // Single column: the card spans the row, so there is nothing to spread out of; the strip only grows down.
      if (!matches("(max-width: 639px)")) add(wrapperOuter, frames.outer, frames.easing.outer, "transform");
      add(wrapperInner, frames.inner, frames.easing.inner, "transform");
      for (const below of elementsBelow(host)) { add(below, frames.below, frames.easing.below, "transform"); belowAnimations.push(animations[animations.length - 1]!); }
      const faded = Array.from(panel.children).filter((child): child is HTMLElement => child instanceof HTMLElement);
      for (const child of faded.length ? faded : [panel]) add(child, frames.content, frames.easing.content, "opacity");
      add(element, frames.notch, frames.easing.notch, "opacity");
    }

    const entry: Group = { animations, direction, built: direction, geometry: measured, below: belowAnimations, touched };
    group.current = entry;
    const settle = () => {
      if (group.current !== entry || !animations.every((animation) => animation.playState === "finished")) return;
      // An open leaves its final styles to plain CSS and drops every animation. A close keeps its last frame until the
      // panel unmounts (the presence timer), otherwise the panel would flash back to full size first.
      if (entry.direction === "open") clearGroup();
      if (resizeWhileBusy.current) { resizeWhileBusy.current = false; if (latest.current.phase !== "closing") placeNotch(false); }
    };
    for (const animation of animations) animation.onfinish = settle;
  };

  // Closing: if focus is inside the panel, hand it back to the card before the panel goes inert.
  useLayoutEffect(() => {
    if (!closing) return;
    const element = root.current;
    if (element && element.contains(document.activeElement)) trigger()?.focus();
  }, [closing, trigger]);

  // Phase changes: start the animation, or turn the running one round (its progress continues from where it is).
  useLayoutEffect(() => {
    if (phase !== "opening" && phase !== "closing") return;
    const direction: EmergeDirection = phase === "opening" ? "open" : "close";
    const current = group.current;
    if (current && running()) {
      if (current.direction !== direction) {
        current.direction = direction;
        for (const animation of current.animations) animation.reverse();
      }
      return;
    }
    if (direction === "open") placeNotch(false);
    else cancelSlide();
    start(direction);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reads refs only; must run once per phase change
  }, [phase]);

  // Another card was picked while the panel is open: the notch glides there. Never on the first open, never while closing.
  useLayoutEffect(() => {
    const previous = shownKey.current;
    shownKey.current = { key: triggerKey };
    if (previous === null) { if (placed.current === null) placeNotch(false); return; }
    if (previous.key === triggerKey || latest.current.phase === "closing" || latest.current.phase === "closed") return;
    placeNotch(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the card only
  }, [triggerKey]);

  // Layout changes (window resize, panel width): keep the notch under the card. A burst of events (a drag) snaps; a lone one glides.
  useEffect(() => {
    const onChange = () => {
      if (latest.current.phase === "closing") return;
      if (running()) { refit(); resizeWhileBusy.current = true; return; }
      const now = performance.now();
      const burst = now - lastResize.current < 150;
      lastResize.current = now;
      placeNotch(!burst);
    };
    window.addEventListener("resize", onChange);
    const observer = typeof ResizeObserver === "function" && root.current ? new ResizeObserver(onChange) : null;
    if (observer && root.current) observer.observe(root.current);
    return () => { window.removeEventListener("resize", onChange); observer?.disconnect(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reads refs only
  }, []);

  useEffect(() => () => { clearGroup(); cancelSlide(); }, []);

  return (
    <div
      ref={root}
      className="panel-reveal"
      data-phase={phase}
      aria-hidden={closing || undefined}
      inert={closing}
    >
      <div ref={outer} className="panel-reveal__x">
        <div ref={inner} className="panel-reveal__y">
          <div ref={content} className="panel-reveal__panel">{children}</div>
        </div>
      </div>
      <span ref={notch} className="panel-reveal__notch" aria-hidden="true" />
    </div>
  );
}
