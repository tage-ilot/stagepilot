import { act, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { usePanelPresence } from "../hooks/usePanelPresence";
import { resetGenieMeshCache } from "../hooks/useGenieMesh";
import { getReadout, resetGenieEngine, setGenieEnv } from "../lib/genieEngine";
import { PanelReveal } from "./PanelReveal";

vi.mock("../lib/panelSnapshot", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/panelSnapshot")>()),
  warmSnapshot: vi.fn(async () => undefined),
  capturePanel: vi.fn(async () => ({ canvas: document.createElement("canvas"), width: 100, height: 50, ms: 10, takenAt: performance.now() })),
}));

function fakeGl() {
  const spies = new Map<string, ReturnType<typeof vi.fn>>();
  const base: Record<string, unknown> = {
    NO_ERROR: 0, getShaderParameter: () => true, getProgramParameter: () => true, getParameter: () => 4096, getError: () => 0,
    isContextLost: () => false, createShader: () => ({}), createProgram: () => ({}), createBuffer: () => ({}), createTexture: () => ({}),
    getExtension: () => ({ loseContext: vi.fn() }),
  };
  return new Proxy(base, {
    get(target, name: string) {
      if (name in target) return target[name];
      if (!spies.has(name)) spies.set(name, vi.fn());
      return spies.get(name);
    },
  }) as unknown as WebGLRenderingContext;
}

function Harness() {
  const [on, setOn] = useState(false);
  const { shown, phase } = usePanelPresence(on ? "x" : null, 300);
  return (
    <>
      <button aria-controls="x-configuration" onClick={() => setOn(!on)}>card</button>
      <div>below</div>
      {shown && (
        <PanelReveal phase={phase} durationMs={300} trigger={() => document.querySelector<HTMLElement>("[aria-controls='x-configuration']")}>
          <section id="x-configuration"><input aria-label="field" /></section>
        </PanelReveal>
      )}
    </>
  );
}

const wrapper = () => document.querySelector<HTMLElement>(".panel-reveal");
const click = () => act(() => { screen.getByText("card").click(); });
const advance = async (ms: number) => { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); };

async function openAndSettle() {
  click();
  await advance(300); // opening -> open
  await advance(400); // idle capture
}

describe("PanelReveal genie mesh switch", () => {
  let media: { reduced: boolean; narrow: boolean };

  beforeEach(async () => {
    vi.useFakeTimers();
    resetGenieEngine();
    resetGenieMeshCache();
    const snapshotModule = await import("../lib/panelSnapshot");
    vi.mocked(snapshotModule.capturePanel).mockClear();
    media = { reduced: false, narrow: false };
    Object.defineProperty(window, "matchMedia", { configurable: true, writable: true, value: ((query: string) => ({
      matches: query.includes("reduced-motion") ? media.reduced : query.includes("max-width") ? media.narrow : false,
      addEventListener: () => undefined, removeEventListener: () => undefined,
    })) });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      const box = this.classList.contains("panel-reveal__clip") ? { width: 100, height: 50, left: 0, top: 20 }
        : this.hasAttribute("aria-controls") ? { width: 20, height: 10, left: 10, top: 0 } : { width: 100, height: 50, left: 0, top: 20 };
      return { ...box, right: box.left + box.width, bottom: box.top + box.height, x: box.left, y: box.top, toJSON: () => ({}) } as DOMRect;
    });
    setGenieEnv({
      createCanvas: () => Object.assign(document.createElement("canvas"), { getContext: () => fakeGl() }),
      readSwitch: () => null,
    });
  });
  afterEach(() => { Reflect.deleteProperty(window, "matchMedia"); vi.restoreAllMocks(); vi.useRealTimers(); resetGenieEngine(); });

  it("closes with the mesh after a snapshot exists: canvas inside the wrapper, aria-hidden, panel still inert, then collapsed", async () => {
    render(<Harness />);
    await openAndSettle();
    expect(wrapper()!.hasAttribute("data-mesh")).toBe(false);
    click(); // closing
    const element = wrapper()!;
    expect(element.getAttribute("data-mesh")).toBe("run");
    const canvas = element.querySelector("canvas")!;
    expect(canvas.getAttribute("aria-hidden")).toBe("true");
    expect(element.hasAttribute("inert")).toBe(true);
    expect(element.getAttribute("aria-hidden")).toBe("true");
    expect(element.style.height).toBe("50px"); // first frame: full height, the content below has not moved
    await advance(100);
    expect(Number.parseFloat(element.style.height)).toBe(50); // the lowest row lags: the content below starts moving later
    await advance(150);
    expect(Number.parseFloat(element.style.height)).toBeLessThan(50);
    await advance(60);
    expect(element.getAttribute("data-mesh")).toBe("collapsed");
    expect(element.querySelector("canvas")).toBeNull();
    expect(getReadout().state).toBe("ok");
    expect(document.querySelector("#x-configuration")).toBeNull(); // unmounted on schedule, as before
  });

  it("the first open has no snapshot yet and uses the CSS animation", async () => {
    render(<Harness />);
    click();
    expect(wrapper()!.hasAttribute("data-mesh")).toBe(false);
    expect(getReadout()).toMatchObject({ state: "fallback", reason: "no-snapshot" });
  });

  it("reopening uses the cached snapshot (mesh on the open as well)", async () => {
    render(<Harness />);
    await openAndSettle();
    click();
    await advance(310);
    expect(document.querySelector("#x-configuration")).toBeNull();
    click(); // open again
    expect(wrapper()!.getAttribute("data-mesh")).toBe("run");
    await advance(310);
    expect(wrapper()!.hasAttribute("data-mesh")).toBe(false);
    expect(wrapper()!.style.height).toBe("");
  });

  it("reduced motion never uses the mesh", async () => {
    media.reduced = true;
    render(<Harness />);
    await openAndSettle();
    click();
    expect(wrapper()!.hasAttribute("data-mesh")).toBe(false);
    expect(wrapper()!.querySelector("canvas")).toBeNull();
    expect(getReadout().reason).toBe("reduced-motion");
  });

  it("the single-column layout keeps the plain vertical emerge", async () => {
    media.narrow = true;
    render(<Harness />);
    await openAndSettle();
    click();
    expect(wrapper()!.hasAttribute("data-mesh")).toBe(false);
    expect(getReadout().reason).toBe("single-column");
  });

  it("falls back, silently, when WebGL is missing", async () => {
    setGenieEnv({ createCanvas: () => Object.assign(document.createElement("canvas"), { getContext: () => null }), readSwitch: () => null });
    render(<Harness />);
    await openAndSettle();
    click();
    expect(wrapper()!.hasAttribute("data-mesh")).toBe(false);
    expect(wrapper()!.getAttribute("data-phase")).toBe("closing");
    expect(getReadout().reason).toBe("no-webgl");
    await advance(300);
    expect(document.querySelector("#x-configuration")).toBeNull();
  });

  it("the kill switch 'off' keeps the CSS animation and takes no snapshot", async () => {
    setGenieEnv({ readSwitch: () => "off" });
    render(<Harness />);
    await openAndSettle();
    const { capturePanel } = await import("../lib/panelSnapshot");
    expect(capturePanel).not.toHaveBeenCalled();
    click();
    expect(wrapper()!.hasAttribute("data-mesh")).toBe(false);
  });

  it("an edit after the snapshot makes it stale: that close uses the CSS animation until it is re-captured", async () => {
    render(<Harness />);
    await openAndSettle();
    act(() => { screen.getByLabelText("field").dispatchEvent(new Event("input", { bubbles: true })); });
    click();
    expect(wrapper()!.hasAttribute("data-mesh")).toBe(false);
    expect(getReadout().reason).toBe("snapshot-stale");
  });

  it("captures at most once per 2 s and never while an animation runs", async () => {
    const { capturePanel } = await import("../lib/panelSnapshot");
    vi.mocked(capturePanel).mockClear();
    render(<Harness />);
    await openAndSettle();
    expect(capturePanel).toHaveBeenCalledTimes(1);
    for (let i = 0; i < 3; i += 1) {
      act(() => { screen.getByLabelText("field").dispatchEvent(new Event("input", { bubbles: true })); });
      await advance(500);
    }
    expect(capturePanel).toHaveBeenCalledTimes(1); // the 2 s gap has not passed
    await advance(2000);
    expect(capturePanel).toHaveBeenCalledTimes(2);
  });

  it("a reversal while closing continues from the current progress and still ends open", async () => {
    render(<Harness />);
    await openAndSettle();
    click(); // closing
    await advance(200);
    const midHeight = Number.parseFloat(wrapper()!.style.height);
    expect(midHeight).toBeGreaterThan(0);
    click(); // back to opening
    expect(wrapper()!.getAttribute("data-phase")).toBe("opening");
    expect(wrapper()!.getAttribute("data-mesh")).toBe("run");
    expect(Number.parseFloat(wrapper()!.style.height)).toBeCloseTo(midHeight, 0); // no jump
    await advance(310);
    expect(wrapper()!.hasAttribute("data-mesh")).toBe(false);
    expect(wrapper()!.getAttribute("data-phase")).toBe("open");
  });
});
