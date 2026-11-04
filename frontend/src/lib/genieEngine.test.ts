import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  BAIL_FRAMES, BailMonitor, RELEASE_AFTER_MS, SNAPSHOT_BUDGET_MS, blockedReason, disableForSession, getReadout, isAnimating,
  releasePending, rendererAlive, reportSnapshot, resetGenieEngine, setGenieEnv, startGenie, type GenieStart,
} from "./genieEngine";
import type { PanelSnapshot } from "./panelSnapshot";

/** A WebGL stand-in: every method is a spy, the few queries the engine reads return sane values. */
function fakeGl(overrides: Record<string, unknown> = {}) {
  const spies = new Map<string, ReturnType<typeof vi.fn>>();
  const loseContext = vi.fn();
  const base: Record<string, unknown> = {
    NO_ERROR: 0,
    getShaderParameter: () => true,
    getProgramParameter: () => true,
    getParameter: () => 4096,
    getError: () => 0,
    isContextLost: () => false,
    createShader: () => ({}), createProgram: () => ({}), createBuffer: () => ({}), createTexture: () => ({}),
    getExtension: () => ({ loseContext }),
    ...overrides,
  };
  const gl = new Proxy(base, {
    get(target, name: string) {
      if (name in target) return target[name];
      if (!spies.has(name)) spies.set(name, vi.fn());
      return spies.get(name);
    },
  });
  return { gl: gl as unknown as WebGLRenderingContext, spies, loseContext };
}

function harness(options: { gl?: ReturnType<typeof fakeGl>; contextAvailable?: boolean; mode?: string | null } = {}) {
  const fake = options.gl ?? fakeGl();
  let clock = 0;
  let nextId = 1;
  const frames: Array<(time: number) => void> = [];
  const timers = new Map<number, { at: number; cb: () => void }>();
  const logs: string[] = [];
  const canvases: HTMLCanvasElement[] = [];
  let mode: string | null = options.mode ?? null;
  setGenieEnv({
    now: () => clock,
    raf: (cb) => { frames.push(cb); return frames.length; },
    caf: () => { /* frames are dropped by the test driver */ },
    setTimeout: (cb, ms) => { const id = nextId++; timers.set(id, { at: clock + ms, cb }); return id; },
    clearTimeout: (id) => { timers.delete(id); },
    createCanvas: () => {
      const canvas = document.createElement("canvas");
      canvas.getContext = (() => (options.contextAvailable === false ? null : fake.gl)) as never;
      canvases.push(canvas);
      return canvas;
    },
    readSwitch: () => mode,
    log: (message) => logs.push(message),
  });
  const advanceTimers = (ms: number) => {
    clock += ms;
    for (const [id, timer] of [...timers]) if (timer.at <= clock) { timers.delete(id); timer.cb(); }
  };
  /** Runs one animation frame, `frameMs` after the previous one. */
  const frame = (frameMs: number) => {
    clock += frameMs;
    frames.shift()?.(clock);
  };
  const runToEnd = () => { frame(0); for (let i = 0; i < 80 && frames.length; i += 1) frame(10); };
  return { fake, frame, runToEnd, advanceTimers, logs, canvases, setMode: (value: string | null) => { mode = value; }, pending: () => frames.length };
}

const snapshot = (width = 400, height = 200): PanelSnapshot => ({ canvas: document.createElement("canvas"), width, height, ms: 12, takenAt: 0 });

function start(overrides: Partial<GenieStart> = {}) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const events: string[] = [];
  const layouts: number[] = [];
  const run = startGenie({
    host, snapshot: snapshot(), card: { cx: 100, y: -12, width: 80 }, opening: false, fromProgress: 0, durationMs: 300,
    onLayout: (bottom) => layouts.push(bottom), onEnd: (result) => events.push(result), ...overrides,
  });
  return { run, host, events, layouts };
}

describe("genie engine", () => {
  beforeEach(() => resetGenieEngine());
  afterEach(() => { resetGenieEngine(); document.body.innerHTML = ""; });

  it("draws the first frame at once, runs on rAF, ends 'done', removes the canvas and reports ok", () => {
    const h = harness();
    const { run, host, events, layouts } = start();
    expect(run).not.toBeNull();
    expect(isAnimating()).toBe(true);
    const canvas = host.querySelector("canvas")!;
    expect(canvas.getAttribute("aria-hidden")).toBe("true");
    expect(canvas.tabIndex).toBe(-1);
    expect(canvas.style.pointerEvents).toBe("none");
    expect(canvas.width).toBe(400);
    expect(canvas.height).toBe(200 + 14); // panel height + the room above it up to the card edge
    expect(layouts[0]).toBeCloseTo(200, 6); // p = 0: content below keeps the full height
    h.runToEnd();
    expect(events).toEqual(["done"]);
    expect(layouts.at(-1)).toBe(0);
    expect(host.querySelector("canvas")).toBeNull();
    expect(isAnimating()).toBe(false);
    expect(getReadout()).toMatchObject({ state: "ok", reason: "", animations: 1, canvasSize: "400x214", triangles: 128 });
    expect(h.pending()).toBe(0); // no rAF loop when idle
  });

  it("opening runs from collapsed to open and ends with the full height", () => {
    const h = harness();
    const { events, layouts } = start({ opening: true, fromProgress: 1 });
    expect(layouts[0]).toBe(0);
    h.runToEnd();
    expect(events).toEqual(["done"]);
    expect(layouts.at(-1)).toBeCloseTo(200, 6);
  });

  it("releases the context (WEBGL_lose_context) and the texture about 5 s after the last animation", () => {
    const h = harness();
    start();
    h.runToEnd();
    expect(rendererAlive()).toBe(true);
    expect(releasePending()).toBe(true);
    h.advanceTimers(RELEASE_AFTER_MS - 1);
    expect(h.fake.loseContext).not.toHaveBeenCalled();
    h.advanceTimers(1);
    expect(h.fake.loseContext).toHaveBeenCalledTimes(1);
    expect(h.fake.spies.get("deleteTexture")).toHaveBeenCalled();
    expect(rendererAlive()).toBe(false);
    expect(getReadout().contextAlive).toBe(false);
  });

  it("a new animation within the 5 s keeps the same context and cancels the release", () => {
    const h = harness();
    start();
    h.runToEnd();
    h.advanceTimers(3000);
    start();
    expect(releasePending()).toBe(false);
    h.advanceTimers(5000);
    expect(h.fake.loseContext).not.toHaveBeenCalled();
    expect(h.canvases).toHaveLength(1);
  });

  it("bails out after 3 consecutive slow frames in the first half and reports 'bailed'", () => {
    const h = harness();
    const { events, host } = start({ durationMs: 1000 });
    h.frame(0);
    h.frame(40); h.frame(40);
    expect(events).toEqual([]);
    h.frame(40);
    expect(events).toEqual(["bailed"]);
    expect(host.querySelector("canvas")).toBeNull();
    expect(getReadout().state).toBe("bailed");
    expect(getReadout().reason).toMatch(/slow frames/);
  });

  it("does not bail when the slow frames are not consecutive, or come in the second half", () => {
    const h = harness();
    const { events } = start({ durationMs: 1000 });
    h.frame(0);
    h.frame(40); h.frame(40); h.frame(16); h.frame(40); h.frame(40); h.frame(16);
    expect(events).toEqual([]);
    h.frame(400); // a long frame ends the first half; slow frames after that never bail
    for (let i = 0; i < 20 && h.pending(); i += 1) h.frame(40);
    expect(events).toEqual(["done"]);
  });

  it("'on' forces the mesh: no bail-out and no session disable", () => {
    const h = harness({ mode: "on" });
    expect(reportSnapshot({ ms: 500, width: 10, height: 10 })).toBe(true);
    const { events } = start({ durationMs: 1000 });
    h.frame(0);
    for (let i = 0; i < 6; i += 1) h.frame(60);
    expect(events).toEqual([]);
  });

  it("a snapshot that took more than 40 ms disables the mesh for the session and logs once", () => {
    const h = harness();
    expect(reportSnapshot({ ms: SNAPSHOT_BUDGET_MS, width: 10, height: 10 })).toBe(true);
    expect(blockedReason()).toBeNull();
    expect(reportSnapshot({ ms: SNAPSHOT_BUDGET_MS + 1, width: 10, height: 10 })).toBe(false);
    expect(blockedReason()).toBe("disabled");
    disableForSession("again");
    expect(h.logs).toHaveLength(1);
    expect(getReadout()).toMatchObject({ state: "disabled", sessionDisabled: true });
    expect(start().run).toBeNull();
    expect(h.canvases).toHaveLength(0); // no context was ever created
  });

  it("the kill switch: 'off' forces the fallback, anything else leaves auto mode", () => {
    const h = harness({ mode: "off" });
    expect(blockedReason()).toBe("kill-switch");
    expect(start().run).toBeNull();
    expect(getReadout()).toMatchObject({ state: "fallback", reason: "kill-switch", mode: "off" });
    expect(h.canvases).toHaveLength(0);
    h.setMode("whatever");
    expect(blockedReason()).toBeNull();
    expect(getReadout().mode).toBe("auto");
  });

  it("falls back quietly when WebGL is unavailable", () => {
    const h = harness({ contextAvailable: false });
    const { run, host } = start();
    expect(run).toBeNull();
    expect(getReadout()).toMatchObject({ state: "fallback", reason: "no-webgl" });
    expect(host.querySelector("canvas")).toBeNull();
    expect(h.logs).toHaveLength(0);
  });

  it("falls back when the texture upload throws or the texture is too large", () => {
    harness({ gl: fakeGl({ texImage2D: () => { throw new Error("SecurityError"); } }) });
    expect(start().run).toBeNull();
    expect(getReadout().reason).toMatch(/^texture-upload/);
    resetGenieEngine();
    harness({ gl: fakeGl({ getParameter: () => 256 }) });
    expect(start().run).toBeNull();
    expect(getReadout().reason).toMatch(/MAX_TEXTURE_SIZE/);
  });

  it("bails when the context is lost mid-run", () => {
    let lost = false;
    const h = harness({ gl: fakeGl({ isContextLost: () => lost }) });
    const { events } = start();
    h.frame(0);
    lost = true;
    h.frame(16);
    expect(events).toEqual(["bailed"]);
    expect(getReadout().reason).toBe("context-lost");
  });

  it("cancel() stops the loop, removes the canvas and returns the current progress", () => {
    const h = harness();
    const { run, host } = start({ durationMs: 1000 });
    h.frame(0);
    h.frame(500);
    const progress = run!.cancel();
    expect(progress).toBeGreaterThan(0);
    expect(progress).toBeLessThan(1);
    expect(host.querySelector("canvas")).toBeNull();
    expect(isAnimating()).toBe(false);
  });
});

describe("BailMonitor", () => {
  it("needs BAIL_FRAMES slow frames in a row, only in the first half", () => {
    const monitor = new BailMonitor();
    expect(BAIL_FRAMES).toBe(3);
    expect([40, 40, 40].map((ms) => monitor.push(ms, 0.2))).toEqual([false, false, true]);
    const second = new BailMonitor();
    expect([40, 40, 40, 40].map((ms) => second.push(ms, 0.6))).toEqual([false, false, false, false]);
    const mixed = new BailMonitor();
    expect([40, 40, 10, 40, 40].map((ms) => mixed.push(ms, 0.1))).toEqual([false, false, false, false, false]);
    const edge = new BailMonitor();
    expect([28, 28, 28].map((ms) => edge.push(ms, 0.1))).toEqual([false, false, false]); // exactly 28 ms is within budget
  });
});
