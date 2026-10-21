// Publish a release and its assets through the GitHub REST API.
//
// The publish job deliberately does not depend on the `gh` CLI. Node 22 is set
// up in that job and provides global `fetch`, so the release is created,
// populated and finalized here through the REST API instead of shelling out.
//
// Immutability is preserved exactly as before: if a release already exists for
// the tag, this exits non-zero and uploads nothing.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const repository = process.env.GITHUB_REPOSITORY;
const token = process.env.GITHUB_TOKEN;
const tag = process.argv[2];
const directory = process.argv[3];

if (!repository) throw new Error("GITHUB_REPOSITORY is not set.");
if (!token) throw new Error("GITHUB_TOKEN is not set.");
if (!tag || !directory) {
  throw new Error("Usage: node scripts/publish_release.mjs TAG RELEASE_ASSETS_DIR");
}
// Stable `vX.Y.Z`, or a pre-release `vX.Y.Z-(alpha|beta|rc).N` (the same set validate_versions.mjs accepts).
if (!/^v\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?$/.test(tag)) {
  throw new Error(`Invalid release tag: ${tag}`);
}

// A pre-release is published as a GitHub pre-release and is NEVER marked "Latest". The in-app
// updater reads https://github.com/<repo>/releases/latest/download/latest.json, and GitHub's
// /releases/latest resolves to the newest release that is neither a draft nor a pre-release, so a
// pre-release can only be reached through its own tag URL and never offered to installed apps.
const prerelease = tag.includes("-");

const api = "https://api.github.com";
const uploads = "https://uploads.github.com";
const headers = {
  Accept: "application/vnd.github+json",
  Authorization: `Bearer ${token}`,
  "X-GitHub-Api-Version": "2022-11-28",
  "User-Agent": "stagepilot-release",
};

// Never let a failing request echo the Authorization header or a signed upload
// URL into the log.
const request = async (url, options = {}) => {
  const response = await fetch(url, { ...options, headers: { ...headers, ...options.headers } });
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    const message = detail.slice(0, 500).replace(/https:\/\/\S+/g, "<url>");
    throw new Error(`${options.method ?? "GET"} ${new URL(url).pathname} failed: ${response.status} ${message}`);
  }
  return response;
};

const existing = await fetch(`${api}/repos/${repository}/releases/tags/${tag}`, { headers });
if (existing.ok) {
  console.error(`Release ${tag} already exists; immutable release assets will not be replaced.`);
  process.exit(1);
}
if (existing.status !== 404) {
  throw new Error(`Unexpected status while checking for ${tag}: ${existing.status}`);
}

const notesPath = path.join(directory, "release-notes.md");
const created = await (
  await request(`${api}/repos/${repository}/releases`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      tag_name: tag,
      name: `StagePilot ${tag}`,
      body: fs.readFileSync(notesPath, "utf8"),
      draft: true,
      ...(prerelease ? { prerelease: true } : {}),
      make_latest: "false",
    }),
  })
).json();

// Signatures stay in the staging directory while latest.json is generated and
// validated, but latest.json embeds their contents. Standalone .sig files are
// deliberately not published as user-facing release downloads.
const entries = fs
  .readdirSync(directory, { withFileTypes: true })
  .filter((entry) => entry.isFile())
  .map((entry) => entry.name)
  .filter((name) => name !== "release-notes.md" && !name.endsWith(".sig"))
  .sort();

// latest.json is uploaded last so no client can observe a manifest that
// references an asset which is not yet present.
const ordered = [...entries.filter((n) => n !== "latest.json"), ...entries.filter((n) => n === "latest.json")];
if (!ordered.includes("latest.json")) throw new Error("latest.json is missing from the staging directory.");

for (const name of ordered) {
  const body = fs.readFileSync(path.join(directory, name));
  await request(`${uploads}/repos/${repository}/releases/${created.id}/assets?name=${encodeURIComponent(name)}`, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream", "Content-Length": String(body.byteLength) },
    body,
  });
  console.log(`uploaded ${name} (${body.byteLength} bytes)`);
}

const published = await (
  await request(`${api}/repos/${repository}/releases/${created.id}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    // Pre-releases must keep prerelease:true and must not request make_latest at all.
    body: JSON.stringify(prerelease ? { draft: false, prerelease: true } : { draft: false, make_latest: "true" }),
  })
).json();
if (prerelease && published.prerelease !== true) {
  throw new Error(`Pre-release ${tag} was not published as a pre-release; refusing to continue.`);
}

console.log(
  JSON.stringify(
    {
      id: published.id,
      tag: published.tag_name,
      draft: published.draft,
      prerelease: published.prerelease,
      url: published.html_url,
      assets: published.assets.map((asset) => ({ name: asset.name, size: asset.size })),
    },
    null,
    2,
  ),
);
