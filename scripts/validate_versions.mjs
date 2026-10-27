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
  ["desktop/src-tauri/Cargo.toml", matchVersion("desktop/src-tauri/Cargo.toml", /^version = "([^"]+)"/m)],
  ["desktop/src-tauri/Cargo.lock", matchVersion("desktop/src-tauri/Cargo.lock", /name = "stagepilot-desktop"\r?\nversion = "([^"]+)"/)],
]);

// The Rust backend and the Planning Center user agent take the version from tauri.conf.json at
// build time (crates/stagepilot-core/build.rs). A literal version in the source would report the
// wrong release (rc.4 shipped "2.41.1"), so the injection and the absence of literals is checked.
const buildRs = read("crates/stagepilot-core/build.rs");
if (!buildRs.includes("tauri.conf.json") || !buildRs.includes("STAGEPILOT_VERSION")) {
  throw new Error("crates/stagepilot-core/build.rs no longer injects the version from tauri.conf.json.");
}
if (!/pub const APP_VERSION: &str = env!\("STAGEPILOT_VERSION"\);/.test(read("crates/stagepilot-core/src/settings.rs"))) {
  throw new Error("APP_VERSION in crates/stagepilot-core/src/settings.rs must come from env!(\"STAGEPILOT_VERSION\").");
}
if (/StagePilot\/\d/.test(read("crates/stagepilot-pco/src/models.rs"))) {
  throw new Error("crates/stagepilot-pco/src/models.rs hard-codes a version in the user agent.");
}

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
  throw new Error("The main updater endpoint is missing or changed.");
}
if (!tauri.plugins?.updater?.pubkey || tauri.plugins.updater.pubkey === "STAGEPILOT_UPDATER_PUBLIC_KEY_REQUIRED") {
  throw new Error("A real Tauri updater public key must replace STAGEPILOT_UPDATER_PUBLIC_KEY_REQUIRED before release.");
}
const updaterEndpoint = "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json";
for (const file of ["desktop/src-tauri/tauri.release.conf.json", "desktop/src-tauri/tauri.macos.conf.json"]) {
  const overlay = JSON.parse(read(file));
  if (overlay.plugins?.updater?.endpoints?.[0] !== updaterEndpoint) {
    throw new Error(`The updater endpoint is missing or changed in ${file}.`);
  }
}

const tracked = process.env.STAGEPILOT_TRACKED_FILES?.split("\n").filter(Boolean) ?? [];
if (tracked.some((file) => /\.(key|pem)$/i.test(file))) {
  throw new Error("A private key-like file is tracked by git.");
}
console.log(`StagePilot ${version} version and updater configuration are consistent.`);
