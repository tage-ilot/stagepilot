import { useCallback, useEffect, useRef, useState, type ComponentPropsWithoutRef, type CSSProperties } from "react";

import { cx } from "./cx";

export interface ScrollRegionProps extends ComponentPropsWithoutRef<"div"> {
  /** Which way the content scrolls. */
  axis?: "x" | "y";
  /** Classes for the bordered frame around the scroller (spacing, radius). `className` and the other props go on the scroller. */
  frameClassName?: string;
  /** Draw the border and the edge fades. Turn off for an empty list that has nothing to scroll. */
  framed?: boolean;
}

interface Edges {
  start: boolean;
  end: boolean;
  /** Thickness of the scrollbar on the other axis, so a fade stops short of it. */
  bar: number;
}

/**
 * A scrolling box drawn as an obvious bordered region (DESIGN.md "Scrolling boxes"). A soft light fade sits ON TOP of the content
 * (inputs and all) along any edge that has more to see, and disappears at the ends, so the user can tell it scrolls even where the
 * system hides scrollbars. The fade is a separate overlay that ignores the pointer, so nothing under it stops working.
 */
export function ScrollRegion({
  axis = "y", frameClassName, framed = true, className, children, onScroll, ...rest
}: ScrollRegionProps) {
  const scroller = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState<Edges>({ start: false, end: false, bar: 0 });

  const measure = useCallback(() => {
    const node = scroller.current;
    if (!node) return;
    const vertical = axis === "y";
    const position = vertical ? node.scrollTop : node.scrollLeft;
    const view = vertical ? node.clientHeight : node.clientWidth;
    const total = vertical ? node.scrollHeight : node.scrollWidth;
    const bar = vertical ? node.offsetWidth - node.clientWidth : node.offsetHeight - node.clientHeight;
    const next: Edges = { start: position > 1, end: position + view < total - 1, bar: Math.max(0, bar) };
    setEdges((current) => (current.start === next.start && current.end === next.end && current.bar === next.bar ? current : next));
  }, [axis]);

  useEffect(() => {
    const node = scroller.current;
    if (!node || !framed) return;
    measure();
    // Content grows and shrinks (cues added, the timeline zoomed, the window resized): measure again whenever it does.
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    resize?.observe(node);
    for (const child of Array.from(node.children)) resize?.observe(child);
    const mutate = typeof MutationObserver === "undefined" ? null : new MutationObserver(() => {
      if (resize) { resize.disconnect(); resize.observe(node); for (const child of Array.from(node.children)) resize.observe(child); }
      measure();
    });
    mutate?.observe(node, { childList: true, subtree: true, attributes: true, attributeFilter: ["style"] });
    return () => { resize?.disconnect(); mutate?.disconnect(); };
  }, [framed, measure]);

  const scrollerNode = (
    <div
      {...rest}
      className={className}
      onScroll={(event) => { measure(); onScroll?.(event); }}
      ref={scroller}
    >
      {children}
    </div>
  );
  if (!framed) return scrollerNode;

  const barStyle = { "--scroll-bar": `${edges.bar}px` } as CSSProperties;
  return (
    <div className={cx("scroll-region relative", frameClassName)} data-axis={axis} style={barStyle}>
      {scrollerNode}
      <div aria-hidden="true" className="scroll-fade scroll-fade-start" data-edge="start" data-visible={edges.start} />
      <div aria-hidden="true" className="scroll-fade scroll-fade-end" data-edge="end" data-visible={edges.end} />
    </div>
  );
}
