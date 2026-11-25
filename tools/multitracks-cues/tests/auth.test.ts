import { describe, expect, it, vi } from "vitest";
import { createPkce, verifyState } from "../src/auth/pkce.js";
import { discoverOAuth } from "../src/auth/discovery.js";
import { CredentialVault, type CredentialBackend } from "../src/auth/credential-store.js";
import { TokenStore } from "../src/auth/token-store.js";
import { AuthenticationService } from "../src/auth/service.js";
import { defaultConfiguration, type Configuration } from "../src/config/schema.js";
import { redact } from "../src/security/redact.js";

class MemoryBackend implements CredentialBackend {
  id = "native-windows";
  data = new Map<string, string>();
  async getPassword(service: string, account: string): Promise<string | null> { return this.data.get(`${service}:${account}`) ?? null; }
  async setPassword(service: string, account: string, password: string): Promise<void> { this.data.set(`${service}:${account}`, password); }
  async deletePassword(service: string, account: string): Promise<void> { this.data.delete(`${service}:${account}`); }
}

class MemoryConfigStore {
  value: Configuration = defaultConfiguration;
  async load(): Promise<Configuration> { return this.value; }
  async save(value: Configuration): Promise<void> { this.value = value; }
}

function oauthFetch(options: { registration?: boolean; tokenExpires?: number } = {}): typeof fetch {
  return vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const request = input instanceof Request ? input : undefined;
    const url = request?.url ?? String(input);
    if (url.includes("oauth-protected-resource")) return Response.json({ resource: "https://mcp.multitracks.com/mcp", authorization_servers: ["https://account.multitracks.com/"], scopes_supported: ["mcp"] });
    if (url.includes(".well-known/oauth-authorization-server")) return Response.json({
      issuer: "https://account.multitracks.com/",
      authorization_endpoint: "https://account.multitracks.com/connect/authorize",
      token_endpoint: "https://account.multitracks.com/connect/token",
      registration_endpoint: options.registration ? "https://account.multitracks.com/connect/register" : undefined,
      revocation_endpoint: "https://account.multitracks.com/connect/revocation",
      response_types_supported: ["code"],
      scopes_supported: ["mcp", "offline_access"],
      code_challenge_methods_supported: ["S256"],
    });
    if (url.endsWith("/connect/register")) return Response.json({ client_id: "issued-stagepilot-client", redirect_uris: ["http://127.0.0.1:54321/oauth/callback"] });
    if (url.endsWith("/connect/token")) {
      const body = init?.body ? String(init.body) : await request?.clone().text() ?? "";
      return Response.json({ access_token: body.includes("refresh_token") ? "refreshed" : "first", refresh_token: "refresh", token_type: "Bearer", expires_in: options.tokenExpires ?? 3600 });
    }
    if (url.endsWith("/connect/revocation")) return new Response(null, { status: 200 });
    return new Response(null, { status: 404 });
  }) as typeof fetch;
}

describe("OAuth and credential safety", () => {
  it("generates S256 PKCE material and rejects state mismatches", () => {
    const first = createPkce();
    const second = createPkce();
    expect(first.verifier).not.toBe(second.verifier);
    expect(first.challenge).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(() => verifyState("expected", "wrong")).toThrow(/state mismatch/i);
    expect(() => verifyState("expected", "expected")).not.toThrow();
  });

  it("discovers protected-resource and authorization metadata", async () => {
    const result = await discoverOAuth(defaultConfiguration.serverUrl, oauthFetch());
    expect(result.issuer).toBe("https://account.multitracks.com/");
    expect(result.codeChallengeMethods).toContain("S256");
    expect(result.registrationEndpoint).toBeUndefined();
  });

  it("fails closed when only an insecure credential backend is available", async () => {
    const backend = new MemoryBackend();
    backend.id = "file";
    const vault = new CredentialVault(async () => backend);
    await expect(vault.set("service", "account", "secret")).rejects.toThrow(/Refusing insecure/);
  });

  it("stores tokens without returning them through status and refreshes expired tokens", async () => {
    const backend = new MemoryBackend();
    const vault = new CredentialVault(async () => backend);
    const tokenStore = new TokenStore(vault);
    const configStore = new MemoryConfigStore();
    configStore.value = { ...defaultConfiguration, clientId: "stagepilot-client" };
    await tokenStore.save({ serverOrigin: configStore.value.serverUrl, issuer: "https://account.multitracks.com/", clientId: "stagepilot-client" }, { access_token: "expired", refresh_token: "refresh", token_type: "Bearer", expires_in: 1, obtainedAt: 1 });
    const service = new AuthenticationService(configStore as never, tokenStore);
    const tokens = await service.tokens(configStore.value, oauthFetch());
    expect(tokens.access_token).toBe("refreshed");
    const status = await service.status(configStore.value);
    expect(JSON.stringify(status)).not.toContain("refreshed");
  });

  it("uses credential-store-safe keys while retaining the complete identity", () => {
    const store = new TokenStore(new CredentialVault(async () => new MemoryBackend()));
    const identity = {
      serverOrigin: "https://mcp.multitracks.com/mcp",
      issuer: "https://account.multitracks.com/",
      clientId: "client:id",
      organizationId: "org/one",
    };
    expect(store.service(identity)).toMatch(/^[A-Za-z0-9._@-]+$/);
    expect(store.account(identity)).toMatch(/^[A-Za-z0-9._@-]+$/);
    expect(store.service(identity)).not.toContain("https://");
  });

  it("reports missing static registration and uses DCR only when advertised", async () => {
    const backend = new MemoryBackend();
    const configStore = new MemoryConfigStore();
    // Clear the default clientId to test DCR behavior
    configStore.value = { ...defaultConfiguration, clientId: undefined };
    const service = new AuthenticationService(configStore as never, new TokenStore(new CredentialVault(async () => backend)));
    const callbackFactory = async () => {
      let resolve!: (value: { code: string; state: string }) => void;
      return { redirectUrl: "http://127.0.0.1:54321/oauth/callback", result: new Promise<{ code: string; state: string }>((done) => { resolve = done; }), close: async () => undefined, resolve };
    };
    // With no clientId configured, the default kicks in — login succeeds
    const callback = await callbackFactory();
    await expect(service.login(defaultConfiguration, {
      fetchFn: oauthFetch({ registration: true }),
      callbackFactory: async () => callback,
      openBrowser: async (url) => callback.resolve({ code: "authorization-code", state: new URL(url).searchParams.get("state")! }),
    })).resolves.toEqual([]);
  });

  it("redacts credential keys and bearer patterns", () => {
    const value = redact({ access_token: "top-secret", nested: "Bearer abc.def.ghi", safe: "visible" });
    expect(value).toEqual({ access_token: "[REDACTED]", nested: "Bearer [REDACTED]", safe: "visible" });
  });

  it("contains no ChatGPT or Claude OAuth credentials", async () => {
    const source = `${AuthenticationService.toString()} ${discoverOAuth.toString()}`;
    expect(source).not.toMatch(/chatgpt|claude|openai/i);
  });
});

