// @vitest-environment node
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "vite";
import { expect, it } from "vitest";

it("production build contains no data-src, overlay or editor endpoint", async () => {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), "dev-editor-build-"));
  await build({ root, logLevel: "silent", build: { outDir, emptyOutDir: true, write: true } });
  const files: string[] = [];
  const walk = (d: string) => fs.readdirSync(d, { withFileTypes: true }).forEach((e) => (e.isDirectory() ? walk(path.join(d, e.name)) : files.push(path.join(d, e.name))));
  walk(outDir);
  expect(files.some((f) => f.endsWith(".js"))).toBe(true);
  for (const f of files.filter((f) => /\.(js|css|html|map)$/.test(f))) {
    const text = fs.readFileSync(f, "utf8");
    for (const needle of ["data-src", "__dev-editor", "dev-editor", "x-dev-editor-token", "stagepilot-visual-editor"]) {
      expect(text.includes(needle), `${needle} in ${path.basename(f)}`).toBe(false);
    }
  }
  fs.rmSync(outDir, { recursive: true, force: true });
}, 120_000);

// The section gallery (sections.html, src/gallery/, `npm run build:gallery`) is a review tool: it must not be in the web bundle the
// backend serves, and not in the desktop app, which packs the same `frontend/dist` (desktop/src-tauri/tauri.conf.json: frontendDist).
it("production build and the desktop bundle contain no section gallery", async () => {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), "section-gallery-build-"));
  await build({ root, logLevel: "silent", build: { outDir, emptyOutDir: true, write: true } });
  const files: string[] = [];
  const walk = (d: string) => fs.readdirSync(d, { withFileTypes: true }).forEach((e) => (e.isDirectory() ? walk(path.join(d, e.name)) : files.push(path.join(d, e.name))));
  walk(outDir);
  expect(files.some((f) => f.endsWith(".js"))).toBe(true);
  expect(files.filter((f) => /sections\.html$|sections-[\w-]+\.(js|css)$/.test(f))).toEqual([]);
  for (const f of files.filter((f) => /\.(js|css|html|map)$/.test(f))) {
    const text = fs.readFileSync(f, "utf8");
    for (const needle of ["section-gallery", "data-gallery-frame", "StagePilot section gallery", "The section gallery has no backend", "made-up/install.sh"]) {
      expect(text.includes(needle), `${needle} in ${path.basename(f)}`).toBe(false);
    }
  }
  fs.rmSync(outDir, { recursive: true, force: true });
  // The desktop app packs exactly this folder, and builds it with the same `npm run build` (not `build:gallery`).
  const tauri = JSON.parse(fs.readFileSync(path.resolve(root, "../desktop/src-tauri/tauri.conf.json"), "utf8")) as { build: { frontendDist: string; beforeBuildCommand: string } };
  expect(tauri.build.frontendDist).toBe("../../frontend/dist");
  expect(tauri.build.beforeBuildCommand).toBe("npm --prefix ../frontend run build");
  const scripts = (JSON.parse(fs.readFileSync(path.resolve(root, "package.json"), "utf8")) as { scripts: Record<string, string> }).scripts;
  expect(scripts.build).not.toMatch(/gallery/);
  // The gallery has its own output folder and entry, and the app's config names only index.html.
  expect(scripts["build:gallery"]).toContain("vite.gallery.config.ts");
  const galleryConfig = fs.readFileSync(path.resolve(root, "vite.gallery.config.ts"), "utf8");
  expect(galleryConfig).toContain('outDir: "dist-gallery"');
  expect(fs.readFileSync(path.resolve(root, "vite.config.ts"), "utf8")).not.toMatch(/sections\.html|gallery/);
}, 120_000);
