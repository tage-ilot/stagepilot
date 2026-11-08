import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

import { parseSums, verifyAll } from "./vendor-lightkey-helper.mjs";

const root = path.resolve(import.meta.dirname, "..");
const dir = path.join(root, "crates/stagepilot-lights/assets/lightkey-helper");
const sha = (bytes) => crypto.createHash("sha256").update(bytes).digest("hex");
const names = ["install.sh", "lightkey_api.py", "lightkey_cues.py", "uninstall.sh"];

test("the vendored helper files match their own SHA256SUMS and VERSION parses", () => {
  const sums = parseSums(fs.readFileSync(path.join(dir, "SHA256SUMS"), "utf8"));
  const files = new Map(names.map((n) => [n, fs.readFileSync(path.join(dir, n))]));
  verifyAll(files, sums);
  assert.match(fs.readFileSync(path.join(dir, "VERSION"), "utf8").trim(), /^\d+\.\d+\.\d+$/);
});

test("a changed file or a missing checksum line is refused", () => {
  const files = new Map(names.map((n) => [n, Buffer.from(`# ${n}\n`)]));
  const lines = names.map((n) => `${sha(files.get(n))}  ${n}`);
  verifyAll(files, parseSums(lines.join("\n")));
  files.set("install.sh", Buffer.from("# changed\n"));
  assert.throws(() => verifyAll(files, parseSums(lines.join("\n"))), /install\.sh: checksum mismatch/);
  assert.throws(() => verifyAll(files, parseSums(lines.slice(1).join("\n"))), /not listed/);
});
