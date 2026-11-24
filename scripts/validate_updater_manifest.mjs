import fs from "node:fs";
import path from "node:path";

const [manifestPath, assetsDirectory, downloadBase = "https://github.com/tage-ilot/stagepilot/releases/download"] = process.argv.slice(2);
if (!manifestPath || !assetsDirectory) throw new Error("Usage: node scripts/validate_updater_manifest.mjs MANIFEST ASSETS_DIR [DOWNLOAD_BASE_URL]");
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
if (!/^\d+\.\d+\.\d+(?:[-+].+)?$/.test(manifest.version)) throw new Error("Invalid manifest version.");
if (!Date.parse(manifest.pub_date)) throw new Error("Invalid manifest publication date.");
const base = new URL(downloadBase);
if (base.protocol !== "https:" || base.search || base.hash) throw new Error("Download base must be an HTTPS URL without query or fragment.");
const filenames = {
  "darwin-aarch64": `StagePilot_${manifest.version}_aarch64.app.tar.gz`,
  "darwin-x86_64": `StagePilot_${manifest.version}_x64.app.tar.gz`,
  "windows-x86_64": `StagePilot_${manifest.version}_Windows-x64-setup.exe`,
};
if (Object.keys(manifest.platforms ?? {}).sort().join(",") !== Object.keys(filenames).sort().join(",")) {
  throw new Error("Manifest platform inventory is invalid.");
}
for (const [platform, filename] of Object.entries(filenames)) {
  const entry = manifest.platforms?.[platform];
  const expectedUrl = `${base.href.replace(/\/$/, "")}/v${manifest.version}/${filename}`;
  if (entry?.url !== expectedUrl) throw new Error(`Untrusted or missing URL for ${platform}.`);
  if (typeof entry.signature !== "string" || !entry.signature.trim()) throw new Error(`Missing signature for ${platform}.`);
  if (!fs.existsSync(path.join(assetsDirectory, filename))) throw new Error(`Manifest references missing asset ${filename}.`);
  if (!fs.existsSync(path.join(assetsDirectory, `${filename}.sig`))) throw new Error(`Manifest references unsigned asset ${filename}.`);
}
console.log("Updater manifest schema, trusted URLs, artifacts, and signatures are valid.");
