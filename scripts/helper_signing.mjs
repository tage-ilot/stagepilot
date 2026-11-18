// Signing of the helper bundles that ship inside StagePilot (the Playback "Library Link", the Lightkey helper and the ProPresenter Link).
//
// Each helper's folder holds the files of its release plus SHA256SUMS (every file in the folder, one line each) and
// SHA256SUMS.sig (the base64 of the 64-byte Ed25519 signature of the exact bytes of SHA256SUMS). StagePilot sends a bundle to a
// helper that is older than the one it carries, and the helper installs it only when the signature is the project's. The private
// key is on the release machine only; this file never reads it, only scripts/sign_helpers.mjs does, when it is told where it is.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

/** The project's helper-signing public key: the raw 32 bytes, base64. The helpers' source and StagePilot carry the same constant. */
export const HELPER_PUBLIC_KEY = "Evky97ydM5zWJ02p6Wc7RguEHMglX3WRxiPrege4Op8=";

export const SUMS = "SHA256SUMS";
export const SIG = "SHA256SUMS.sig";
export const HELPERS = [
  { name: "Library Link (Playback helper)", dir: "crates/stagepilot-playback/assets/playback-helper" },
  { name: "Lightkey helper", dir: "crates/stagepilot-lights/assets/lightkey-helper" },
  { name: "ProPresenter Link (ProPresenter helper)", dir: "crates/stagepilot-propresenter/assets/propresenter-link" },
];

const SPKI_ED25519_PREFIX = Buffer.from("302a300506032b6570032100", "hex");
const sha256 = (bytes) => crypto.createHash("sha256").update(bytes).digest("hex");

/** A public key object from the raw 32 bytes (base64). */
export function publicKeyObject(rawBase64) {
  const raw = Buffer.from(rawBase64, "base64");
  if (raw.length !== 32 || raw.toString("base64") !== rawBase64) throw new Error("A public key is 32 bytes in base64.");
  return crypto.createPublicKey({ key: Buffer.concat([SPKI_ED25519_PREFIX, raw]), format: "der", type: "spki" });
}

/** The raw public key (32 bytes, base64) of a private or public key object. */
export function rawPublicKey(keyObject) {
  const publicKey = keyObject.type === "private" ? crypto.createPublicKey(keyObject) : keyObject;
  const spki = publicKey.export({ format: "der", type: "spki" });
  return spki.subarray(spki.length - 32).toString("base64");
}

/** The files of a bundle: every file in the folder except the checksum list and its signature, by name. Subfolders are not allowed. */
export function bundleFiles(dir) {
  const names = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === SUMS || entry.name === SIG) continue;
    if (!entry.isFile()) throw new Error(`${dir}/${entry.name} is not a plain file; a helper bundle holds files only.`);
    if (!/^[A-Za-z0-9_][A-Za-z0-9_.-]*$/.test(entry.name)) throw new Error(`${dir}/${entry.name} is not a plain file name.`);
    names.push(entry.name);
  }
  return names.sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
}

/** The text of SHA256SUMS for a folder: `<sha256 hex>  <name>` per file, sorted by name. */
export function buildSums(dir) {
  const names = bundleFiles(dir);
  if (names.length === 0) throw new Error(`${dir} holds no files.`);
  return names.map((name) => `${sha256(fs.readFileSync(path.join(dir, name)))}  ${name}\n`).join("");
}

/** SHA256SUMS text as a Map of name to hex digest; null when a line is not `<64 hex>  <name>` or a name appears twice. */
export function parseSums(text) {
  const table = new Map();
  for (const line of text.split("\n")) {
    if (line.trim() === "") continue;
    const match = /^([0-9a-f]{64}) [ *]([A-Za-z0-9_][A-Za-z0-9_.-]*)$/.exec(line.replace(/\r$/, ""));
    if (!match || table.has(match[2])) return null;
    table.set(match[2], match[1]);
  }
  return table;
}

/** Rewrites SHA256SUMS of a folder and signs it. Writes SHA256SUMS and SHA256SUMS.sig (the base64 of the signature, one line). */
export function signBundle(dir, privateKey) {
  const sums = Buffer.from(buildSums(dir), "utf8");
  const signature = crypto.sign(null, sums, privateKey);
  fs.writeFileSync(path.join(dir, SUMS), sums);
  fs.writeFileSync(path.join(dir, SIG), `${signature.toString("base64")}\n`);
  return { files: bundleFiles(dir), signature: signature.toString("base64") };
}

/**
 * Whether a folder is a bundle the key signed: the signature file is there and is 64 bytes of base64, it verifies over the exact bytes
 * of SHA256SUMS, SHA256SUMS lists every file of the folder (and nothing else) and each file matches its line. Returns the problems,
 * an empty list when the bundle is good.
 */
export function bundleProblems(dir, publicKeyBase64 = HELPER_PUBLIC_KEY) {
  const problems = [];
  const sumsFile = path.join(dir, SUMS);
  const sigFile = path.join(dir, SIG);
  if (!fs.existsSync(sumsFile)) return [`${SUMS} is missing`];
  const sums = fs.readFileSync(sumsFile);
  const table = parseSums(sums.toString("utf8"));
  if (table === null) problems.push(`${SUMS} is not a list of '<sha256>  <name>' lines`);
  else {
    const names = bundleFiles(dir);
    for (const name of names) {
      if (!table.has(name)) problems.push(`${name} is not listed in ${SUMS}`);
      else if (table.get(name) !== sha256(fs.readFileSync(path.join(dir, name)))) problems.push(`${name} does not match ${SUMS} (changed after it was listed or signed)`);
    }
    for (const name of table.keys()) if (!names.includes(name)) problems.push(`${SUMS} lists ${name}, which is not in the folder`);
  }
  if (!fs.existsSync(sigFile)) return [...problems, `${SIG} is missing: the bundle is not signed`];
  const text = fs.readFileSync(sigFile, "utf8").trim();
  const signature = Buffer.from(text, "base64");
  if (text === "") return [...problems, `${SIG} is empty: the bundle is not signed`];
  if (signature.length !== 64 || signature.toString("base64") !== text) return [...problems, `${SIG} is not the base64 of a 64-byte signature`];
  let verified = false;
  try {
    verified = crypto.verify(null, sums, publicKeyObject(publicKeyBase64), signature);
  } catch {
    verified = false;
  }
  if (!verified) problems.push(`${SIG} is not a valid signature of ${SUMS} by the project's helper-signing key`);
  return problems;
}

/** Every bundle of the repository at `root` checked; throws one error naming everything wrong. */
export function assertSignedBundles(root, publicKeyBase64 = HELPER_PUBLIC_KEY) {
  const all = [];
  for (const helper of HELPERS) {
    for (const problem of bundleProblems(path.join(root, helper.dir), publicKeyBase64)) all.push(`${helper.name} (${helper.dir}): ${problem}`);
  }
  if (all.length) {
    throw new Error(`The helper bundles are not signed by the project. Run: node scripts/sign_helpers.mjs --key <helper-signing.pem> on the release machine.\n  ${all.join("\n  ")}`);
  }
}
