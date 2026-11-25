import assert from "node:assert/strict";
import test from "node:test";
import { lockedCommit, tagFor } from "./update-versevision.mjs";

const SHA = "c45b87bb59848fb5068f009670571769273da8e7";
const lock = `[[package]]
name = "versevision-web"
version = "0.0.0"
source = "git+https://github.com/huntrw6/versevision?branch=stable#${SHA}"
dependencies = [
`;

test("the locked VerseVision commit is read from Cargo.lock", () => {
  assert.equal(lockedCommit(lock), SHA);
  assert.equal(lockedCommit(lock.replace(/\n/g, "\r\n")), SHA);
  assert.equal(lockedCommit('name = "serde"\nversion = "1.0.0"\n'), null);
});

test("the release tag of a commit is found; pre-releases and other commits are not", () => {
  const out = [
    "1111111111111111111111111111111111111111\trefs/tags/v0.9.0",
    "2222222222222222222222222222222222222222\trefs/tags/v0.9.0^{}",
    `${SHA}\trefs/tags/v1.0.0^{}`,
    "3333333333333333333333333333333333333333\trefs/tags/v1.1.0-rc1^{}",
  ].join("\n");
  assert.equal(tagFor(out, SHA), "v1.0.0");
  assert.equal(tagFor(out, "3333333333333333333333333333333333333333"), null);
  assert.equal(tagFor(out, "4444444444444444444444444444444444444444"), null);
});
