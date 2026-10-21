import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const forbidden = /(^|\/)(?:\.tools|\.deploy-worktree|\.guardrails-worktree|\.milestone-[^/]+|\.release-[^/]+|node_modules|target|dist)(?:\/|$)|(?:^|\/)(?:\.env(?!\.example)(?:\..*)?|journal-[^/]+\.json)$|\.(?:db|sqlite|sqlite3|key|pem|p12|pfx|mobileprovision)$/i;

function sha256(file) {
  const hash = crypto.createHash("sha256");
  hash.update(fs.readFileSync(file));
  return hash.digest("hex");
}

function auditSource() {
  const tracked = execFileSync("git", ["ls-files", "-z"], { cwd: root })
    .toString("utf8").split("\0").filter(Boolean);
  const leaks = tracked.filter((file) => forbidden.test(file));
  if (leaks.length) throw new Error(`Release-export leak(s): ${leaks.join(", ")}`);
  console.log(JSON.stringify({ mode: "source", trackedFiles: tracked.length, leaks: [] }));
}

function auditAssets(directory, version) {
  // Stable X.Y.Z, or a pre-release X.Y.Z-(alpha|beta|rc).N (a Rust release candidate). The asset
  // inventory is identical for both; publish_release.mjs keeps pre-releases away from "Latest".
  if (!/^\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?$/.test(version)) {
    throw new Error("A stable X.Y.Z or pre-release X.Y.Z-(alpha|beta|rc).N version is required.");
  }
  const required = [
    `StagePilot_${version}_aarch64.dmg`,
    `StagePilot_${version}_x64.dmg`,
    `StagePilot_${version}_aarch64.app.tar.gz`,
    `StagePilot_${version}_aarch64.app.tar.gz.sig`,
    `StagePilot_${version}_x64.app.tar.gz`,
    `StagePilot_${version}_x64.app.tar.gz.sig`,
    `StagePilot_${version}_x64-setup.exe`,
    `StagePilot_${version}_x64-setup.exe.sig`,
    "latest.json",
    "release-notes.md",
  ];
  const actual = fs.readdirSync(directory).filter((name) => fs.statSync(path.join(directory, name)).isFile()).sort();
  const missing = required.filter((name) => !actual.includes(name));
  const unexpected = actual.filter((name) => !required.includes(name));
  if (missing.length || unexpected.length) {
    throw new Error(`Asset inventory mismatch; missing=${missing.join(",") || "none"}; unexpected=${unexpected.join(",") || "none"}`);
  }
  const assets = actual.map((name) => {
    const file = path.join(directory, name);
    const size = fs.statSync(file).size;
    if (size < 1 || size > 512 * 1024 * 1024) throw new Error(`Invalid asset size: ${name}`);
    return { name, size, sha256: sha256(file) };
  });
  console.log(JSON.stringify({ mode: "assets", version, assets }, null, 2));
}

const [command, first, second] = process.argv.slice(2);
if (command === "source") auditSource();
else if (command === "assets" && first && second) auditAssets(first, second);
else throw new Error("Usage: node scripts/audit_release.mjs source | assets DIRECTORY VERSION");
