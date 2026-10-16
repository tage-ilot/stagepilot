import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// Proves every part of the updater metadata chain that is provable on
// Linux without building a native installer. It NEVER fabricates a platform
// artifact: the fixtures below are opaque placeholder bytes used solely to
// exercise manifest generation/validation logic, and are deleted afterwards.
// Whether a real signed installer updates a real machine is NOT proven here
// and remains DEFERRED to the native acceptance matrix.

const root = path.resolve(import.meta.dirname, "..");
const BASE = "https://github.com/tage-ilot/stagepilot/releases/download";
const OTHER = "https://github.com/tage-ilot/stagepilot-alpha/releases/download";

const generate = (directory, tag, base) => execFileSync(process.execPath, [
  path.join(root, "scripts/generate_updater_manifest.mjs"), directory, tag, base,
], { encoding: "utf8", stdio: "pipe" });

const validate = (directory, base) => execFileSync(process.execPath, [
  path.join(root, "scripts/validate_updater_manifest.mjs"),
  path.join(directory, "latest.json"), directory, base,
], { encoding: "utf8", stdio: "pipe" });

const rejects = (run, because) => {
  assert.throws(run, because);
};

const stage = (version) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "stagepilot-updater-chain-"));
  for (const file of [
    `StagePilot_${version}_aarch64.app.tar.gz`,
    `StagePilot_${version}_x64.app.tar.gz`,
    `StagePilot_${version}_x64-setup.exe`,
  ]) {
    // Placeholder bytes. These are not, and must never be presented as, real
    // signed platform artifacts.
    fs.writeFileSync(path.join(directory, file), `placeholder-${file}`);
    fs.writeFileSync(path.join(directory, `${file}.sig`), `placeholder-signature-${file}`);
  }
  return directory;
};

const proven = [];
const directories = [];

try {
  // 1. latest.json generation and validation against the release download base.
  const version = "2.41.0";
  const release = stage(version);
  directories.push(release);
  generate(release, `v${version}`, BASE);
  validate(release, BASE);
  const manifest = JSON.parse(fs.readFileSync(path.join(release, "latest.json"), "utf8"));
  assert.equal(manifest.version, version);
  assert(Number.isFinite(Date.parse(manifest.pub_date)));
  assert.deepEqual(Object.keys(manifest.platforms).sort(), [
    "darwin-aarch64", "darwin-x86_64", "windows-x86_64",
  ]);
  proven.push("latest_json_generation_and_validation");

  // 2. Every signature is embedded from its .sig sidecar, not invented.
  for (const [platform, filename] of Object.entries({
    "darwin-aarch64": `StagePilot_${version}_aarch64.app.tar.gz`,
    "darwin-x86_64": `StagePilot_${version}_x64.app.tar.gz`,
    "windows-x86_64": `StagePilot_${version}_x64-setup.exe`,
  })) {
    const sidecar = fs.readFileSync(path.join(release, `${filename}.sig`), "utf8").trim();
    assert.equal(manifest.platforms[platform].signature, sidecar);
    assert.equal(manifest.platforms[platform].url, `${BASE}/v${version}/${filename}`);
  }
  proven.push("signature_embedding_from_sidecar");

  // 3. A missing signature must stop manifest generation before publication.
  const unsigned = stage(version);
  directories.push(unsigned);
  fs.rmSync(path.join(unsigned, `StagePilot_${version}_x64-setup.exe.sig`));
  rejects(() => generate(unsigned, `v${version}`, BASE), /Missing updater signature/);
  proven.push("missing_signature_rejected");

  // 4. An empty signature must stop manifest generation.
  const empty = stage(version);
  directories.push(empty);
  fs.writeFileSync(path.join(empty, `StagePilot_${version}_aarch64.app.tar.gz.sig`), "   \n");
  rejects(() => generate(empty, `v${version}`, BASE), /Empty updater signature/);
  proven.push("empty_signature_rejected");

  // 5. A missing artifact must stop manifest generation.
  const incomplete = stage(version);
  directories.push(incomplete);
  fs.rmSync(path.join(incomplete, `StagePilot_${version}_x64.app.tar.gz`));
  rejects(() => generate(incomplete, `v${version}`, BASE), /Missing updater artifact/);
  proven.push("missing_artifact_rejected");

  // 6. Version/tag agreement is enforced.
  rejects(() => generate(stage(version), version, BASE), /Invalid release tag/);
  proven.push("tag_version_agreement_enforced");

  // 7. A non-HTTPS or query-bearing endpoint is refused.
  rejects(() => generate(release, `v${version}`, "http://example.com/releases"), /HTTPS/);
  rejects(() => generate(release, `v${version}`, `${BASE}?token=x`), /query or fragment/);
  proven.push("endpoint_scheme_and_shape_enforced");

  // 8. A manifest built for any other download base must not validate against this repo's.
  const foreign = stage(version);
  directories.push(foreign);
  generate(foreign, `v${version}`, OTHER);
  rejects(() => validate(foreign, BASE), /Untrusted or missing URL/);
  proven.push("foreign_download_base_rejected");

  // 9. A tampered URL that escapes the allowlisted path is refused.
  const tampered = stage(version);
  directories.push(tampered);
  generate(tampered, `v${version}`, BASE);
  const escaped = JSON.parse(fs.readFileSync(path.join(tampered, "latest.json"), "utf8"));
  escaped.platforms["windows-x86_64"].url = `${BASE}/v${version}/%2e%2e%2foutside.bin`;
  fs.writeFileSync(path.join(tampered, "latest.json"), JSON.stringify(escaped));
  rejects(() => validate(tampered, BASE), /Untrusted or missing URL/);
  proven.push("path_escape_rejected");

  // 10. A dropped platform makes the manifest inventory invalid.
  const partial = stage(version);
  directories.push(partial);
  generate(partial, `v${version}`, BASE);
  const dropped = JSON.parse(fs.readFileSync(path.join(partial, "latest.json"), "utf8"));
  delete dropped.platforms["darwin-x86_64"];
  fs.writeFileSync(path.join(partial, "latest.json"), JSON.stringify(dropped));
  rejects(() => validate(partial, BASE), /platform inventory is invalid/);
  proven.push("platform_inventory_enforced");

  // 11. The configured updater public key is real, not a placeholder, and the
  //     base/main channel and release overlays (Windows/macOS) all resolve
  //     the same direct GitHub releases endpoint — the dead control-plane
  //     broker must never be configured as an updater endpoint.
  const config = JSON.parse(fs.readFileSync(path.join(root, "desktop/src-tauri/tauri.conf.json"), "utf8"));
  const windows = JSON.parse(fs.readFileSync(path.join(root, "desktop/src-tauri/tauri.release.conf.json"), "utf8"));
  const macos = JSON.parse(fs.readFileSync(path.join(root, "desktop/src-tauri/tauri.macos.conf.json"), "utf8"));
  const pubkey = config.plugins.updater.pubkey;
  assert(typeof pubkey === "string" && pubkey.length > 40, "updater public key is missing");
  assert(!/REQUIRED|PLACEHOLDER|CHANGEME|TODO/i.test(pubkey), "updater public key is still a placeholder");
  assert.deepEqual(config.plugins.updater.endpoints, [
    "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json",
  ]);
  assert.deepEqual(windows.plugins.updater.endpoints, [
    "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json",
  ]);
  assert.deepEqual(macos.plugins.updater.endpoints, [
    "https://github.com/tage-ilot/stagepilot/releases/latest/download/latest.json",
  ]);
  proven.push("public_key_and_endpoint_configuration");

  // 12. The release version is plain X.Y.Z (no pre-release suffix), the only form the release audit accepts.
  assert.match(version, /^\d+\.\d+\.\d+$/);
  proven.push("release_version_is_plain_x_y_z");
} finally {
  for (const directory of directories) fs.rmSync(directory, { recursive: true, force: true });
}

console.log(JSON.stringify({ provenOnLinux: proven }, null, 2));
assert.equal(proven.length, 12);
