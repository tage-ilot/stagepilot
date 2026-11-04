/**
 * Genie mesh engine: one shared WebGL canvas that warps a panel snapshot while a config panel opens or closes.
 * Everything doubtful falls back (returns null / ends "bailed") and the caller keeps the CSS reveal. DESIGN.md
 * "Config panel reveal" describes the behaviour; `window.__spGenie` is the devtools readout.
 */
import { GENIE_K, MESH_COLS, MESH_ROWS, buildMesh, genieProgress, layoutBottom, type CardTarget, type MeshGeometry } from "./genieMesh";
import type { PanelSnapshot } from "./panelSnapshot";

export const SNAPSHOT_BUDGET_MS = 40;
export const FRAME_BUDGET_MS = 28;
export const BAIL_FRAMES = 3;
export const RELEASE_AFTER_MS = 5000;
export const FALLBACK_FADE_MS = 120;

export type GenieState = "idle" | "ok" | "bailed" | "disabled" | "fallback";
export interface GenieReadout {
  state: GenieState;
  /** Why the last animation did not use the mesh (or bailed); "" when ok. */
  reason: string;
  mode: "auto" | "on" | "off";
  snapshotMs: number | null;
  /** Largest snapshot cost seen, ms. */
  worstSnapshotMs: number | null;
  snapshotSize: string | null;
  canvasSize: string | null;
  uploadMs: number | null;
  meshRows: number;
  meshCols: number;
  triangles: number;
  frames: number;
  worstFrameMs: number | null;
  meanFrameMs: number | null;
  /** True once the mesh was switched off for this session (a capture over budget, or repeated failures). */
  sessionDisabled: boolean;
  animations: number;
  contextAlive: boolean;
}

export interface GenieEnv {
  now: () => number;
  raf: (cb: (time: number) => void) => number;
  caf: (id: number) => void;
  setTimeout: (cb: () => void, ms: number) => number;
  clearTimeout: (id: number) => void;
  createCanvas: () => HTMLCanvasElement;
  readSwitch: () => string | null;
  log: (message: string) => void;
}

const browserEnv = (): GenieEnv => ({
  now: () => performance.now(),
  raf: (cb) => window.requestAnimationFrame(cb),
  caf: (id) => window.cancelAnimationFrame(id),
  setTimeout: (cb, ms) => window.setTimeout(cb, ms),
  clearTimeout: (id) => window.clearTimeout(id),
  createCanvas: () => document.createElement("canvas"),
  readSwitch: () => {
    try { return window.localStorage.getItem("spGenieMesh"); } catch { return null; }
  },
  log: (message) => console.info(message),
});

let env: GenieEnv = browserEnv();
let sessionDisabled = false;
let loggedDisable = false;
const initialReadout = (): GenieReadout => ({
  state: "idle", reason: "", mode: "auto", snapshotMs: null, worstSnapshotMs: null, snapshotSize: null, canvasSize: null,
  uploadMs: null, meshRows: MESH_ROWS, meshCols: MESH_COLS, triangles: MESH_ROWS * MESH_COLS * 2, frames: 0,
  worstFrameMs: null, meanFrameMs: null, sessionDisabled: false, animations: 0, contextAlive: false,
});
const readout: GenieReadout = initialReadout();
if (typeof window !== "undefined") (window as unknown as { __spGenie?: GenieReadout }).__spGenie = readout;

export const getReadout = () => readout;

function record(patch: Partial<GenieReadout>) {
  Object.assign(readout, patch, { sessionDisabled, contextAlive: renderer !== null && !renderer.lost() });
}

/** Test seam: replace the clock, timers, canvas factory and kill-switch reader; call with no argument to restore. */
export function setGenieEnv(partial?: Partial<GenieEnv>) {
  env = { ...browserEnv(), ...partial };
}

export function resetGenieEngine() {
  stopActive();
  disposeRenderer();
  sessionDisabled = false;
  loggedDisable = false;
  Object.assign(readout, initialReadout());
  setGenieEnv();
}

export function switchMode(): "auto" | "on" | "off" {
  const raw = env.readSwitch();
  return raw === "off" || raw === "on" ? raw : "auto";
}

/** Why the mesh must not be used right now, or null. Cheap; called before each capture and animation. */
export function blockedReason(): string | null {
  const mode = switchMode();
  record({ mode });
  if (mode === "off") return "kill-switch";
  if (mode !== "on" && sessionDisabled) return "disabled";
  return null;
}

export function disableForSession(reason: string) {
  sessionDisabled = true;
  if (!loggedDisable) {
    loggedDisable = true;
    env.log(`[StagePilot] Genie mesh animation disabled for this session (${reason}); using the plain panel animation.`);
  }
  record({ state: "disabled", reason });
}

/** Called after every capture. Over budget on a normal session -> the mesh is off for good. */
export function reportSnapshot(snapshot: Pick<PanelSnapshot, "ms" | "width" | "height">): boolean {
  record({
    snapshotMs: snapshot.ms,
    worstSnapshotMs: Math.max(readout.worstSnapshotMs ?? 0, snapshot.ms),
    snapshotSize: `${snapshot.width}x${snapshot.height}`,
  });
  if (snapshot.ms > SNAPSHOT_BUDGET_MS && switchMode() !== "on") {
    disableForSession(`snapshot took ${Math.round(snapshot.ms)} ms (budget ${SNAPSHOT_BUDGET_MS} ms)`);
    return false;
  }
  return true;
}

export function reportFallback(reason: string) {
  if (readout.state !== "disabled") record({ state: "fallback", reason });
}

/** Counts consecutive slow frames in the first half of an animation. Pure; injected frame times in tests. */
export class BailMonitor {
  private slow = 0;
  push(frameMs: number, fraction: number): boolean {
    if (fraction > 0.5) { this.slow = 0; return false; }
    this.slow = frameMs > FRAME_BUDGET_MS ? this.slow + 1 : 0;
    return this.slow >= BAIL_FRAMES;
  }
}

// ---------------------------------------------------------------------------------------------------- WebGL

const VERTEX = `attribute vec2 a_uv;uniform vec4 u_src;uniform vec3 u_tgt;uniform float u_p;uniform vec2 u_res;varying vec2 v_uv;
float sm(float x){return x*x*(3.-2.*x);}
void main(){float u=a_uv.x,v=a_uv.y;float q=clamp(u_p*(1.+${GENIE_K.toFixed(2)})-v*${GENIE_K.toFixed(2)},0.,1.);float qe=sm(q);
float y=mix(u_src.y+v*u_src.w,u_tgt.y,qe);float xc=u_src.x+u_src.z*.5;
float cx=mix(xc,u_tgt.x,pow(qe,1.05));float w=mix(u_src.z,u_tgt.z,pow(qe,.45));
v_uv=vec2(u,v);gl_Position=vec4((cx+(u-.5)*w)/u_res.x*2.-1.,1.-y/u_res.y*2.,0.,1.);}`;
const FRAGMENT = "precision mediump float;uniform sampler2D u_tex;varying vec2 v_uv;void main(){gl_FragColor=texture2D(u_tex,v_uv);}";

interface Renderer {
  canvas: HTMLCanvasElement;
  gl: WebGLRenderingContext;
  mesh: MeshGeometry;
  texture: WebGLTexture | null;
  textureSource: HTMLCanvasElement | null;
  uniforms: Record<"src" | "tgt" | "p" | "res", WebGLUniformLocation | null>;
  lost: () => boolean;
}

let renderer: Renderer | null = null;
let releaseTimer: number | null = null;

function compile(gl: WebGLRenderingContext, type: number, source: string) {
  const shader = gl.createShader(type);
  if (!shader) throw new Error("createShader");
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(`shader: ${gl.getShaderInfoLog(shader)}`);
  return shader;
}

/** Context creation is lazy (first animated use) and shared. Returns null when WebGL is unavailable. */
function acquireRenderer(): Renderer | null {
  if (releaseTimer !== null) { env.clearTimeout(releaseTimer); releaseTimer = null; }
  if (renderer && !renderer.lost()) return renderer;
  disposeRenderer();
  const canvas = env.createCanvas();
  const gl = (canvas.getContext("webgl", {
    alpha: true, premultipliedAlpha: true, antialias: false, depth: false, stencil: false,
    powerPreference: "low-power", preserveDrawingBuffer: false,
  }) ?? null) as WebGLRenderingContext | null;
  if (!gl) return null;
  const program = gl.createProgram();
  if (!program) return null;
  gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, VERTEX));
  gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, FRAGMENT));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error("program link");
  gl.useProgram(program);
  const mesh = buildMesh(MESH_ROWS, MESH_COLS);
  const vertices = gl.createBuffer();
  const indices = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, vertices);
  gl.bufferData(gl.ARRAY_BUFFER, mesh.uv, gl.STATIC_DRAW);
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indices);
  gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, mesh.indices, gl.STATIC_DRAW);
  const attribute = gl.getAttribLocation(program, "a_uv");
  gl.enableVertexAttribArray(attribute);
  gl.vertexAttribPointer(attribute, 2, gl.FLOAT, false, 0, 0);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
  gl.clearColor(0, 0, 0, 0);
  const created: Renderer = {
    canvas, gl, mesh, texture: null, textureSource: null,
    uniforms: {
      src: gl.getUniformLocation(program, "u_src"), tgt: gl.getUniformLocation(program, "u_tgt"),
      p: gl.getUniformLocation(program, "u_p"), res: gl.getUniformLocation(program, "u_res"),
    },
    lost: () => gl.isContextLost(),
  };
  canvas.addEventListener("webglcontextlost", (event) => { event.preventDefault(); record({ contextAlive: false }); });
  renderer = created;
  return created;
}

function uploadTexture(r: Renderer, snapshot: PanelSnapshot) {
  if (r.textureSource === snapshot.canvas && r.texture) return;
  const gl = r.gl;
  const max = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number;
  if (snapshot.width > max || snapshot.height > max) throw new Error("texture larger than MAX_TEXTURE_SIZE");
  if (r.texture) gl.deleteTexture(r.texture);
  const texture = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, snapshot.canvas);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  if (gl.getError() !== gl.NO_ERROR) throw new Error("texture upload GL error");
  r.texture = texture;
  r.textureSource = snapshot.canvas;
}

function disposeRenderer() {
  if (releaseTimer !== null) { env.clearTimeout(releaseTimer); releaseTimer = null; }
  const r = renderer;
  renderer = null;
  if (!r) return;
  try {
    if (r.texture) r.gl.deleteTexture(r.texture);
    r.gl.getExtension("WEBGL_lose_context")?.loseContext();
  } catch { /* releasing is best effort */ }
  r.canvas.remove();
  record({ contextAlive: false });
}

function scheduleRelease() {
  if (releaseTimer !== null) env.clearTimeout(releaseTimer);
  releaseTimer = env.setTimeout(() => { releaseTimer = null; if (!active) disposeRenderer(); }, RELEASE_AFTER_MS);
}

/** Test/diagnostic helpers. */
export const rendererAlive = () => renderer !== null;
export const releasePending = () => releaseTimer !== null;

// ------------------------------------------------------------------------------------------------ animation

export interface GenieStart {
  host: HTMLElement;
  snapshot: PanelSnapshot;
  /** Card the panel pours into: x/y/width in the host's (panel's) coordinate space; y = card bottom edge, <= 0. */
  card: CardTarget;
  opening: boolean;
  /** Where the animation starts (0 open .. 1 collapsed); a reversal continues from the current progress. */
  fromProgress: number;
  durationMs: number;
  /** Called every frame with the height the dashboard below must have (px). */
  onLayout: (bottomPx: number) => void;
  onEnd: (result: "done" | "bailed") => void;
}

export interface GenieRun {
  /** Stops the animation and returns the progress it had reached. */
  cancel: () => number;
}

let active: { stop: () => number } | null = null;
export const isAnimating = () => active !== null;

function stopActive() {
  active?.stop();
  active = null;
}

/**
 * Starts a mesh animation, or returns null (the caller keeps the CSS reveal). The first frame is drawn synchronously,
 * so call it before the browser paints the new phase.
 */
export function startGenie(start: GenieStart): GenieRun | null {
  const blocked = blockedReason();
  if (blocked) { if (blocked === "disabled") record({ state: "disabled", reason: readout.reason }); else reportFallback(blocked); return null; }
  stopActive();
  let r: Renderer | null;
  try {
    r = acquireRenderer();
  } catch (error) {
    disposeRenderer();
    reportFallback(`webgl-setup: ${String(error)}`);
    return null;
  }
  if (!r) { reportFallback("no-webgl"); return null; }
  const { snapshot, card, host } = start;
  const panel = { width: snapshot.width, height: snapshot.height };
  const pad = Math.ceil(Math.max(0, -card.y)) + 2;
  const uploadStart = env.now();
  try {
    uploadTexture(r, snapshot);
  } catch (error) {
    disposeRenderer();
    reportFallback(`texture-upload: ${String(error)}`);
    return null;
  }
  const uploadMs = env.now() - uploadStart;
  const canvas = r.canvas;
  canvas.width = panel.width;
  canvas.height = panel.height + pad;
  Object.assign(canvas.style, {
    position: "absolute", left: "0", top: `${-pad}px`, width: `${panel.width}px`, height: `${panel.height + pad}px`,
    pointerEvents: "none", display: "block",
  });
  canvas.setAttribute("aria-hidden", "true");
  canvas.tabIndex = -1;
  host.appendChild(canvas);

  const gl = r.gl;
  const to = start.opening ? 0 : 1;
  const duration = Math.max(1, start.durationMs);
  const bail = new BailMonitor();
  const forced = switchMode() === "on";
  let raf = 0;
  let t0 = -1;
  let last = -1;
  let progress = start.fromProgress;
  let frames = 0;
  let worst = 0;
  let total = 0;
  let finished = false;

  const draw = (p: number) => {
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform4f(r.uniforms.src, 0, pad, panel.width, panel.height);
    gl.uniform3f(r.uniforms.tgt, card.cx, pad + card.y, card.width);
    gl.uniform1f(r.uniforms.p, p);
    gl.uniform2f(r.uniforms.res, canvas.width, canvas.height);
    gl.drawElements(gl.TRIANGLES, r.mesh.indices.length, gl.UNSIGNED_SHORT, 0);
    start.onLayout(layoutBottom(p, panel, card));
  };

  const finish = (result: "done" | "bailed", reason = "") => {
    if (finished) return;
    finished = true;
    env.caf(raf);
    canvas.remove();
    active = null;
    scheduleRelease();
    record({
      state: result === "done" ? "ok" : "bailed", reason, frames, worstFrameMs: frames ? worst : null,
      meanFrameMs: frames ? total / frames : null, animations: readout.animations + 1,
    });
    start.onEnd(result);
  };

  const step = (time: number) => {
    if (finished) return;
    if (t0 < 0) { t0 = time; last = time; }
    const dt = time - last;
    last = time;
    const fraction = Math.min(1, (time - t0) / duration);
    if (r.lost()) { finish("bailed", "context-lost"); return; }
    if (frames > 0) {
      worst = Math.max(worst, dt);
      total += dt;
      if (!forced && bail.push(dt, fraction)) { frames += 1; finish("bailed", `slow frames (${Math.round(dt)} ms)`); return; }
    }
    frames += 1;
    progress = genieProgress(fraction, start.fromProgress, to);
    try { draw(progress); } catch (error) { finish("bailed", `draw: ${String(error)}`); return; }
    if (fraction >= 1) finish("done");
    else raf = env.raf(step);
  };

  try {
    draw(start.fromProgress);
  } catch (error) {
    canvas.remove();
    reportFallback(`draw: ${String(error)}`);
    scheduleRelease();
    return null;
  }
  record({
    uploadMs, canvasSize: `${canvas.width}x${canvas.height}`, snapshotSize: `${snapshot.width}x${snapshot.height}`,
    state: readout.state === "disabled" ? "ok" : readout.state,
  });
  raf = env.raf(step);
  const stop = () => {
    if (!finished) {
      finished = true;
      env.caf(raf);
      canvas.remove();
      active = null;
      scheduleRelease();
    }
    return progress;
  };
  active = { stop };
  return { cancel: stop };
}
