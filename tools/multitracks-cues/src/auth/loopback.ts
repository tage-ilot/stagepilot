import { createServer } from "node:http";
import type { AddressInfo } from "node:net";

export interface AuthorizationCallback {
  code: string;
  state: string | null;
}

export async function listenForAuthorizationCallback(timeoutMs = 180_000, port = 0): Promise<{
  redirectUrl: string;
  result: Promise<AuthorizationCallback>;
  close: () => Promise<void>;
}> {
  let resolveResult!: (result: AuthorizationCallback) => void;
  let rejectResult!: (error: Error) => void;
  const result = new Promise<AuthorizationCallback>((resolve, reject) => {
    resolveResult = resolve;
    rejectResult = reject;
  });
  const server = createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    if (url.pathname !== "/oauth/callback") {
      response.writeHead(404).end("Not found");
      return;
    }
    const error = url.searchParams.get("error");
    const code = url.searchParams.get("code");
    if (error || !code) {
      response.writeHead(400, { "Content-Type": "text/plain; charset=utf-8" });
      response.end("StagePilot authorization failed. You may close this window.");
      rejectResult(new Error(`Authorization failed: ${error ?? "authorization code missing"}.`));
      return;
    }
    response.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
    response.end("StagePilot authorization completed. You may close this window.");
    resolveResult({ code, state: url.searchParams.get("state") });
  });
  server.listen(port, "127.0.0.1");
  await new Promise<void>((resolve, reject) => {
    server.once("listening", resolve);
    server.once("error", reject);
  });
  const address = server.address() as AddressInfo;
  const timer = setTimeout(() => rejectResult(new Error("OAuth login timed out.")), timeoutMs);
  timer.unref();
  return {
    redirectUrl: `http://127.0.0.1:${address.port}/oauth/callback`,
    result: result.finally(() => clearTimeout(timer)),
    close: async () =>
      new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve()))),
  };
}

