import { afterEach, describe, expect, it, vi } from "vitest";

import { SnapshotError, capturePanel, isBlankSample, verifyNotBlank } from "./panelSnapshot";

const rgba = (...pixels: number[][]) => Uint8ClampedArray.from(pixels.flat());

describe("isBlankSample", () => {
  it("is blank when every pixel is transparent", () => {
    expect(isBlankSample(new Uint8ClampedArray(24 * 24 * 4))).toBe(true);
  });
  it("is blank when every pixel is the same single colour", () => {
    expect(isBlankSample(rgba([20, 30, 40, 255], [20, 30, 40, 255], [21, 29, 40, 255]))).toBe(true);
  });
  it("is not blank when colours differ", () => {
    expect(isBlankSample(rgba([20, 30, 40, 255], [200, 30, 40, 255]))).toBe(false);
  });
  it("is not blank for a panel on a transparent margin (some opaque, some not)", () => {
    expect(isBlankSample(rgba([0, 0, 0, 0], [20, 30, 40, 255]))).toBe(false);
  });
  it("an empty sample is blank", () => {
    expect(isBlankSample([])).toBe(true);
  });
});

function fakeProbe(data: Uint8ClampedArray | Error) {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation((() => ({
    drawImage: () => { if (data instanceof Error) throw data; },
    getImageData: () => ({ data }),
  })) as never);
}

describe("capturePanel / verifyNotBlank", () => {
  afterEach(() => vi.restoreAllMocks());

  const render = () => Promise.resolve(Object.assign(document.createElement("canvas"), { width: 200, height: 100 }));
  const node = () => document.createElement("div");

  it("returns the canvas with its measured cost", async () => {
    fakeProbe(rgba([1, 2, 3, 255], [200, 2, 3, 255]));
    let clock = 100;
    const snapshot = await capturePanel(node(), { render: () => { clock += 17; return render(); }, now: () => clock });
    expect(snapshot).toMatchObject({ width: 200, height: 100, ms: 17 });
  });

  it("rejects a blank snapshot", async () => {
    fakeProbe(new Uint8ClampedArray(24 * 24 * 4));
    await expect(capturePanel(node(), { render })).rejects.toMatchObject({ reason: "blank" });
  });

  it("rejects a tainted canvas (the read-back throws)", async () => {
    fakeProbe(new Error("SecurityError"));
    await expect(capturePanel(node(), { render })).rejects.toMatchObject({ reason: "tainted" });
  });

  it("wraps a renderer failure and rejects a zero-sized result", async () => {
    await expect(capturePanel(node(), { render: () => Promise.reject(new Error("foreignObject failed")) })).rejects.toBeInstanceOf(SnapshotError);
    await expect(capturePanel(node(), { render: () => Promise.resolve(Object.assign(document.createElement("canvas"), { width: 0, height: 0 })) })).rejects.toMatchObject({ reason: "blank" });
  });

  it("verifyNotBlank needs a 2D context", () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    expect(() => verifyNotBlank(document.createElement("canvas"))).toThrow(SnapshotError);
  });
});
