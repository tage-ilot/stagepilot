// Repeatable pixel test for the config-panel notch (DESIGN.md "Config panel reveal"). NOT run in CI.
//
// For all five panels it opens the panel, then compares the pixels inside the notch (above the panel's top border) with the
// panel's own pixels at the same x, 13-16 css px below the border, and prints a table of the largest per-channel difference
// (0..255) after: first open, a swap from another card, close + re-open, a window resize, and the last frame of an open
// (animations paused at their end, i.e. the moment the notch reaches rest).
//
//   node scripts/panel-notch-pixels.mjs [--base http://127.0.0.1:5192/] [--out ./notch-frames] [--dsf 2]
//   env: PLAYWRIGHT=/path/to/playwright-core/index.mjs  CHROME=/path/to/chrome  (defaults: `playwright-core`, Playwright's own browser)
//
// Samples (per scenario): the notch centre and 4 positions across the triangle, at 4 and 10 css px above the border, each a
// block 1 css px wide x 2 css px tall (device pixels averaged), plus the "spec" block (6 x 3 css px, 3-6 px above, notch centre).
// The panel reference is the same size block, same x, 13-16 css px below the border. --out also writes 6x zoomed crops.
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) args.set(process.argv[i].replace(/^--/, ""), process.argv[i + 1]);
const BASE = args.get("base") ?? process.env.BASE ?? "http://127.0.0.1:5192/";
const OUT = args.get("out") ?? process.env.OUT ?? "";
const DSF = Number(args.get("dsf") ?? 2);
const { chromium } = await import(process.env.PLAYWRIGHT ?? "playwright-core");

const PANELS = [
  ["Services", "planning-center-configuration"],
  ["Playback", "midi-configuration"],
  ["Presentation", "propresenter-configuration"],
  ["Lighting", "lighting-configuration"],
  ["Backend", "backend-configuration"],
];
const HOVER = args.get("hover") === "1"; // leave the pointer on the card after the click (the default moves it away)
const WIDTH = Number(args.get("width") ?? 1440);
const ONLY = (args.get("only") ?? "").split(",").filter(Boolean);
const SCEN = (args.get("scenarios") ?? "first,swap,reopen,resize,rise-end").split(",");
const OTHER = (id) => (id === "backend-configuration" ? "planning-center-configuration" : "backend-configuration");

/** Minimal PNG decoder (8-bit RGB/RGBA, not interlaced), enough for Playwright screenshots. */
function decodePng(buffer) {
  let at = 8; let width = 0; let height = 0; let channels = 0; const idat = [];
  while (at < buffer.length) {
    const length = buffer.readUInt32BE(at); const type = buffer.toString("ascii", at + 4, at + 8); const data = buffer.subarray(at + 8, at + 8 + length);
    if (type === "IHDR") { width = data.readUInt32BE(0); height = data.readUInt32BE(4); channels = data[9] === 6 ? 4 : 3; }
    if (type === "IDAT") idat.push(data);
    at += 12 + length;
  }
  const raw = zlib.inflateSync(Buffer.concat(idat)); const stride = width * channels; const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)]; const row = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x += 1) {
      const a = x >= channels ? pixels[y * stride + x - channels] : 0; const b = y ? pixels[(y - 1) * stride + x] : 0; const c = x >= channels && y ? pixels[(y - 1) * stride + x - channels] : 0;
      let value = row[x];
      if (filter === 1) value += a; else if (filter === 2) value += b; else if (filter === 3) value += (a + b) >> 1;
      else if (filter === 4) { const p = a + b - c; const pa = Math.abs(p - a); const pb = Math.abs(p - b); const pc = Math.abs(p - c); value += pa <= pb && pa <= pc ? a : pb <= pc ? b : c; }
      pixels[y * stride + x] = value & 255;
    }
  }
  return { width, height, channels, pixels };
}

/** Mean RGB of a css-px block [x0,x1) x [y0,y1) given in css px relative to the image's css origin. */
function mean(image, x0, x1, y0, y1) {
  const sum = [0, 0, 0]; let n = 0;
  for (let y = Math.round(y0 * DSF); y < Math.round(y1 * DSF); y += 1) for (let x = Math.round(x0 * DSF); x < Math.round(x1 * DSF); x += 1) {
    const i = (y * image.width + x) * image.channels; sum[0] += image.pixels[i]; sum[1] += image.pixels[i + 1]; sum[2] += image.pixels[i + 2]; n += 1;
  }
  return sum.map((v) => v / n);
}

const geometry = (page) => page.evaluate(() => {
  const root = document.querySelector(".panel-reveal"); const panel = document.querySelector(".panel-reveal__panel")?.firstElementChild;
  if (!root || !panel) return null;
  const notch = document.querySelector(".panel-reveal__notch");
  const x = Number.parseFloat(notch.style.getPropertyValue("--notch-x")) + 14;
  const rect = panel.getBoundingClientRect(); const host = root.getBoundingClientRect();
  return { centre: host.left + x, top: rect.top + window.scrollY, left: rect.left, right: rect.right, scrolled: window.scrollY, busy: 0 };
});

async function settle(page) { if (!HOVER) await page.mouse.move(2, 2); await page.waitForFunction(() => document.getAnimations().every((a) => !Number.isFinite(a.effect?.getComputedTiming().endTime) || a.playState !== "running"), null, { timeout: 8000 }); await page.waitForTimeout(250); }
async function open(page, id) { await page.click(`[aria-controls='${id}']`); await settle(page); }

async function measure(page, label, name) {
  const g = await geometry(page);
  if (!g) return { error: "no panel" };
  const half = 24;
  const clip = { x: Math.max(0, Math.floor(g.centre) - half), y: Math.max(0, Math.floor(g.top) - 16), width: half * 2, height: 36 };
  const shot = await page.screenshot({ clip });
  const image = decodePng(shot);
  const originX = clip.x; const originY = clip.y;
  const cx = g.centre - originX; const border = g.top - originY;
  const worst = { diff: 0, where: "", notch: [], panel: [] };
  const rows = [];
  const sample = (h, dx, w, tall, tag) => {
    const x0 = cx + dx - w / 2; const x1 = cx + dx + w / 2;
    const notch = mean(image, x0, x1, border - h - tall / 2, border - h + tall / 2);
    const panel = mean(image, x0, x1, border + 13, border + 13 + (tall === 3 ? 3 : tall));
    const d = Math.max(...notch.map((v, i) => Math.abs(v - panel[i])));
    rows.push({ tag, h, dx: +dx.toFixed(1), diff: +d.toFixed(1) });
    if (d > worst.diff) Object.assign(worst, { diff: d, where: `${tag} h=${h} dx=${dx.toFixed(1)}`, notch: notch.map((v) => +v.toFixed(1)), panel: panel.map((v) => +v.toFixed(1)) });
  };
  sample(4.5, 0, 6, 3, "spec");
  // The triangle's fill ends in its apex ~10 css px above the border, so "10 px above" would only hit the 1px edge: the high row is 9 px above (1 css px tall).
  for (const [h, tall] of [[4, 2], [9, 1]]) {
    const interior = Math.max(0.2, 10 - h - 0.5); // half width of the triangle's fill at this height (45 degree sides, 1px edge)
    for (const f of [0, -0.7, -0.35, 0.35, 0.7]) sample(h, interior * f, 1, tall, "tri");
  }
  if (OUT && label !== "") {
    fs.mkdirSync(OUT, { recursive: true });
    const crop = await page.screenshot({ clip: { x: clip.x + 8, y: clip.y, width: 32, height: 36 }, scale: "device" });
    fs.writeFileSync(path.join(OUT, `${name}-${label}.png`), crop);
  }
  return { diff: +worst.diff.toFixed(1), where: worst.where, notch: worst.notch, panel: worst.panel, rows };
}

const browser = await chromium.launch({ executablePath: process.env.CHROME || undefined, args: ["--no-sandbox"] });
const results = [];
async function fresh(width = WIDTH) {
  const context = await browser.newContext({ viewport: { width, height: 900 }, deviceScaleFactor: DSF });
  const page = await context.newPage();
  await page.goto(BASE); await page.waitForTimeout(2500);
  // A fresh demo backend asks for the dashboard PIN (docs/configuration.md: initial PIN 1234).
  const pin = page.locator("input[type=password]");
  if (await pin.count()) { await pin.first().fill(process.env.PIN ?? "1234"); await pin.first().press("Enter"); await page.waitForTimeout(4000); }
  return { context, page };
}
const SCENARIOS = SCEN;
for (const [name, id] of PANELS) {
  if (ONLY.length && !ONLY.includes(name)) continue;
  const row = { panel: name };
  for (const scenario of SCENARIOS) {
    const { context, page } = await fresh();
    try {
      if (scenario === "first") await open(page, id);
      else if (scenario === "swap") { await open(page, OTHER(id)); await open(page, id); await page.waitForTimeout(500); }
      else if (scenario === "reopen") { await open(page, id); await page.click(`[aria-controls='${id}']`); await page.waitForTimeout(900); await open(page, id); }
      else if (scenario === "resize") { await open(page, id); await page.setViewportSize({ width: 1290, height: 820 }); await page.waitForTimeout(700); await settle(page); }
      else {
        await page.click(`[aria-controls='${id}']`);
        await page.waitForTimeout(30);
        await page.evaluate(() => document.getAnimations().forEach((animation) => { const t = animation.effect?.getComputedTiming().endTime; if (!Number.isFinite(t)) return; animation.pause(); animation.currentTime = t; }));
        await page.waitForTimeout(150);
      }
      row[scenario] = await measure(page, scenario, name);
    } catch (error) { row[scenario] = { error: String(error).slice(0, 120) }; }
    await context.close();
  }
  results.push(row);
}
await browser.close();

const pad = (s, n) => String(s).padEnd(n);
console.log(`max per-channel difference (0..255), notch vs panel, DSF ${DSF}, ${BASE}`);
console.log(pad("panel", 14) + SCENARIOS.map((s) => pad(s, 10)).join("") + "worst");
let overall = 0;
for (const row of results) {
  let line = pad(row.panel, 14); let worst = "";
  let top = 0;
  for (const scenario of SCENARIOS) {
    const r = row[scenario];
    line += pad(r?.error ? "ERR" : r.diff, 10);
    if (r && !r.error && r.diff >= top) { top = r.diff; worst = `${scenario}: ${r.where} notch ${r.notch} panel ${r.panel}`; }
  }
  overall = Math.max(overall, top);
  console.log(line + worst);
}
console.log(`overall max ${overall} ${overall <= 2 ? "(within 2/255)" : "(over 2/255)"}`);
if (OUT) fs.mkdirSync(OUT, { recursive: true });
if (OUT) fs.writeFileSync(path.join(OUT, "notch-pixels.json"), JSON.stringify(results, null, 1));
