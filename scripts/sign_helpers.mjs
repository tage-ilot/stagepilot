#!/usr/bin/env node
// Signs the helper bundles that ship inside StagePilot. Run it on the RELEASE MACHINE, after the helpers' files are final and
// before the release commit:
//
//   node scripts/sign_helpers.mjs --key ~/.config/stagepilot-signing/helper-signing.pem
//
// For each helper folder it
//   1. rewrites SHA256SUMS: `<sha256>  <name>` for every file in the folder, sorted by name (so the version constant or VERSION file
//      is covered), and
//   2. writes SHA256SUMS.sig next to it: the base64 of the 64-byte Ed25519 signature of the exact bytes of SHA256SUMS.
// Commit the six changed files:
//   crates/stagepilot-playback/assets/playback-helper/{SHA256SUMS,SHA256SUMS.sig}
//   crates/stagepilot-lights/assets/lightkey-helper/{SHA256SUMS,SHA256SUMS.sig}
// Nothing is written unless the key is an Ed25519 private key whose public half is the project's pinned key (HELPER_PUBLIC_KEY in
// scripts/helper_signing.mjs, in StagePilot and in both helpers). The private key is only read, never copied, printed or kept.
//
//   node scripts/sign_helpers.mjs --verify      checks the committed bundles with the pinned key (no private key needed)
//   node scripts/sign_helpers.mjs --sums-only   for a contributor who changed a helper file: rewrites SHA256SUMS only. No key, no
//                                               signature: the release machine signs it later. (A test fails while it is out of date.)
//
// `--root DIR` works on another checkout; `--public-key BASE64` replaces the pinned key (the tests use a throwaway key pair).
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import { HELPER_PUBLIC_KEY, HELPERS, SUMS, assertSignedBundles, buildSums, rawPublicKey, signBundle } from "./helper_signing.mjs";

export function parseArgs(argv) {
  const options = { key: null, root: path.resolve(import.meta.dirname, ".."), publicKey: HELPER_PUBLIC_KEY, verify: false, sumsOnly: false };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const value = () => {
      if (i + 1 >= argv.length) throw new Error(`${flag} needs a value.`);
      i += 1;
      return argv[i];
    };
    if (flag === "--key") options.key = value();
    else if (flag === "--root") options.root = path.resolve(value());
    else if (flag === "--public-key") options.publicKey = value();
    else if (flag === "--verify") options.verify = true;
    else if (flag === "--sums-only") options.sumsOnly = true;
    else throw new Error(`Unknown option ${flag}. Usage: node scripts/sign_helpers.mjs --key <helper-signing.pem> | --verify | --sums-only`);
  }
  return options;
}

export function run(argv, log = console.log) {
  const options = parseArgs(argv);
  if (options.verify) {
    assertSignedBundles(options.root, options.publicKey);
    log("Both helper bundles are signed by the project's key.");
    return;
  }
  if (options.sumsOnly) {
    for (const helper of HELPERS) {
      const dir = path.join(options.root, helper.dir);
      fs.writeFileSync(path.join(dir, SUMS), buildSums(dir));
      log(`${helper.name}: rewrote ${SUMS} in ${helper.dir} (the release machine signs it)`);
    }
    return;
  }
  if (!options.key) throw new Error("Usage: node scripts/sign_helpers.mjs --key <helper-signing.pem>");
  const privateKey = crypto.createPrivateKey(fs.readFileSync(options.key));
  if (privateKey.asymmetricKeyType !== "ed25519") throw new Error("The signing key must be an Ed25519 private key.");
  if (rawPublicKey(privateKey) !== options.publicKey) {
    throw new Error("That key is not the project's helper-signing key (its public half is not the pinned one). Nothing was signed.");
  }
  for (const helper of HELPERS) {
    const dir = path.join(options.root, helper.dir);
    const { files } = signBundle(dir, privateKey);
    log(`${helper.name}: signed ${files.length} files in ${helper.dir} (SHA256SUMS, SHA256SUMS.sig)`);
  }
  assertSignedBundles(options.root, options.publicKey);        // read it all back the way a release will
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    run(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
