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
