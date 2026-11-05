// Repeatable close-animation check for the config panels (DESIGN.md "Config panel reveal"). NOT run in CI.
//
//   node scripts/panel-close-frames.mjs [--base http://127.0.0.1:5173/] [--out ./close-frames] [--throttle 4]
//   env: PLAYWRIGHT=/path/to/playwright-core/index.mjs  CHROME=/path/to/chrome
//
// 1. Scrubbed frames: for the first, middle and last card and the tall Lighting panel, opens the panel, starts a close, pauses every
//    finite animation and sets currentTime to 60/75/85/92/97/100 % of the close. Prints the strip's height / width / x range and the
//    panel group's opacity (the outer wrapper's computed opacity), and writes a screenshot per frame to --out.
// 2. Real playback: a close at 60 Hz (rAF sampler, CDP CPU throttle --throttle) counting the frames in which a strip of 1-30 px height
//    is visible (opacity > 0.1), and whether anything is left in the DOM or visible after the panel unmounts.
import fs from "node:fs";
import path from "node:path";

const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) args.set(process.argv[i].replace(/^--/, ""), process.argv[i + 1]);
const BASE = args.get("base") ?? "http://127.0.0.1:5173/";
const OUT = args.get("out") ?? "";
const THROTTLE = Number(args.get("throttle") ?? 4);
const { chromium } = await import(process.env.PLAYWRIGHT ?? "playwright-core");
const CASES = [["first (Services)", "planning-center-configuration"], ["middle (Presentation)", "propresenter-configuration"], ["last (Backend)", "backend-configuration"], ["tall (Lighting)", "lighting-configuration"]];
const POINTS = [0.6, 0.75, 0.85, 0.92, 0.97, 1];

const browser = await chromium.launch({ executablePath: process.env.CHROME || undefined, args: ["--no-sandbox"] });
async function fresh(slow = false) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  const page = await context.newPage();
  // Slow-motion dev hook (DESIGN.md): the scrubbed part runs 8x slower so the unmount timer cannot fire while frames are being captured.
  if (slow) await page.addInitScript(() => localStorage.setItem("spGenieMs", "8"));
  await page.goto(BASE); await page.waitForTimeout(2500);
  const pin = page.locator("input[type=password]");
  if (await pin.count()) { await pin.first().fill(process.env.PIN ?? "1234"); await pin.first().press("Enter"); await page.waitForTimeout(4000); }
  return { context, page };
}
const finiteDone = (page) => page.waitForFunction(() => document.getAnimations().every((a) => !Number.isFinite(a.effect?.getComputedTiming().endTime) || a.playState !== "running"), null, { timeout: 8000 });
const state = () => {
  const panel = document.querySelector(".panel-reveal__panel")?.firstElementChild;
  const outer = document.querySelector(".panel-reveal__x");
  if (!panel || !outer) return null;
  const rect = panel.getBoundingClientRect();
  const notch = document.querySelector(".panel-reveal__notch-rise");
  const matrix = notch ? new DOMMatrix(getComputedStyle(notch).transform) : null;
  return { h: +rect.height.toFixed(2), w: +rect.width.toFixed(1), x0: +rect.left.toFixed(1), x1: +rect.right.toFixed(1), opacity: +Number(getComputedStyle(outer).opacity).toFixed(3), notchSunk: matrix ? +matrix.m42.toFixed(1) : null };
};

console.log("== scrubbed close frames (strip = the panel element's box under the card)");
console.log("card".padEnd(24) + "at".padEnd(6) + "height".padEnd(10) + "opacity".padEnd(9) + "x-range".padEnd(18) + "notch sunk(px of 14)");
const summary = [];
for (const [label, id] of CASES) {
  const { context, page } = await fresh(true);
  await page.click(`[aria-controls='${id}']`); await page.mouse.move(2, 2); await page.waitForTimeout(4600); await finiteDone(page);
  await page.click(`[aria-controls='${id}']`); await page.mouse.move(2, 2);
  await page.evaluate(() => document.getAnimations().forEach((a) => { if (Number.isFinite(a.effect?.getComputedTiming().endTime)) a.pause(); }));
  const duration = await page.evaluate(() => Math.max(...document.getAnimations().map((a) => a.effect?.getComputedTiming().endTime ?? 0).filter(Number.isFinite)));
  const card = await page.evaluate((cid) => { const r = document.querySelector(`[aria-controls='${cid}']`).getBoundingClientRect(); return { x0: r.left, x1: r.right, bottom: r.bottom }; }, id);
  for (const p of POINTS) {
    await page.evaluate(([at]) => document.getAnimations().forEach((a) => { if (Number.isFinite(a.effect?.getComputedTiming().endTime)) a.currentTime = at; }), [p * duration]);
    await page.waitForTimeout(60);
    const s = await page.evaluate(state);
    if (!s) { console.log(label.padEnd(24) + `${Math.round(p * 100)}%`.padEnd(6) + "(unmounted)"); summary.push({ label, p, gone: true }); continue; }
    console.log(label.padEnd(24) + `${Math.round(p * 100)}%`.padEnd(6) + String(s.h).padEnd(10) + String(s.opacity).padEnd(9) + `${s.x0}-${s.x1}`.padEnd(18) + s.notchSunk);
    summary.push({ label, p, ...s });
    if (OUT) { fs.mkdirSync(OUT, { recursive: true }); await page.screenshot({ path: path.join(OUT, `close-${label.split(" ")[0]}-${Math.round(p * 100)}.png`), clip: { x: 0, y: Math.max(0, card.bottom - 100), width: 1440, height: 260 } }); }
  }
  await context.close();
}
// the 92 % criterion: nothing taller than ~3 px or more opaque than ~10 %
const bad = summary.filter((s) => s.p === 0.92 && !s.gone && s.h > 3 && s.opacity > 0.1);
console.log(bad.length ? `92 % check: ${bad.length} case(s) still tall AND opaque` : "92 % check: every case is either <= 3 px tall or <= 10 % opaque");
const end = summary.filter((s) => s.p === 1 && !s.gone && s.h > 0.5 && s.opacity > 0.01);
console.log(end.length ? "100 % check: something is left" : "100 % check: nothing left (0 px tall or fully transparent)");

console.log(`\n== real playback, 60 Hz sampler, CPU throttle ${THROTTLE}x`);
for (const [label, id] of CASES) {
  const { context, page } = await fresh();
  const cdp = await context.newCDPSession(page);
  await page.click(`[aria-controls='${id}']`); await page.mouse.move(2, 2); await finiteDone(page); await page.waitForTimeout(300);
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: THROTTLE });
  await page.evaluate(() => {
    window.__samples = [];
    const read = () => {
      const panel = document.querySelector(".panel-reveal__panel")?.firstElementChild; const outer = document.querySelector(".panel-reveal__x");
      if (!panel || !outer) return { gone: true };
      return { h: panel.getBoundingClientRect().height, o: Number(getComputedStyle(outer).opacity) };
    };
    const tick = (t) => {
      if (!window.__finish) { const y = document.getAnimations().find((a) => a.effect?.target?.classList?.contains("panel-reveal__y")); if (y && !y.__watched) { y.__watched = true; y.finished.then(() => { window.__finish = performance.now(); }).catch(() => {}); } }
      window.__samples.push({ t, ...read() }); if (window.__samples.length < 400) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await page.click(`[aria-controls='${id}']`); await page.mouse.move(2, 2);
  await page.waitForTimeout(1600);
  const samples = await page.evaluate(() => window.__samples);
  const finishedAt = await page.evaluate(() => window.__finish ?? null);
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: 1 });
  const first = samples.findIndex((s) => !s.gone && s.h !== undefined);
  const during = samples.slice(first).filter((s, i, all) => !s.gone || i < all.findIndex((x) => x.gone));
  const visibleStrip = during.filter((s) => !s.gone && s.h >= 1 && s.h <= 30 && s.o > 0.1).length;
  const unmountAt = samples.find((s) => s.gone);
  const lag = finishedAt !== null && unmountAt ? unmountAt.t - finishedAt : null; // finish event -> first frame without the panel
  const last = [...during].reverse().find((s) => !s.gone);
  console.log(`${label.padEnd(24)} frames sampled during close: ${during.length}, frames with a 1-30 px strip visible (opacity > 0.1): ${visibleStrip}, last frame before unmount: h ${last ? last.h.toFixed(1) : "-"} px opacity ${last ? last.o.toFixed(2) : "-"}, unmounted: ${unmountAt ? "yes" : "NO"}, finish -> first frame without the panel: ${lag === null ? "n/a" : lag.toFixed(1) + " ms"}`);
  await context.close();
}
await browser.close();
