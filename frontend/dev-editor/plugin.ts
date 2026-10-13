// Dev-only visual text/colour editor. `apply: "serve"` keeps every part of it
// (tagging, overlay script, HTTP endpoint) out of `vite build` output.
import { execFile } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Plugin } from "vite";
import {
  EditError,
  describeElement,
  editColor,
  removeColor,
  editCssVar,
  editDesignMd,
  editText,
  safeSrcPath,
  tagJsx,
  type ColorProp,
} from "./source";

const BASE = "/__dev-editor";
const OVERLAY_URL = "/dev-editor/overlay.ts";

function run(cmd: string, args: string[], cwd: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { cwd, timeout: 30_000 }, (err, stdout, stderr) =>
      err ? reject(new EditError((stderr || err.message).trim().slice(0, 400), 500)) : resolve(stdout.trim()),
    );
  });
}

function readBody(req: IncomingMessage): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let size = 0;
    req.on("data", (c: Buffer) => {
      size += c.length;
      if (size > 64_000) {
        reject(new EditError("Body too large", 413));
        req.destroy();
      } else chunks.push(c);
    });
    req.on("end", () => {
      try {
        const v: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}");
        if (v && typeof v === "object" && !Array.isArray(v)) resolve(v as Record<string, unknown>);
        else reject(new EditError("Body must be an object"));
      } catch {
        reject(new EditError("Bad JSON"));
      }
    });
    req.on("error", reject);
  });
}

function send(res: ServerResponse, status: number, body: unknown) {
  res.statusCode = status;
  res.setHeader("content-type", "application/json");
  res.setHeader("cache-control", "no-store");
  res.end(JSON.stringify(body));
}

const int = (v: unknown, name: string): number => {
  if (typeof v !== "number" || !Number.isInteger(v) || v < 1) throw new EditError(`${name} must be a positive integer`);
  return v;
};
const str = (v: unknown, name: string): string => {
  if (typeof v !== "string") throw new EditError(`${name} must be a string`);
  return v;
};

export interface VisualEditorOptions {
  /** Directory containing package.json, tailwind.config.js and src/. */
  frontendRoot: string;
  /** Override the per-run token (tests). */
  token?: string;
}

export function visualEditor(options: VisualEditorOptions): Plugin {
  const frontendRoot = fs.realpathSync(options.frontendRoot);
  const srcRoot = path.join(frontendRoot, "src");
  const token = options.token ?? crypto.randomBytes(24).toString("hex");
  const touched = new Set<string>(); // repo-relative paths this session wrote
  let repoRoot = path.resolve(frontendRoot, "..");
  let themeColors: Record<string, string> = {};

  async function loadTheme() {
    const url = pathToFileURL(path.join(frontendRoot, "tailwind.config.js")).href + `?t=${Date.now()}`;
    const mod = (await import(/* @vite-ignore */ url)) as { default: { theme: { extend: { colors: Record<string, string> } } } };
    themeColors = mod.default.theme.extend.colors;
  }
  const tokenNames = () => new Set(Object.keys(themeColors));

  function track(abs: string) {
    touched.add(path.relative(repoRoot, abs).split(path.sep).join("/"));
  }

  function writeIfChanged(abs: string, before: string, after: string) {
    if (before !== after) {
      fs.writeFileSync(abs, after);
      track(abs);
    }
  }

  async function handle(action: string, body: Record<string, unknown>): Promise<unknown> {
    if (action === "describe") {
      const abs = safeSrcPath(srcRoot, str(body.file, "file"));
      await loadTheme();
      const info = describeElement(fs.readFileSync(abs, "utf8"), int(body.line, "line"), int(body.col, "col"), tokenNames());
      return { ...info, theme: themeColors };
    }
    if (action === "save") {
      const kind = str(body.kind, "kind");
      if (kind === "token") {
        const hex = str(body.hex, "hex");
        const design = path.join(repoRoot, "DESIGN.md");
        const before = fs.readFileSync(design, "utf8");
        writeIfChanged(design, before, editDesignMd(before, str(body.name, "name"), hex));
        await run(process.execPath, ["scripts/design-tokens.mjs"], frontendRoot);
        for (const f of ["src/design-tokens.css", "tailwind.config.js"]) track(path.join(frontendRoot, f));
        await loadTheme();
        return { ok: true };
      }
      const abs = safeSrcPath(srcRoot, str(body.file, "file"));
      const line = int(body.line, "line");
      const col = int(body.col, "col");
      const before = fs.readFileSync(abs, "utf8");
      let after: string;
      if (kind === "text") {
        after = editText(before, line, col, int((body.segment as number) + 1, "segment") - 1, str(body.text, "text"), body.old as string | undefined);
      } else if (kind === "color") {
        await loadTheme();
        const prop = str(body.prop, "prop");
        if (prop !== "text" && prop !== "bg" && prop !== "border") throw new EditError("prop must be text, bg or border");
        const from = body.from === undefined ? undefined : str(body.from, "from");
        const value = str(body.value, "value");
        after = value === "" ? removeColor(before, line, col, prop as ColorProp, str(body.from, "from"), tokenNames()) : editColor(before, line, col, prop as ColorProp, value, tokenNames(), from);
      } else throw new EditError("kind must be text, color or token");
      writeIfChanged(abs, before, after);
      return { ok: true, changed: before !== after };
    }
    if (action === "css-var") {
      const abs = safeSrcPath(srcRoot, "index.css");
      const before = fs.readFileSync(abs, "utf8");
      writeIfChanged(abs, before, editCssVar(before, str(body.name, "name"), str(body.value, "value")));
      return { ok: true };
    }
    if (action === "status") {
      const branch = await run("git", ["rev-parse", "--abbrev-ref", "HEAD"], repoRoot);
      return { branch, files: [...touched] };
    }
    if (action === "commit") {
      const branch = await run("git", ["rev-parse", "--abbrev-ref", "HEAD"], repoRoot);
      if (["main", "master", "HEAD"].includes(branch)) throw new EditError(`Refusing to commit on '${branch}'; switch to a dev branch`, 403);
      const files = [...touched];
      if (!files.length) throw new EditError("Nothing saved this session to commit", 409);
      const message = str(body.message ?? "chore(frontend): visual editor changes", "message").slice(0, 200) || "chore(frontend): visual editor changes";
      await run("git", ["add", "--", ...files], repoRoot);
      const staged = await run("git", ["diff", "--cached", "--name-only", "--", ...files], repoRoot);
      if (!staged) throw new EditError("No changes to commit", 409);
      await run("git", ["commit", "-m", message, "--", ...files], repoRoot);
      touched.clear();
      return { ok: true, branch, commit: await run("git", ["rev-parse", "--short", "HEAD"], repoRoot) };
    }
    throw new EditError("Unknown action", 404);
  }

  return {
    name: "stagepilot-visual-editor",
    apply: "serve",
    enforce: "pre",
    configResolved(config) {
      repoRoot = path.resolve(config.root, "..");
    },
    transform(code, id) {
      const file = id.split("?")[0]!;
      if (!/\.tsx$/.test(file) || /\.test\.tsx$/.test(file) || file.includes("/node_modules/")) return null;
      const rel = path.relative(srcRoot, file);
      if (rel.startsWith("..") || path.isAbsolute(rel)) return null;
      return { code: tagJsx(code, rel.split(path.sep).join("/")), map: null };
    },
    transformIndexHtml() {
      return [
        { tag: "meta", attrs: { name: "dev-editor-token", content: token }, injectTo: "head" },
        { tag: "script", attrs: { type: "module", src: OVERLAY_URL }, injectTo: "body" },
      ];
    },
    configureServer(server) {
      server.middlewares.use(BASE, (req, res, next) => {
        void (async () => {
          try {
            if (req.method !== "GET" && req.method !== "POST") {
              res.setHeader("allow", "GET, POST");
              return send(res, 405, { error: "Method not allowed" });
            }
            const given = req.headers["x-dev-editor-token"];
            const ok =
              typeof given === "string" &&
              given.length === token.length &&
              crypto.timingSafeEqual(Buffer.from(given), Buffer.from(token));
            if (!ok) return send(res, 401, { error: "Missing or wrong editor token" });
            const origin = req.headers.origin;
            if (origin && new URL(origin).host !== req.headers.host) return send(res, 403, { error: "Cross-origin request" });
            const action = (req.url ?? "/").split("?")[0]!.replace(/^\//, "");
            if (req.method === "GET" && action !== "status") return send(res, 405, { error: "Use POST" });
            const body = req.method === "POST" ? await readBody(req) : {};
            send(res, 200, await handle(action, body));
          } catch (e) {
            if (e instanceof EditError) send(res, e.status, { error: e.message });
            else {
              send(res, 500, { error: "Internal editor error" });
              server.config.logger.error(String(e));
            }
          }
        })().catch(next);
      });
    },
  };
}
