import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");
const jsonVersion = (file) => JSON.parse(read(file)).version;
const matchVersion = (file, pattern) => {
  const match = pattern.exec(read(file));
  if (!match) throw new Error(`Could not read a version from ${file}.`);
  return match[1];
};

const versions = new Map([
  ["desktop/src-tauri/tauri.conf.json", jsonVersion("desktop/src-tauri/tauri.conf.json")],
  ["desktop/package.json", jsonVersion("desktop/package.json")],
  ["frontend/package.json", jsonVersion("frontend/package.json")],
  ["backend/pyproject.toml", matchVersion("backend/pyproject.toml", /^version = "([^"]+)"/m)],
  ["desktop/src-tauri/Cargo.toml", matchVersion("desktop/src-tauri/Cargo.toml", /^version = "([^"]+)"/m)],
  ["backend/src/stagepilot/__init__.py", matchVersion("backend/src/stagepilot/__init__.py", /__version__ = "([^"]+)"/)],
  ["backend runtime settings", matchVersion("backend/src/stagepilot/core/config.py", /^\s+version: str = "([^"]+)"/m)],
]);

const unique = new Set(versions.values());
if (unique.size !== 1) {
  for (const [source, version] of versions) console.error(`${source}: ${version}`);
  throw new Error("StagePilot version values do not match.");
}
const version = [...unique][0];
const expectedTag = process.argv[2] ?? process.env.GITHUB_REF_NAME;
if (expectedTag && expectedTag !== `v${version}`) {
  throw new Error(`Release tag ${expectedTag} does not match application version ${version}.`);
}

const tauri = JSON.parse(read("desktop/src-tauri/tauri.conf.json"));
if (tauri.productName !== "StagePilot") throw new Error("The stable product name changed.");
if (tauri.identifier !== "org.stagepilot.desktop") throw new Error("The stable bundle identifier changed.");
if (tauri.app?.windows?.[0]?.label !== "main") throw new Error("The stable main window label changed.");
if (tauri.bundle?.createUpdaterArtifacts !== true) throw new Error("Updater artifacts are not enabled.");
if (tauri.plugins?.updater?.endpoints?.[0] !== "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json") {
  throw new Error("The trusted updater endpoint is missing or changed.");
}
if (!tauri.plugins?.updater?.pubkey || tauri.plugins.updater.pubkey === "STAGEPILOT_UPDATER_PUBLIC_KEY_REQUIRED") {
  throw new Error("A real Tauri updater public key must replace STAGEPILOT_UPDATER_PUBLIC_KEY_REQUIRED before release.");
}

const tracked = process.env.STAGEPILOT_TRACKED_FILES?.split("\n").filter(Boolean) ?? [];
if (tracked.some((file) => /\.(key|pem)$/i.test(file))) {
  throw new Error("A private key-like file is tracked by git.");
}
console.log(`StagePilot ${version} version and updater configuration are consistent.`);
