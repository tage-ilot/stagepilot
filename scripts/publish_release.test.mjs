// Request-body tests for scripts/publish_release.mjs with a mocked fetch (no network).
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

const root = path.resolve(import.meta.dirname, "..");

function publish(tag, env = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "stagepilot-publish-"));
  try {
    const assets = path.join(dir, "assets");
    fs.mkdirSync(assets);
    fs.writeFileSync(path.join(assets, "release-notes.md"), "notes");
    fs.writeFileSync(path.join(assets, "latest.json"), "{}");
    fs.writeFileSync(path.join(assets, "StagePilot_x.dmg"), "dmg");
    const logFile = path.join(dir, "requests.json");
    const result = spawnSync(
      process.execPath,
      ["--import", pathToFileURL(path.join(root, "scripts/publish_release_mock_fetch.mjs")).href, path.join(root, "scripts/publish_release.mjs"), tag, assets],
      {
        encoding: "utf8",
        env: { ...process.env, GITHUB_REPOSITORY: "tage-ilot/stagepilot", GITHUB_TOKEN: "test-token", MOCK_FETCH_LOG: logFile, ...env },
      },
    );
    const requests = fs.existsSync(logFile) ? JSON.parse(fs.readFileSync(logFile, "utf8")) : [];
    return { ...result, requests };
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

const releaseRequests = (requests, method) =>
  requests.filter((r) => r.method === method && new URL(r.url).host === "api.github.com" && !r.url.includes("/releases/tags/"));

for (const tag of ["v2.42.0-rc.1", "v2.42.0-beta.3", "v2.42.0-alpha.1"]) {
  test(`${tag} is created and published as a pre-release, never as Latest`, () => {
    const { status, stderr, requests } = publish(tag);
    assert.equal(status, 0, stderr);
    const [create] = releaseRequests(requests, "POST");
    assert.equal(create.body.prerelease, true);
    assert.equal(create.body.draft, true);
    assert.equal(create.body.make_latest, "false");
    const patches = releaseRequests(requests, "PATCH");
    assert.equal(patches.length, 1);
    assert.deepEqual(patches[0].body, { draft: false, prerelease: true });
    assert.ok(!requests.some((r) => r.body && r.body.make_latest === "true"), "must never request make_latest=true");
  });
}

test("a plain X.Y.Z tag keeps the existing request bodies and is made Latest", () => {
  const { status, stderr, requests } = publish("v2.42.0");
  assert.equal(status, 0, stderr);
  const [create] = releaseRequests(requests, "POST");
  assert.deepEqual(Object.keys(create.body).sort(), ["body", "draft", "make_latest", "name", "tag_name"]);
  assert.equal(create.body.draft, true);
  assert.equal(create.body.make_latest, "false");
  assert.deepEqual(releaseRequests(requests, "PATCH").map((r) => r.body), [{ draft: false, make_latest: "true" }]);
});

test("latest.json is uploaded last", () => {
  const { requests } = publish("v2.42.0-rc.1");
  const uploads = requests.filter((r) => r.url.includes("/assets?name=")).map((r) => new URL(r.url).searchParams.get("name"));
  assert.equal(uploads.at(-1), "latest.json");
});

for (const tag of ["v2.42.0", "v2.42.0-rc.1"]) {
  test(`an existing release for ${tag} aborts before anything is created or uploaded`, () => {
    const { status, stderr, requests } = publish(tag, { MOCK_EXISTING_RELEASE: "1" });
    assert.notEqual(status, 0);
    assert.match(stderr, /already exists/);
    assert.deepEqual(requests.map((r) => r.method), ["GET"]);
  });
}

test("a pre-release that the API reports as non-pre-release fails the publish", () => {
  const { status, stderr } = publish("v2.42.0-rc.1", { MOCK_PATCH_DROPS_PRERELEASE: "1" });
  assert.notEqual(status, 0);
  assert.match(stderr, /not published as a pre-release/);
});

for (const tag of ["v2.42.0-rc", "v2.42.0-foo.1", "v2.42", "2.42.0", "v2.42.0-rc.1+build"]) {
  test(`invalid tag ${tag} is rejected before any request`, () => {
    const { status, stderr, requests } = publish(tag);
    assert.notEqual(status, 0);
    assert.match(stderr, /Invalid release tag/);
    assert.equal(requests.length, 0);
  });
}
