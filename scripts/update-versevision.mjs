#!/usr/bin/env node
// StagePilot carries VerseVision inside its server. Cargo.toml follows the branch `stable` of github.com/huntrw6/versevision, which the
// VerseVision release workflow moves to every final release. Cargo.lock pins the exact commit a build uses; this script re-resolves it
// in both lockfiles (the workspace and desktop/src-tauri), so a StagePilot release is built with the newest VerseVision release without
// anyone editing a version or a commit.
//
//   node scripts/update-versevision.mjs           update both lockfiles to the newest release, print what they now hold
//   node scripts/update-versevision.mjs --check   change nothing; exit 1 when a lockfile is behind the newest release
//
// Pre-release tags (v1.2.0-rc1) never move `stable`, so they are never picked up here.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const REPO = "https://github.com/huntrw6/versevision";
const CRATES = ["versevision-web", "versevision-core", "versevision-bible", "versevision-pp"];
const MANIFESTS = ["Cargo.toml", "desktop/src-tauri/Cargo.toml"];

/** The commit Cargo.lock holds for the VerseVision git source, or null. */
export function lockedCommit(lockText) {
  const match = lockText.match(/name = "versevision-web"\r?\nversion = "[^"]+"\r?\nsource = "git\+[^"#]*huntrw6\/versevision[^"#]*#([0-9a-f]{40})"/);
  return match ? match[1] : null;
}

/** The release tag (v1.2.3) whose commit is `sha`, from `git ls-remote --tags` output; final releases only. */
export function tagFor(lsRemoteTags, sha) {
  for (const line of lsRemoteTags.split(/\r?\n/)) {
    const [commit, ref] = line.split(/\s+/);
    const m = ref && ref.match(/^refs\/tags\/(v\d+\.\d+\.\d+)\^\{\}$/);
    if (m && commit === sha) return m[1];
  }
  return null;
}

function run(cmd, args, cwd) {
  return execFileSync(cmd, args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

function main() {
  const root = path.resolve(import.meta.dirname, "..");
  const check = process.argv.includes("--check");
  const remote = run("git", ["ls-remote", REPO, "refs/heads/stable", "refs/tags/v*"]);
  const stable = remote.split(/\r?\n/).find((l) => l.endsWith("\trefs/heads/stable"))?.split(/\s+/)[0];
  if (!stable) throw new Error(`${REPO} has no branch "stable" yet (the first VerseVision release creates it)`);
  const tag = tagFor(remote, stable);
  let behind = false;
  for (const manifest of MANIFESTS) {
    const lockFile = path.join(root, path.dirname(manifest), "Cargo.lock");
    const before = lockedCommit(fs.readFileSync(lockFile, "utf8"));
    if (!check && before !== stable) {
      run("cargo", ["update", "--manifest-path", path.join(root, manifest), ...CRATES.flatMap((c) => ["-p", c])], root);
    }
    const after = lockedCommit(fs.readFileSync(lockFile, "utf8"));
    const state = after === stable ? "current" : "BEHIND";
    if (after !== stable) behind = true;
    console.log(`${path.join(path.dirname(manifest), "Cargo.lock").replace(/^\.\//, "")}: ${after?.slice(0, 8)} ${state}${before !== after ? ` (was ${before?.slice(0, 8)})` : ""}`);
  }
  console.log(`VerseVision newest release: ${tag ?? "(stable is not at a release tag)"} ${stable.slice(0, 8)}`);
  if (!tag) throw new Error("`stable` does not point at a release tag; refusing to build on an unreleased VerseVision");
  if (check && behind) process.exit(1);
}

if (import.meta.url === `file://${process.argv[1]}`) main();
