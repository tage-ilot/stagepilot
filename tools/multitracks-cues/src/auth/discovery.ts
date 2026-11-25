import { z } from "zod";
import { SchemaError } from "../errors.js";
import { EXIT } from "../constants.js";
import type { OAuthDiscovery } from "./models.js";

const resourceSchema = z.object({
  resource: z.url(),
  authorization_servers: z.array(z.url()).min(1),
  scopes_supported: z.array(z.string()).default([]),
});

const authorizationSchema = z.object({
  issuer: z.url(),
  authorization_endpoint: z.url(),
  token_endpoint: z.url(),
  revocation_endpoint: z.url().optional(),
  userinfo_endpoint: z.url().optional(),
  registration_endpoint: z.url().optional(),
  scopes_supported: z.array(z.string()).default([]),
  code_challenge_methods_supported: z.array(z.string()).default([]),
});

async function fetchJson(url: URL, fetchFn: typeof fetch): Promise<unknown> {
  const response = await fetchFn(url, { headers: { Accept: "application/json" } });
  if (!response.ok) throw new Error(`Discovery request failed (${response.status}) at ${url.origin}.`);
  return response.json();
}

export async function discoverOAuth(
  serverUrl: string,
  fetchFn: typeof fetch = fetch,
): Promise<OAuthDiscovery> {
  const server = new URL(serverUrl);
  const resourceUrl = new URL(
    `/.well-known/oauth-protected-resource${server.pathname === "/" ? "" : server.pathname}`,
    server.origin,
  );
  let resourceResult: unknown;
  try {
    resourceResult = await fetchJson(resourceUrl, fetchFn);
  } catch {
    resourceResult = await fetchJson(new URL("/.well-known/oauth-protected-resource", server.origin), fetchFn);
  }
  const resource = resourceSchema.safeParse(resourceResult);
  if (!resource.success) {
    throw new SchemaError("MultiTracks protected-resource metadata is incompatible.", EXIT.SCHEMA);
  }
  const issuer = new URL(resource.data.authorization_servers[0]!);
  const metadataUrls = [
    new URL(".well-known/oauth-authorization-server", issuer),
    new URL(".well-known/openid-configuration", issuer),
  ];
  let metadataResult: unknown;
  let lastError: unknown;
  for (const url of metadataUrls) {
    try {
      metadataResult = await fetchJson(url, fetchFn);
      break;
    } catch (error) {
      lastError = error;
    }
  }
  if (!metadataResult) throw lastError;
  const metadata = authorizationSchema.safeParse(metadataResult);
  if (!metadata.success || !metadata.data.code_challenge_methods_supported.includes("S256")) {
    throw new SchemaError("The authorization server does not advertise required S256 PKCE support.", EXIT.SCHEMA);
  }
  return {
    issuer: metadata.data.issuer,
    authorizationEndpoint: metadata.data.authorization_endpoint,
    tokenEndpoint: metadata.data.token_endpoint,
    revocationEndpoint: metadata.data.revocation_endpoint,
    userInfoEndpoint: metadata.data.userinfo_endpoint,
    registrationEndpoint: metadata.data.registration_endpoint,
    scopes: [...new Set([...resource.data.scopes_supported, ...metadata.data.scopes_supported])],
    codeChallengeMethods: metadata.data.code_challenge_methods_supported,
    protectedResource: resource.data.resource,
    authorizationServers: resource.data.authorization_servers,
  };
}

