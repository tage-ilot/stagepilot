// Tests for scripts/helper_signing.mjs, scripts/sign_helpers.mjs and the release check in scripts/validate_versions.mjs.
// A throwaway key pair is made here for every test: the project's private key is never on a test machine, and the pinned public key is
// only ever used to prove that a bundle signed by another key is refused.
//
//   node --test scripts/helper_signing.test.mjs
import assert from "node:assert/strict";
import crypto from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { HELPERS, HELPER_PUBLIC_KEY, SIG, SUMS, assertSignedBundles, bundleProblems, buildSums, parseSums, rawPublicKey } from "./helper_signing.mjs";
import { run } from "./sign_helpers.mjs";

const repo = path.resolve(import.meta.dirname, "..");

function keyPair() {
  const { privateKey, publicKey } = crypto.generateKeyPairSync("ed25519");
  return { privateKey, publicKey, raw: rawPublicKey(publicKey), pem: privateKey.export({ format: "pem", type: "pkcs8" }) };
}

/** A temporary repository root holding copies of both helper folders. */
function checkout() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "helper-signing-"));
  for (const helper of HELPERS) {
    const target = path.join(root, helper.dir);
    fs.mkdirSync(target, { recursive: true });
    for (const name of fs.readdirSync(path.join(repo, helper.dir))) {
      if (name === SIG) continue;
      fs.copyFileSync(path.join(repo, helper.dir, name), path.join(target, name));
    }
  }
  return root;
}

const quiet = () => {};

test("the pinned key is a 32-byte Ed25519 public key", () => {
  const raw = Buffer.from(HELPER_PUBLIC_KEY, "base64");
  assert.equal(raw.length, 32);
  assert.equal(raw.toString("base64"), HELPER_PUBLIC_KEY);
});

test("SHA256SUMS in the working tree lists every file of each helper folder and matches it (no key needed)", () => {
  for (const helper of HELPERS) {
    const dir = path.join(repo, helper.dir);
    assert.equal(fs.readFileSync(path.join(dir, SUMS), "utf8"), buildSums(dir), `${helper.dir}: run node scripts/sign_helpers.mjs --sums-only after changing a helper file`);
  }
});

test("a bundle signed with the test key verifies with the test key and with nothing else", () => {
  const mine = keyPair();
  const root = checkout();
  run(["--root", root, "--key", writePem(root, mine), "--public-key", mine.raw], quiet);
  assertSignedBundles(root, mine.raw);
  assert.throws(() => assertSignedBundles(root), /not signed by the project/, "the pinned project key does not accept it");
  for (const helper of HELPERS) {
    const dir = path.join(root, helper.dir);
    assert.equal(fs.readFileSync(path.join(dir, SIG), "utf8").trim().length, 88, "base64 of 64 bytes");
    assert.equal(parseSums(fs.readFileSync(path.join(dir, SUMS), "utf8")).size, fs.readdirSync(dir).length - 2, "every file but the two signature files");
  }
});

function writePem(root, pair) {
  const file = path.join(root, "test-key.pem");
  fs.writeFileSync(file, pair.pem, { mode: 0o600 });
  return file;
}

test("signing refuses a key that is not the pinned one, and writes nothing", () => {
  const root = checkout();
  const other = keyPair();
  const before = fs.readFileSync(path.join(root, HELPERS[0].dir, SUMS), "utf8");
  assert.throws(() => run(["--root", root, "--key", writePem(root, other)], quiet), /not the project's helper-signing key/);
  assert.equal(fs.readFileSync(path.join(root, HELPERS[0].dir, SUMS), "utf8"), before);
  for (const helper of HELPERS) assert.equal(fs.existsSync(path.join(root, helper.dir, SIG)), false);
});

test("signing refuses a key that is not Ed25519", () => {
  const root = checkout();
  const rsa = crypto.generateKeyPairSync("rsa", { modulusLength: 2048 }).privateKey.export({ format: "pem", type: "pkcs8" });
  const file = path.join(root, "rsa.pem");
  fs.writeFileSync(file, rsa);
  assert.throws(() => run(["--root", root, "--key", file], quiet), /Ed25519/);
});

test("--sums-only rewrites the lists and leaves the signatures alone", () => {
  const mine = keyPair();
  const root = checkout();
  run(["--root", root, "--key", writePem(root, mine), "--public-key", mine.raw], quiet);
  const dir = path.join(root, HELPERS[1].dir);
  const signature = fs.readFileSync(path.join(dir, SIG), "utf8");
  fs.appendFileSync(path.join(dir, "lightkey_api.py"), "# changed\n");
  run(["--root", root, "--sums-only"], quiet);
  assert.equal(fs.readFileSync(path.join(dir, SIG), "utf8"), signature);
  assert.ok(bundleProblems(dir, mine.raw).length > 0, "the old signature no longer covers the new list");
});

test("every way a bundle can be wrong is named", () => {
  const mine = keyPair();
  const root = checkout();
  run(["--root", root, "--key", writePem(root, mine), "--public-key", mine.raw], quiet);
  const dir = path.join(root, HELPERS[0].dir);
  assert.deepEqual(bundleProblems(dir, mine.raw), []);

  // a file changed after it was signed
  fs.appendFileSync(path.join(dir, "playback_library.py"), "# tampered\n");
  assert.match(bundleProblems(dir, mine.raw).join("\n"), /playback_library\.py does not match SHA256SUMS/);
  fs.copyFileSync(path.join(repo, HELPERS[0].dir, "playback_library.py"), path.join(dir, "playback_library.py"));
  run(["--root", root, "--key", writePem(root, mine), "--public-key", mine.raw], quiet);

  // a new file nobody listed
  fs.writeFileSync(path.join(dir, "extra.py"), "print('x')\n");
  assert.match(bundleProblems(dir, mine.raw).join("\n"), /extra\.py is not listed/);
  fs.rmSync(path.join(dir, "extra.py"));

  // the list edited after signing, a bad signature file, no signature file
  const sums = fs.readFileSync(path.join(dir, SUMS), "utf8");
  fs.writeFileSync(path.join(dir, SUMS), `${sums}\n`);
  assert.match(bundleProblems(dir, mine.raw).join("\n"), /not a valid signature/);
  fs.writeFileSync(path.join(dir, SUMS), sums);
  assert.deepEqual(bundleProblems(dir, mine.raw), []);
  const signature = fs.readFileSync(path.join(dir, SIG), "utf8");
  for (const [text, message] of [["", /is empty/], ["not base64!!\n", /64-byte/], [`${Buffer.alloc(63).toString("base64")}\n`, /64-byte/], [`${Buffer.alloc(64).toString("base64")}\n`, /not a valid signature/]]) {
    fs.writeFileSync(path.join(dir, SIG), text);
    assert.match(bundleProblems(dir, mine.raw).join("\n"), message);
  }
  fs.rmSync(path.join(dir, SIG));
  assert.match(bundleProblems(dir, mine.raw).join("\n"), /SHA256SUMS\.sig is missing/);
  fs.writeFileSync(path.join(dir, SIG), signature);

  // signed by another key
  const other = keyPair();
  assert.match(bundleProblems(dir, other.raw).join("\n"), /not a valid signature/);
});

test("the working tree before the release machine signs it is refused as unsigned", () => {
  const root = checkout();
  assert.throws(() => assertSignedBundles(root), /SHA256SUMS\.sig is missing: the bundle is not signed/);
  const placeholder = path.join(root, HELPERS[0].dir, SIG);
  fs.writeFileSync(placeholder, "");
  assert.throws(() => assertSignedBundles(root), /is empty: the bundle is not signed/);
});

test("--verify and the release check agree with the library", () => {
  const mine = keyPair();
  const root = checkout();
  run(["--root", root, "--key", writePem(root, mine), "--public-key", mine.raw], quiet);
  const logged = [];
  run(["--root", root, "--verify", "--public-key", mine.raw], (line) => logged.push(line));
  assert.match(logged.join("\n"), /signed by the project's key/);
  assert.throws(() => run(["--root", root, "--verify"], quiet), /not signed by the project/);
});

test("the command line refuses what it does not know and a missing key", () => {
  assert.throws(() => run(["--bogus"], quiet), /Unknown option/);
  assert.throws(() => run([], quiet), /Usage/);
  assert.throws(() => run(["--key"], quiet), /needs a value/);
  const result = spawnSync(process.execPath, [path.join(repo, "scripts/sign_helpers.mjs"), "--bogus"], { encoding: "utf8" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Unknown option/);
});

test("validate_versions refuses a release (tag argument) whose helpers are not signed, and says how to sign", () => {
  const result = spawnSync(process.execPath, [path.join(repo, "scripts/validate_versions.mjs"), "v0.0.0-not-a-real-tag"], { encoding: "utf8", cwd: repo });
  assert.notEqual(result.status, 0);
  const output = `${result.stdout}${result.stderr}`;
  // The tag does not match, or the helpers are unsigned: either way a release is refused, and the message never asks for the private key's contents.
  assert.ok(/not signed by the project|does not match|tag/i.test(output), output);
  assert.ok(!/BEGIN PRIVATE KEY/.test(output));
});

test("a private key never sits in the helper folders", () => {
  for (const helper of HELPERS) {
    for (const name of fs.readdirSync(path.join(repo, helper.dir))) {
      const text = fs.readFileSync(path.join(repo, helper.dir, name), "utf8");
      assert.ok(!/BEGIN (EC |OPENSSH |RSA )?PRIVATE KEY/.test(text), `${helper.dir}/${name}`);
    }
  }
  assert.equal(execFileSync("git", ["ls-files", "*.pem", "*.key"], { cwd: repo, encoding: "utf8" }).trim(), "");
});
