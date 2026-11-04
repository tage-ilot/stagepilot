/** DOM -> texture source for the genie mesh. Always 1x. Loaded lazily; see DESIGN.md "Config panel reveal". */

export interface PanelSnapshot {
  canvas: HTMLCanvasElement;
  width: number;
  height: number;
  /** Cost of the capture itself (excludes the one-time library import and font embedding). */
  ms: number;
  takenAt: number;
}

export class SnapshotError extends Error {
  constructor(public reason: string, cause?: unknown) {
    super(reason);
    this.cause = cause;
  }
}

/** True when an RGBA sample is all transparent or one single colour (a failed or empty snapshot). */
export function isBlankSample(data: ArrayLike<number>): boolean {
  if (data.length < 4) return true;
  const at = (index: number) => data[index] ?? 0;
  let opaque = false;
  let varied = false;
  for (let i = 0; i < data.length; i += 4) {
    if (at(i + 3) > 0) opaque = true;
    if (
      Math.abs(at(i) - at(0)) > 2 || Math.abs(at(i + 1) - at(1)) > 2 ||
      Math.abs(at(i + 2) - at(2)) > 2 || Math.abs(at(i + 3) - at(3)) > 2
    ) varied = true;
  }
  return !opaque || !varied;
}

/** Reads a 24x24 downscale of the canvas back and rejects blank results. Throws SnapshotError("tainted" | "blank"). */
export function verifyNotBlank(canvas: HTMLCanvasElement, doc: Document = document): void {
  const probe = doc.createElement("canvas");
  probe.width = 24;
  probe.height = 24;
  const ctx = probe.getContext("2d");
  if (!ctx) throw new SnapshotError("no-2d-context");
  let data: Uint8ClampedArray;
  try {
    ctx.drawImage(canvas, 0, 0, 24, 24);
    data = ctx.getImageData(0, 0, 24, 24).data;
  } catch (error) {
    throw new SnapshotError("tainted", error);
  }
  if (isBlankSample(data)) throw new SnapshotError("blank");
}

export type Renderer = (node: HTMLElement) => Promise<HTMLCanvasElement>;

let fontCss: Promise<string> | null = null;

/** html-to-image is imported on first use only, so the main bundle does not carry it. */
async function loadLibrary() {
  return import("html-to-image");
}

/** One-time work (library import, font embedding) kept out of the measured capture. Safe to call repeatedly. */
export async function warmSnapshot(node: HTMLElement): Promise<void> {
  const lib = await loadLibrary();
  fontCss ??= lib.getFontEmbedCSS(node).catch(() => "");
  await fontCss;
}

const defaultRenderer: Renderer = async (node) => {
  const lib = await loadLibrary();
  fontCss ??= lib.getFontEmbedCSS(node).catch(() => "");
  return lib.toCanvas(node, { pixelRatio: 1, fontEmbedCSS: await fontCss, cacheBust: false, skipAutoScale: true });
};

/** Captures `node` at 1x. Throws SnapshotError("render" | "tainted" | "blank" | ...). */
export async function capturePanel(
  node: HTMLElement,
  options: { render?: Renderer; now?: () => number } = {},
): Promise<PanelSnapshot> {
  const render = options.render ?? defaultRenderer;
  const now = options.now ?? (() => performance.now());
  const start = now();
  let canvas: HTMLCanvasElement;
  try {
    canvas = await render(node);
  } catch (error) {
    throw new SnapshotError("render", error);
  }
  const ms = now() - start;
  if (!canvas.width || !canvas.height) throw new SnapshotError("blank");
  verifyNotBlank(canvas, node.ownerDocument);
  return { canvas, width: canvas.width, height: canvas.height, ms, takenAt: now() };
}
