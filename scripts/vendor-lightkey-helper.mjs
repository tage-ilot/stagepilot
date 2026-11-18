#!/usr/bin/env node
// Vendors the Lightkey helper (huntrw6/lightkey-api) into the StagePilot binary.
//
//   node scripts/vendor-lightkey-helper.mjs v0.5.0
//
// Downloads the release assets of the given tag, verifies every file against the release's own
// SHA256SUMS, and writes them plus a VERSION file to
// crates/stagepilot-lights/assets/lightkey-helper/ (embedded with include_bytes!). Run it with
// the TESTED helper release before cutting a StagePilot release (docs/releasing-stagepilot.md).
// Nothing is written unless every file verified.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const REPO = "huntrw6/lightkey-api";
const FILES = ["install.sh", "lightkey_api.py", "lightkey_cues.py", "uninstall.sh"];
const SUMS = "SHA256SUMS";
const root = path.resolve(import.meta.dirname, "..");
const out = path.join(root, "crates/stagepilot-lights/assets/lightkey-helper");

export function parseSums(text) {
  const sums = new Map();
  for (const line of text.split("\n")) {
    const match = /^([0-9a-f]{64}) [ *]?([A-Za-z0-9_.-]+)$/.exec(line.trim());
    if (match) sums.set(match[2], match[1]);
  }
  return sums;
}

export function verifyAll(files, sums) {
  for (const name of FILES) {
    const want = sums.get(name);
    if (!want) throw new Error(`${name} is not listed in ${SUMS}`);
    const got = crypto.createHash("sha256").update(files.get(name)).digest("hex");
    if (got !== want) throw new Error(`${name}: checksum mismatch (have ${got}, ${SUMS} says ${want})`);
  }
}

/**
 * StagePilot sends this helper to Lighting Macs that run an older one, and the helper installs it by itself (docs/lights.md, "Updating
 * it"). That code lives in `lightkey_api.py` and its launcher in `install.sh`. A release of huntrw6/lightkey-api that lacks them would
 * silently take the self-update away from every Mac set up from this build, so vendoring refuses it.
 */
export function assertSelfUpdating(files) {
  const api = files.get("lightkey_api.py")?.toString("utf8") ?? "";
  const installer = files.get("install.sh")?.toString("utf8") ?? "";
  if (!api.includes("# ===== BEGIN helper-update core =====") || !/^UPDATER_LEVEL = [1-9]/m.test(api)) {
    throw new Error("lightkey_api.py of this release cannot update itself (no helper-update core). Port it to the helper repository and tag that release first.");
  }
  if (!installer.includes("lightkey_api_run.py")) throw new Error("install.sh of this release does not install the launcher that puts an old version back. Port it first.");
}

async function download(tag, name) {
  const url = `https://github.com/${REPO}/releases/download/${tag}/${name}`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return Buffer.from(await response.arrayBuffer());
}

async function main() {
  const tag = process.argv[2];
  if (!tag || !/^v\d+\.\d+\.\d+$/.test(tag)) {
    console.error("Usage: node scripts/vendor-lightkey-helper.mjs vX.Y.Z");
    process.exit(2);
  }
  const files = new Map();
  for (const name of [...FILES, SUMS]) files.set(name, await download(tag, name));
  const sums = parseSums(files.get(SUMS).toString("utf8"));
  verifyAll(files, sums);
  assertSelfUpdating(files);
  fs.mkdirSync(out, { recursive: true });
  for (const name of fs.readdirSync(out)) fs.rmSync(path.join(out, name), { force: true });
  for (const [name, bytes] of files) fs.writeFileSync(path.join(out, name), bytes);
  fs.writeFileSync(path.join(out, "VERSION"), `${tag.slice(1)}\n`);
  console.log(`Vendored lightkey-api ${tag} into ${path.relative(root, out)}`);
  for (const name of FILES) console.log(`  ${sums.get(name)}  ${name}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
