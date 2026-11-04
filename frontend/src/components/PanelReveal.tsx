import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";

import {
  NOTCH_ABOVE,
  NOTCH_WIDTH,
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

/** The panel's children, which fade in as the content (the panel itself when it has none). */
function fadeTargets(panel: HTMLElement): HTMLElement[] {
  const children = Array.from(panel.children).filter((child): child is HTMLElement => child instanceof HTMLElement);
  return children.length ? children : [panel];
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
  /** One fade per child of the panel. A child that is replaced while the animation runs (data arriving) hands its fade to the new one. */
  fades: Map<HTMLElement, Animation>;
  watcher: MutationObserver | null;
}

/**
 * Config panel open / close (DESIGN.md "Config panel reveal"). The panel is laid out at its final size at once and
 * only transform and opacity animate (Web Animations, one group): the outer wrapper scales the strip out of the card's
 * width, the inner wrapper grows it down from the card's bottom edge, the content below slides by the same amount (FLIP),
 * the content fades in late and a notch on the panel's top border points at the card (it rises out of the border at the end of
 * an open and sinks back into it at the start of a close). Nothing animates height, grid rows, margins, clip-path or filters,
 * so the dashboard is not laid out again per frame.
 */
export function PanelReveal({ phase, durationMs, trigger, triggerKey, children }: PanelRevealProps) {
  const root = useRef<HTMLDivElement>(null);
  const outer = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  // The notch: track (follows the panel's top edge as the gap to the card closes) > clip (static strip above the border) > rise (translateY) > notch (translateX, accent) > fill (static triangle)
  // > counterX > counterY > bg. The bg is a copy of the panel's own background at the panel's position; the counters cancel the
  // notch's movement so it stays lined up with the panel and the notch shows exactly the panel's colour.
  const track = useRef<HTMLSpanElement>(null);
  const clip = useRef<HTMLSpanElement>(null);
  const rise = useRef<HTMLSpanElement>(null);
  const notch = useRef<HTMLSpanElement>(null);
  const counterX = useRef<HTMLSpanElement>(null);
  const counterY = useRef<HTMLSpanElement>(null);
  const fill = useRef<HTMLSpanElement>(null);
  const closing = phase === "closing";

  const group = useRef<Group | null>(null);
  const slide = useRef<Animation[]>([]);
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
    current.watcher?.disconnect();
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
    syncFill();
    const frames = emergeFrames(current.geometry, current.built);
    for (const animation of current.below) {
      const effect = animation.effect as KeyframeEffect | null;
      if (effect && typeof effect.setKeyframes === "function") effect.setKeyframes(frames.below);
    }
  };

  const cancelSlide = () => {
    const current = slide.current;
    slide.current = [];
    for (const animation of current) { animation.onfinish = null; animation.cancel(); }
  };

  /** Copies the panel's background (colour, gradient wash) and its box onto the notch's fill, so both paint the same pixels (and the fill continues it upwards). */
  const syncFill = () => {
    const element = fill.current;
    const panel = panelElement();
    if (!element || !panel) return;
    const styles = getComputedStyle(panel);
    element.style.setProperty("left", `${Number.parseFloat(styles.marginLeft) || 0}px`);
    element.style.setProperty("width", `${panel.offsetWidth}px`);
    // The copy reaches NOTCH_ABOVE px above the panel (a transparent top border), so the notch's fill is the panel's background continued upwards.
    element.style.setProperty("height", `${panel.offsetHeight + NOTCH_ABOVE}px`);
    element.style.setProperty("border-width", `${(Number.parseFloat(styles.borderTopWidth) || 0) + NOTCH_ABOVE}px ${styles.borderRightWidth} ${styles.borderBottomWidth} ${styles.borderLeftWidth}`);
    element.style.setProperty("background-color", styles.backgroundColor);
    element.style.setProperty("background-image", styles.backgroundImage);
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
    syncFill();
    // Already there (or on its way there): leave a running slide alone. A re-measure with the same answer, e.g. the
    // ResizeObserver firing because the swapped-in panel has another height, must not cut the glide short.
    if (previous && Math.abs(previous.centre - centre) <= 0.5 && previous.accent === accent) return;

    let fromCentre = previous?.centre ?? centre;
    let fromAccent = previous?.accent ?? accent;
    const gliding = glide && previous !== null && canAnimate(element) && (Math.abs(previous.centre - centre) > 0.5 || previous.accent !== accent);
    if (gliding && slide.current.length) {
      // Retarget from where the notch is right now, so interrupting a slide never jumps.
      const computed = getComputedStyle(element);
      const x = parseTranslateX(computed.transform);
      if (x !== null) fromCentre = x + NOTCH_WIDTH / 2;
      fromAccent = computed.color || fromAccent;
    }
    cancelSlide();
    element.style.setProperty("--notch-x", `${centre - NOTCH_WIDTH / 2}px`);
    element.style.setProperty("--notch-accent", accent);
    placed.current = { centre, accent };
    if (!gliding) return;
    const frames = slideFrames(fromCentre, centre, fromAccent, accent);
    const timing = { duration: SLIDE_MS, easing: SLIDE_EASING };
    const animation = element.animate(frames.notch, timing);
    const counter = counterX.current?.animate(frames.counter, timing);
    slide.current = counter ? [animation, counter] : [animation];
    animation.onfinish = () => { if (slide.current[0] === animation) slide.current = []; };
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
    const risen = rise.current;
    const risenCounter = counterY.current;
    const cover = clip.current;
    const following = track.current;
    if (!host || !wrapperOuter || !wrapperInner || !panel || !card || !element || !risen || !risenCounter || !cover || !following || !canAnimate(wrapperOuter)) return;
    const duration = latest.current.durationMs;
    const animations: Animation[] = [];
    const belowAnimations: Animation[] = [];
    const touched: HTMLElement[] = [];
    let measured: EmergeGeometry | null = null;
    const fades = new Map<HTMLElement, Animation>();
    const add = (target: HTMLElement, frames: Keyframe[], easing: string, hint: string) => {
      target.style.setProperty("will-change", hint);
      touched.push(target);
      const animation = target.animate(frames, { duration, easing, fill: "both" });
      animations.push(animation);
      return animation;
    };
    let contentFrames: Keyframe[] = [];

    if (matches("(prefers-reduced-motion: reduce)")) {
      add(wrapperOuter, fadeFrames(direction), "linear", "opacity");
      add(cover, fadeFrames(direction), "linear", "opacity");
    } else {
      const geometry = emergeGeometry(card.getBoundingClientRect(), panel.getBoundingClientRect());
      if (!geometry) return;
      measured = geometry;
      // Single column: the card spans the row, so there is nothing to spread out of; the strip only grows down.
      const spread = !matches("(max-width: 639px)");
      const frames = emergeFrames(geometry, direction, spread);
      if (spread) add(wrapperOuter, frames.outer, frames.easing.outer, "transform");
      add(wrapperInner, frames.inner, frames.easing.inner, "transform");
      for (const below of elementsBelow(host)) { add(below, frames.below, frames.easing.below, "transform"); belowAnimations.push(animations[animations.length - 1]!); }
      contentFrames = frames.content;
      for (const child of fadeTargets(panel)) fades.set(child, add(child, frames.content, frames.easing.content, "opacity"));
      add(following, frames.track, frames.easing.inner, "transform");
      add(risen, frames.notch, frames.easing.notch, "transform");
      add(risenCounter, frames.notchCounter, frames.easing.notch, "transform");
    }

    const entry: Group = { animations, direction, built: direction, geometry: measured, below: belowAnimations, touched, fades, watcher: null };
    group.current = entry;
    const settle = () => {
      if (group.current !== entry || !animations.every((animation) => animation.playState === "finished")) return;
      // An open leaves its final styles to plain CSS and drops every animation. A close keeps its last frame until the
      // panel unmounts (the presence timer), otherwise the panel would flash back to full size first.
      if (entry.direction === "open") clearGroup();
      if (resizeWhileBusy.current) { resizeWhileBusy.current = false; if (latest.current.phase !== "closing") placeNotch(false); }
    };
    for (const animation of animations) animation.onfinish = settle;

    // The panel's children can be replaced while it opens (the Lighting panel swaps a section once its settings arrive). A
    // fade on a removed child would keep ticking on the main thread without ever painting, and the new child would show its
    // text at full opacity while the strip is still narrow. So the new child takes over the fade, in step with the others.
    if (fades.size && typeof MutationObserver === "function") {
      entry.watcher = new MutationObserver(() => {
        if (group.current !== entry) return;
        const present = new Set(fadeTargets(panel));
        for (const [child, animation] of entry.fades) {
          if (present.has(child)) continue;
          animation.onfinish = null;
          animation.cancel();
          entry.fades.delete(child);
          const at = animations.indexOf(animation);
          if (at >= 0) animations.splice(at, 1);
          child.style.removeProperty("will-change");
        }
        const reference = animations[0];
        for (const child of present) {
          if (entry.fades.has(child) || !canAnimate(child)) continue;
          child.style.setProperty("will-change", "opacity");
          touched.push(child);
          const animation = child.animate(contentFrames, { duration, easing: "linear", fill: "both" });
          if (reference) { animation.currentTime = reference.currentTime; animation.playbackRate = reference.playbackRate; }
          animation.onfinish = settle;
          animations.push(animation);
          entry.fades.set(child, animation);
        }
      });
      entry.watcher.observe(panel, { childList: true });
    }
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
      <span ref={track} className="panel-reveal__notch-track" aria-hidden="true">
        <span ref={clip} className="panel-reveal__notch-clip">
          <span ref={rise} className="panel-reveal__notch-rise">
            <span ref={notch} className="panel-reveal__notch">
              <span className="panel-reveal__notch-fill">
                <span ref={counterX} className="panel-reveal__notch-cx">
                  <span ref={counterY} className="panel-reveal__notch-cy">
                    <span ref={fill} className="panel-reveal__notch-bg" />
                  </span>
                </span>
              </span>
            </span>
          </span>
        </span>
      </span>
    </div>
  );
}
