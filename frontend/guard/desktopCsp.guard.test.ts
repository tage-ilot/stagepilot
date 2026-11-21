import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import { playbackLibraryArtworkUrl, resolveApiOrigin } from "../src/api";

/**
 * The guard (DESIGN.md, song cards): the desktop window's Content-Security-Policy lets the page load what the frontend builds a URL for.
 *
 * In the Mac and Windows app the page is `tauri://localhost` (Windows: `http://tauri.localhost`) and every address the frontend builds
 * (`apiOrigin`: `http://127.0.0.1:<port>`, its WebSocket `ws://127.0.0.1:<port>`) is a DIFFERENT origin, so `'self'` does not cover it.
 * `fetch` is `connect-src`; an `<img>` is `img-src`. The album covers were missing for weeks because `connect-src` had the loopback origin and
 * `img-src` did not: the pictures were blocked, nothing was logged anywhere a person looks, and a browser preview (same origin) showed them.
 *
 * This test reads `desktop/src-tauri/tauri.conf.json` and fails with what to write. It is a text check of the policy, not a browser run.
 */
// `__dirname` (not import.meta.url): this file runs in jsdom, because it imports api.ts, which reads `window`.
const desktop = `${resolve(__dirname, "../../desktop/src-tauri")}/`;
const frontend = `${resolve(__dirname, "..")}/`;
const CONF = "desktop/src-tauri/tauri.conf.json";

/** `"default-src 'self'; img-src 'self' data:"` -> `{ "default-src": ["'self'"], "img-src": ["'self'", "data:"] }`. */
function parseCsp(csp: string): Record<string, string[]> {
  const directives: Record<string, string[]> = {};
  for (const part of csp.split(";")) {
    const [name, ...sources] = part.trim().split(/\s+/);
    if (name) directives[name.toLowerCase()] = sources;
  }
  return directives;
}

/** Does a CSP source list allow this URL? Only the forms this app can need: `scheme://host`, `scheme://host:port`, `scheme://host:*`. */
function allows(sources: string[], url: string): boolean {
  const target = new URL(url);
  return sources.some((source) => {
    const match = /^([a-z][a-z0-9+.-]*):\/\/([^:/]+)(?::(\*|\d+))?$/i.exec(source);
    if (!match) return false;
    const [, scheme, host, port] = match;
    if (scheme!.toLowerCase() + ":" !== target.protocol || host!.toLowerCase() !== target.hostname) return false;
    return port === "*" || (port ?? "") === target.port;
  });
}

/** A source `img-src` may list: the page itself, inline `data:` pictures, and the loopback server (any port, the same range `connect-src` has). */
const IMG_SOURCE = /^('self'|data:|http:\/\/127\.0\.0\.1:(\*|\d+))$/;

/** Every problem with a policy, each saying what to write. Empty: the policy is right. */
function cspProblems(csp: string): string[] {
  const directives = parseCsp(csp);
  const problems: string[] = [];
  // What the desktop frontend builds a URL for, in the shapes `resolveApiOrigin` gives (any saved port, the default 8765).
  const origins = [8765, 1, 8080, 65535].map((port) => resolveApiOrigin({ protocol: "tauri:", origin: "tauri://localhost", hostname: "localhost", port: "" }, true, false, undefined, port));
  for (const [directive, kind] of [["connect-src", "http"], ["connect-src", "ws"], ["img-src", "http"]] as const) {
    const sources = directives[directive] ?? directives["default-src"] ?? [];
    for (const origin of origins) {
      const url = kind === "ws" ? origin.replace(/^http/, "ws") : origin;
      if (!allows(sources, url)) {
        const write = kind === "ws" ? "ws://127.0.0.1:*" : "http://127.0.0.1:*";
        problems.push(`${CONF} app.security.csp: ${directive} does not allow ${url}. The frontend builds URLs from apiOrigin (api.ts), a different origin from the desktop window, so 'self' does not cover it: write ${write} in ${directive}.`);
        break;
      }
    }
  }
  const img = directives["img-src"];
  if (!img) problems.push(`${CONF} app.security.csp: there is no img-src, so it falls back to default-src 'self' and every cover is blocked. Write img-src 'self' data: http://127.0.0.1:*`);
  for (const source of img ?? []) {
    if (!IMG_SOURCE.test(source)) problems.push(`${CONF} app.security.csp: img-src lists ${source}. Pictures may come only from 'self', data: and http://127.0.0.1:<port or *>. No wildcard host, no https:, no http:, no blob:. Serve the picture through StagePilot's own API (apiOrigin) instead; if a new kind of source is truly needed, change IMG_SOURCE in this test in the same PR and say why.`);
  }
  return problems;
}

const policy = (conf: { app?: { security?: { csp?: unknown } } }): string => {
  const csp = conf.app?.security?.csp;
  if (typeof csp !== "string") throw new Error(`${CONF} app.security.csp is not a string; the desktop window needs a policy (see this test).`);
  return csp;
};
const confs = readdirSync(desktop).filter((name) => /^tauri(\..+)?\.conf\.json$/.test(name));

describe("desktop window Content-Security-Policy", () => {
  it("allows every origin the frontend builds a URL for, in connect-src and img-src, and nothing wider for pictures", () => {
    const conf = JSON.parse(readFileSync(`${desktop}tauri.conf.json`, "utf8"));
    expect(cspProblems(policy(conf))).toEqual([]);
  });

  it("has one policy: no other Tauri config or page overrides it", () => {
    expect(confs).toContain("tauri.conf.json");
    for (const name of confs.filter((file) => file !== "tauri.conf.json")) {
      const conf = JSON.parse(readFileSync(`${desktop}${name}`, "utf8"));
      expect(conf.app?.security, `${name} sets app.security: keep the one policy in tauri.conf.json (or make this test read both and check each)`).toBeUndefined();
    }
    expect(readFileSync(`${frontend}index.html`, "utf8").toLowerCase(), "frontend/index.html has a CSP meta tag that would override the desktop policy").not.toContain("content-security-policy");
  });

  it("builds the artwork address on the same origin the policy was checked against", () => {
    const origin = resolveApiOrigin({ protocol: "tauri:", origin: "tauri://localhost", hostname: "localhost", port: "" }, true, false, undefined, 8765);
    expect(origin).toBe("http://127.0.0.1:8765");
    // apiOrigin in this (jsdom) run is the test page's own origin; what matters is the route hangs off it.
    expect(playbackLibraryArtworkUrl(7, "abc")).toMatch(/^https?:\/\/[^/]+\/api\/v1\/playback-library\/songs\/7\/artwork\?v=abc$/);
  });

  describe("the checker itself", () => {
    const WITH = "default-src 'self'; connect-src 'self' http://127.0.0.1:* ws://127.0.0.1:*; font-src 'self' data:; img-src 'self' data: http://127.0.0.1:*; style-src 'self' 'unsafe-inline'";

    it("passes the fixed policy", () => expect(cspProblems(WITH)).toEqual([]));

    it("fails the policy that blocked every cover, and says what to write", () => {
      const problems = cspProblems(WITH.replace("img-src 'self' data: http://127.0.0.1:*", "img-src 'self' data:"));
      expect(problems.length).toBeGreaterThan(0);
      expect(problems[0]).toContain("img-src does not allow http://127.0.0.1:8765");
      expect(problems[0]).toContain("write http://127.0.0.1:* in img-src");
    });

    it("fails a missing img-src (falls back to 'self') and a missing connect-src entry", () => {
      expect(cspProblems(WITH.replace(" img-src 'self' data: http://127.0.0.1:*;", "")).join("\n")).toContain("img-src does not allow");
      expect(cspProblems(WITH.replace(" ws://127.0.0.1:*", "")).join("\n")).toContain("write ws://127.0.0.1:* in connect-src");
    });

    it.each(["*", "https:", "http:", "blob:", "https://*.example.com", "http://*", "http://localhost:*", "http://127.0.0.1"])("fails a wider img-src source: %s", (source) => {
      expect(cspProblems(WITH.replace("img-src 'self' data: http://127.0.0.1:*", `img-src 'self' data: http://127.0.0.1:* ${source}`)).join("\n")).toContain(`img-src lists ${source}`);
    });

    it("matches a host source by scheme, host and port", () => {
      expect(allows(["http://127.0.0.1:*"], "http://127.0.0.1:8765")).toBe(true);
      expect(allows(["http://127.0.0.1:8765"], "http://127.0.0.1:8766")).toBe(false);
      expect(allows(["http://127.0.0.1:*"], "ws://127.0.0.1:8765")).toBe(false);
      expect(allows(["http://127.0.0.1:*"], "http://127.0.0.2:8765")).toBe(false);
      expect(allows(["'self'", "data:"], "http://127.0.0.1:8765")).toBe(false);
    });
  });
});
