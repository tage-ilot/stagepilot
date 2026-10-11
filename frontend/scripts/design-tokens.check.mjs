import assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync, spawnSync } from "node:child_process";
import { cpSync, mkdtempSync, mkdirSync, symlinkSync, readFileSync, appendFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";
import resolveConfig from "tailwindcss/resolveConfig.js";
import config from "../tailwind.config.js";
import postcss from "postcss";
import tailwindcss from "tailwindcss";

const frontend = resolve(dirname(fileURLToPath(import.meta.url)), "..");
test("pointer variants generate scoped real hover and coarse target CSS", async () => {
  const result = await postcss([tailwindcss({
    ...config,
    content: [{ raw: 'can-hover:bg-primary coarse:min-h-ds-touch-target', extension: "html" }],
  })]).process("@tailwind utilities;", { from: undefined });
  const rules = [];
  result.root.walkRules((rule) => rules.push(rule));
  const hover = rules.find((rule) => rule.selector === ".can-hover\\:bg-primary:hover");
  const coarse = rules.find((rule) => rule.selector === ".coarse\\:min-h-ds-touch-target");
  assert.ok(hover);
  assert.ok(coarse);
  assert.equal(hover.parent.name, "media");
  assert.equal(hover.parent.params, "(hover: hover)");
  assert.equal(coarse.parent.name, "media");
  assert.equal(coarse.parent.params, "(pointer: coarse)");
  assert.ok(coarse.nodes.some((node) => node.prop === "min-height" && node.value === "44px"));
  assert.equal(config.future?.hoverOnlyWhenSupported, undefined);
});
test("compatibility keeps stock radii, spacing, brand and stage colors", () => {
  const theme = resolveConfig(config).theme;
  assert.equal(theme.borderRadius.DEFAULT, "0.25rem");
  assert.equal(theme.borderRadius.lg, "0.5rem");
  assert.equal(theme.borderRadius["2xl"], "1rem");
  assert.equal(theme.borderRadius["ds-default"], "8px");
  assert.equal(theme.spacing["2xl"], undefined);
  assert.equal(theme.spacing["ds-2xl"], "24px");
  assert.equal(theme.fontFamily.brand, undefined);
  assert.equal(theme.colors.stage[850], "#111923");
  assert.equal(theme.colors.idle, "#64748b");
  assert.equal(theme.colors.ink, "#f8f4ec");
  assert.equal(theme.colors.neutral[500], "#737373");
});
test("font is self-hosted with swap", () => {
  const css = readFileSync(resolve(frontend, "node_modules/@fontsource-variable/inter/index.css"), "utf8");
  assert.match(css, /font-display: swap/);
  assert.match(css, /font-family: 'Inter Variable'/);
  assert.doesNotMatch(css, /https?:/);
});
test("generated output is current; either stale output is rejected", () => {
  const dir = mkdtempSync(resolve(tmpdir(), "sp-tokens-"));
  try {
    const copy = resolve(dir, "frontend");
    mkdirSync(copy);
    for (const name of ["scripts", "tailwind.config.js", "src/design-tokens.css"]) {
      cpSync(resolve(frontend, name), resolve(copy, name), { recursive: true });
    }
    cpSync(resolve(frontend, "../DESIGN.md"), resolve(dir, "DESIGN.md"));
    symlinkSync(resolve(frontend, "node_modules"), resolve(copy, "node_modules"), "dir");
    const script = resolve(copy, "scripts/design-tokens.mjs");
    execFileSync(process.execPath, [script, "--check"]);
    for (const name of ["tailwind.config.js", "src/design-tokens.css"]) {
      appendFileSync(resolve(copy, name), "\n/* stale */\n");
      const result = spawnSync(process.execPath, [script, "--check"], { encoding: "utf8" });
      assert.equal(result.status, 1);
      assert.match(result.stderr, new RegExp(`Stale generated tokens: ${name.replaceAll(".", "\\.")}`));
      cpSync(resolve(frontend, name), resolve(copy, name));
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
