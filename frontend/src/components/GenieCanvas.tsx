import type { CSSProperties, Ref } from "react";

/**
 * Slot the shared genie canvas is appended into while a mesh animation runs (see lib/genieEngine.ts). Empty and
 * invisible otherwise; the canvas itself is `aria-hidden`, unfocusable and ignores the pointer.
 */
export function GenieCanvas({ slot }: { slot: Ref<HTMLDivElement> }) {
  const style: CSSProperties = { position: "absolute", left: 0, top: 0, width: "100%", height: 0, overflow: "visible", pointerEvents: "none" };
  return <div ref={slot} className="panel-reveal__fx" aria-hidden="true" style={style} />;
}
