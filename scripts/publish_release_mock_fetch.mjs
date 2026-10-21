// Test-only preload for scripts/publish_release.test.mjs: replaces global fetch with an in-memory
// GitHub stand-in and records every request (method, URL, JSON body) to MOCK_FETCH_LOG.
// MOCK_EXISTING_RELEASE=1 makes the tag lookup find an existing release.
import fs from "node:fs";

const log = [];
const flush = () => fs.writeFileSync(process.env.MOCK_FETCH_LOG, JSON.stringify(log));
const reply = (status, body) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

globalThis.fetch = async (url, options = {}) => {
  const method = options.method ?? "GET";
  const text = typeof options.body === "string" ? options.body : undefined;
  log.push({ method, url: String(url), body: text ? JSON.parse(text) : undefined });
  flush();
  const { pathname } = new URL(url);
  if (method === "GET" && pathname.includes("/releases/tags/")) {
    return process.env.MOCK_EXISTING_RELEASE === "1" ? reply(200, { id: 1 }) : reply(404, {});
  }
  if (method === "POST" && pathname.endsWith("/releases")) return reply(201, { id: 42 });
  if (method === "POST" && pathname.endsWith("/assets")) return reply(201, {});
  if (method === "PATCH") {
    const sent = JSON.parse(text);
    const stayPrerelease = process.env.MOCK_PATCH_DROPS_PRERELEASE !== "1";
    return reply(200, {
      id: 42,
      tag_name: "mock",
      draft: sent.draft,
      prerelease: stayPrerelease ? sent.prerelease === true : false,
      html_url: "https://example.invalid/release",
      assets: [],
    });
  }
  return reply(500, { message: `unexpected ${method} ${pathname}` });
};
