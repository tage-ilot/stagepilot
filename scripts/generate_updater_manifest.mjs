import fs from "node:fs";
import path from "node:path";

const [assetsDirectory, tag, downloadBase = "https://github.com/tage-ilot/stagepilot/releases/download"] = process.argv.slice(2);
if (!assetsDirectory || !tag) throw new Error("Usage: node scripts/generate_updater_manifest.mjs ASSETS_DIR vVERSION [DOWNLOAD_BASE_URL]");
const version = tag.replace(/^v/, "");
if (`v${version}` !== tag) throw new Error(`Invalid release tag: ${tag}`);
const base = new URL(downloadBase);
if (base.protocol !== "https:" || base.search || base.hash) throw new Error("Download base must be an HTTPS URL without query or fragment.");
const normalizedBase = base.href.replace(/\/$/, "");

const platforms = {
  "darwin-aarch64": `StagePilot_${version}_aarch64.app.tar.gz`,
  "darwin-x86_64": `StagePilot_${version}_x64.app.tar.gz`,
  "windows-x86_64": `StagePilot_${version}_Windows-x64-setup.exe`,
};
const manifestPlatforms = {};
for (const [platform, filename] of Object.entries(platforms)) {
  const artifact = path.join(assetsDirectory, filename);
  const signaturePath = `${artifact}.sig`;
  if (!fs.existsSync(artifact)) throw new Error(`Missing updater artifact: ${filename}`);
  if (!fs.existsSync(signaturePath)) throw new Error(`Missing updater signature: ${filename}.sig`);
  const signature = fs.readFileSync(signaturePath, "utf8").trim();
  if (!signature) throw new Error(`Empty updater signature: ${filename}.sig`);
  manifestPlatforms[platform] = {
    signature,
    url: `${normalizedBase}/${tag}/${filename}`,
  };
}
const notesPath = path.join(assetsDirectory, "release-notes.md");
const notes = fs.existsSync(notesPath) ? fs.readFileSync(notesPath, "utf8").trim() : `StagePilot ${version}`;
fs.writeFileSync(path.join(assetsDirectory, "latest.json"), `${JSON.stringify({
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: manifestPlatforms,
}, null, 2)}\n`);
console.log(`Generated latest.json for ${Object.keys(manifestPlatforms).join(", ")}.`);
