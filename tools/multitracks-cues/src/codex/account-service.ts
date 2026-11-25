import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { createConnection } from "node:net";
import open from "open";
import path from "node:path";
import { promises as fs } from "node:fs";
import {
  createOpenAIOAuthRequest,
  exchangeOpenAIOAuthCode,
  DEFAULT_OPENAI_OAUTH_CLIENT_ID,
} from "@openai-oauth/core";
import { loadAuthTokens, saveAuthTokens } from "@openai-oauth/local/auth-file";
import { codexHome } from "./executable.js";

const LOGIN_REDIRECT_HOST = "localhost";
const LOGIN_PORT = 1455;
const LOGIN_TIMEOUT_MS = 5 * 60_000;

export function maskEmail(email: string | undefined): string | null {
  if (!email) return null;
  const [name, domain] = email.split("@");
  return domain ? `${name?.slice(0, 1) ?? "*"}***@${domain}` : "[MASKED]";
}

function authFilePath(): string {
  return path.join(codexHome(), "auth.json");
}

async function isPortReachable(host: string, port: number, timeoutMs = 100): Promise<boolean> {
  return new Promise((resolve) => {
    let settled = false;
    const socket = createConnection({ host, port });
    const settle = (value: boolean) => {
      if (settled) return;
      settled = true;
      socket.destroy();
      resolve(value);
    };
    socket.setTimeout(timeoutMs);
    socket.once("connect", () => settle(true));
    socket.once("error", () => settle(false));
    socket.once("timeout", () => settle(false));
  });
}

const callbackSuccessHtml = `<!DOCTYPE html>
<html><head><title>Sign-in complete</title></head>
<body style="font-family:system-ui,sans-serif;display:flex;justify-content:center;align-items:center;height:100vh;margin:0;background:#f5f5f5">
<div style="text-align:center;padding:2rem;background:white;border-radius:12px;box-shadow:0 2px 12px rgba(0,0,0,0.1)">
<h1 style="margin:0 0 0.5rem">Sign-in complete</h1>
<p style="color:#666;margin:0">Your ChatGPT credentials are saved locally. You can close this window and return to your terminal.</p>
</div></body></html>`;

export class CodexAccountService {
  constructor() {}

  async status(): Promise<{ authenticated: boolean; accountType: string; email: string | null; accountId: string | null }> {
    try {
      const auth = await loadAuthTokens({
        authFilePath: authFilePath(),
        fetch: globalThis.fetch.bind(globalThis),
        ensureFresh: false,
      });
      return {
        authenticated: true,
        accountType: "ChatGPT",
        email: null,
        accountId: auth.accountId,
      };
    } catch {
      return { authenticated: false, accountType: "ChatGPT", email: null, accountId: null };
    }
  }

  async login(opener = open): Promise<ReturnType<CodexAccountService["status"]>> {
    if (await isPortReachable(LOGIN_REDIRECT_HOST, LOGIN_PORT)) {
      throw new Error(`Port ${LOGIN_PORT} is already in use. Stop the process using that port and try again.`);
    }

    const redirectUri = `http://${LOGIN_REDIRECT_HOST}:${LOGIN_PORT}/auth/callback`;
    const request = await createOpenAIOAuthRequest({
      redirectUri,
      clientId: DEFAULT_OPENAI_OAUTH_CLIENT_ID,
    });

    const callbackPromise = new Promise<{ code: string }>((resolve, reject) => {
      const timeout = setTimeout(() => { cleanup(); reject(new Error("ChatGPT login timed out.")); }, LOGIN_TIMEOUT_MS);
      const server = createServer((req: IncomingMessage, res: ServerResponse) => {
        const url = new URL(req.url ?? "/", `http://${LOGIN_REDIRECT_HOST}:${LOGIN_PORT}`);
        if (url.pathname !== "/auth/callback") {
          res.writeHead(404, { "Content-Type": "text/plain" });
          res.end("Not found");
          return;
        }
        const error = url.searchParams.get("error");
        if (error) {
          res.writeHead(400, { "Content-Type": "text/html" });
          res.end(callbackSuccessHtml);
          cleanup();
          reject(new Error(`ChatGPT login failed: ${error}`));
          return;
        }
        const state = url.searchParams.get("state");
        const code = url.searchParams.get("code");
        if (!code || state !== request.state) {
          res.writeHead(400, { "Content-Type": "text/html" });
          res.end(callbackSuccessHtml);
          cleanup();
          reject(new Error("ChatGPT login callback was invalid."));
          return;
        }
        res.writeHead(200, { "Content-Type": "text/html" });
        res.end(callbackSuccessHtml);
        cleanup();
        resolve({ code });
      });

      const cleanup = () => { clearTimeout(timeout); server.close(); };
      server.listen(LOGIN_PORT, LOGIN_REDIRECT_HOST);
    });

    process.stdout.write(`OpenAI OAuth login URL: ${request.authorizationUrl}\n`);
    await opener(request.authorizationUrl);
    process.stdout.write("Complete ChatGPT sign-in in your browser.\n");

    const callback = await callbackPromise;
    const token = await exchangeOpenAIOAuthCode({
      code: callback.code,
      codeVerifier: request.codeVerifier,
      redirectUri,
      clientId: DEFAULT_OPENAI_OAUTH_CLIENT_ID,
    });

    const saved = await saveAuthTokens({ token, authFilePath: authFilePath() });
    process.stdout.write(`Credentials saved to ${saved.path}\n`);

    return this.status();
  }

  async logout(): Promise<void> {
    await fs.rm(authFilePath(), { force: true });
    if ((await this.status()).authenticated) throw new Error("ChatGPT logout was not confirmed.");
  }
}

