import fs from "node:fs";
import path from "node:path";

const [manifestPath, assetsDirectory] = process.argv.slice(2);
if (!manifestPath || !assetsDirectory) throw new Error("Usage: node scripts/validate_updater_manifest.mjs MANIFEST ASSETS_DIR");
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
if (!/^\d+\.\d+\.\d+(?:[-+].+)?$/.test(manifest.version)) throw new Error("Invalid manifest version.");
if (!Date.parse(manifest.pub_date)) throw new Error("Invalid manifest publication date.");
for (const platform of ["darwin-aarch64", "darwin-x86_64", "windows-x86_64"]) {
  const entry = manifest.platforms?.[platform];
  if (!entry?.url?.startsWith("https://github.com/tage-ilot/stagepilot/releases/download/")) throw new Error(`Untrusted or missing URL for ${platform}.`);
  if (typeof entry.signature !== "string" || !entry.signature.trim()) throw new Error(`Missing signature for ${platform}.`);
  const filename = decodeURIComponent(new URL(entry.url).pathname.split("/").at(-1));
  if (!fs.existsSync(path.join(assetsDirectory, filename))) throw new Error(`Manifest references missing asset ${filename}.`);
  if (!fs.existsSync(path.join(assetsDirectory, `${filename}.sig`))) throw new Error(`Manifest references unsigned asset ${filename}.`);
}
console.log("Updater manifest schema, trusted URLs, artifacts, and signatures are valid.");
