import { beforeAll, describe, expect, it } from "vitest";

let css = "";
beforeAll(async () => {
  const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
  css = readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
});
const rule = (selector: string) => {
  const start = css.indexOf(`${selector} {`);
  expect(start, selector).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start) + 1);
};

describe("config panel notch CSS contract (DESIGN.md \"Config panel reveal\")", () => {
  it("has a zero-sized, non-interactive track that only carries the translateY following the panel's top edge", () => {
    const track = rule(".panel-reveal__notch-track");
    expect(track).toContain("height: 0");
    expect(track).toContain("pointer-events: none");
    expect(track).not.toMatch(/transition|animation|opacity|will-change/);
  });

  it("clips the notch to the strip above the panel's top border with a static, non-interactive clip-path", () => {
    const clip = rule(".panel-reveal__notch-clip");
    expect(clip).toContain("clip-path: inset(0)");
    expect(clip).toContain("pointer-events: none");
    expect(clip).toContain("height: 14px");
    expect(clip).toContain("top: calc(var(--emerge-top, 0px) - 12px)"); // the border's top edge is 12px below the clip's top
    expect(clip).toContain("z-index: 3");
  });

  it("is an upright 45 degree triangle (no rotation) with a 1px accent edge and a fill inset by 1px", () => {
    const notch = rule(".panel-reveal__notch");
    expect(notch).toContain("color: var(--notch-accent, transparent)");
    expect(notch).not.toContain("rotate");
    // the accent is a 1px chevron (outer edge out, inner edge back) on a pseudo-element, so it never sits under the translucent fill
    // (a clip-path on the notch itself would clip the fill too)
    const chevron = rule(".panel-reveal__notch::before");
    expect(chevron).toContain("background: currentColor");
    expect(chevron).toContain("clip-path: polygon(0 14px, 12.4px 1.6px");
    expect(chevron).toContain("26.586px 14px, 14.586px 2px, 13.414px 2px, 1.414px 14px");
    expect(notch).not.toContain("clip-path");
    expect(rule(".panel-reveal__notch-fill")).toContain("clip-path: polygon(1.414px 14px"); // 1px perpendicular = 1.414px along the axis
  });

  it("never transitions or animates anything in CSS, and has no opacity fade (the clip reveals it)", () => {
    for (const selector of [".panel-reveal__notch-clip", ".panel-reveal__notch-rise", ".panel-reveal__notch", ".panel-reveal__notch-fill", ".panel-reveal__notch-bg"]) {
      const block = rule(selector);
      expect(block).not.toMatch(/transition|animation|opacity|will-change/);
    }
    expect(css).not.toMatch(/data-phase="opening"\] \.panel-reveal__notch/);
  });

  it("the fill's counter wrapper cancels the notch's x translation", () => {
    expect(rule(".panel-reveal__notch")).toContain("transform: translateX(var(--notch-x, 0px))");
    expect(rule(".panel-reveal__notch-cx")).toContain("transform: translateX(calc(var(--notch-x, 0px) * -1))");
  });

  it("is dropped in forced-colors", () => {
    expect(css).toContain("@media (forced-colors: active) { .panel-reveal__notch-track { display: none; } }");
  });
});
