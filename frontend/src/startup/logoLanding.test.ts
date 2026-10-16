import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { forgetLoadingLogo, landLogo, rememberLoadingLogo } from "./logoLanding";

function logoAt(left: number, top: number, width: number, height: number): HTMLElement {
  const element = document.createElement("h1");
  const rect = { left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) };
  element.getBoundingClientRect = () => rect as DOMRect;
  document.createRange = () => ({ selectNodeContents: () => undefined, getBoundingClientRect: () => rect }) as unknown as Range;
  return element;
}

describe("loading logo landing", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    forgetLoadingLogo();
    window.matchMedia = ((query: string) => ({ matches: false, media: query })) as typeof window.matchMedia;
    globalThis.DOMMatrixReadOnly = class { m41 = 0; m42 = 0; } as unknown as typeof DOMMatrixReadOnly;
  });
  afterEach(() => { vi.useRealTimers(); });

  it("does nothing when no loading logo was remembered (a page reload, a test)", () => {
    const root = document.createElement("main");
    const header = logoAt(32, 50, 223, 40);
    expect(landLogo(header, root)).toBeUndefined();
    expect(root.dataset.landing).toBeUndefined();
    expect(header.style.translate).toBe("");
  });

  it("starts the header logo at the loading logo's centre and size, glides home, then clears itself", () => {
    const root = document.createElement("main");
    const big = logoAt(400, 330, 620, 100);
    rememberLoadingLogo(big);
    const header = logoAt(32, 44, 223, 36);
    const cleanup = landLogo(header, root);
    expect(cleanup).toBeTypeOf("function");
    expect(root.dataset.landing).toBe("moving");
    // Same centre (710,380 vs 143.5,62) and the size ratio 620/223.
    expect(header.style.translate).toBe(`${710 - 143.5}px ${380 - 62}px`);
    expect(Number(header.style.scale)).toBeCloseTo(620 / 223, 5);
    vi.advanceTimersByTime(16);
    vi.advanceTimersByTime(900);
    expect(header.style.translate).toBe("");
    expect(header.style.scale).toBe("");
    expect(root.dataset.landing).toBe("done");
    vi.advanceTimersByTime(600);
    expect(root.dataset.landing).toBeUndefined();
  });

  it("holds the remembered position for the next mount when unmounted mid-flight (React StrictMode runs effects twice)", () => {
    const root = document.createElement("main");
    rememberLoadingLogo(logoAt(400, 330, 620, 100));
    const header = logoAt(32, 44, 223, 36);
    const cleanup = landLogo(header, root)!;
    cleanup();
    expect(root.dataset.landing).toBeUndefined();
    expect(header.style.translate).toBe("");
    expect(landLogo(header, root)).toBeTypeOf("function");
  });

  it("skips the flight for people who asked for reduced motion", () => {
    window.matchMedia = ((query: string) => ({ matches: true, media: query })) as typeof window.matchMedia;
    const root = document.createElement("main");
    rememberLoadingLogo(logoAt(400, 330, 620, 100));
    expect(landLogo(logoAt(32, 44, 223, 36), root)).toBeUndefined();
    expect(root.dataset.landing).toBeUndefined();
  });

  it("skips the flight when either logo has no size yet", () => {
    const root = document.createElement("main");
    rememberLoadingLogo(logoAt(0, 0, 0, 0));
    expect(landLogo(logoAt(32, 44, 223, 36), root)).toBeUndefined();
  });

  it("hides the rest of the dashboard from the first frame of the flight and fades it in afterwards (stylesheet rules)", async () => {
    const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
    const css = readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
    expect(css).toMatch(/\[data-landing="moving"\] > :not\(header\),[^{]*\{\s*opacity: 0;/);
    // No transition while moving, or the dashboard would flash before it is hidden.
    expect(css).not.toMatch(/\.stagepilot-dashboard\[data-landing\] > :not\(header\)/);
    expect(css).toMatch(/\[data-landing="done"\] > :not\(header\),[^{]*\{\s*animation: landing-fade-in 400ms/);
  });
});
