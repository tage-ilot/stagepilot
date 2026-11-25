import open from "open";
import {
  exchangeAuthorization,
  refreshAuthorization,
  registerClient,
} from "@modelcontextprotocol/sdk/client/auth.js";
import type {
  AuthorizationServerMetadata,
  OAuthClientInformationMixed,
  OAuthClientMetadata,
  OAuthTokens,
} from "@modelcontextprotocol/sdk/shared/auth.js";
import { DEFAULT_SCOPE, DEFAULT_MULTITRACKS_CLIENT_ID, DEFAULT_MULTITRACKS_CLIENT_SECRET, EXIT } from "../constants.js";
import { AuthenticationError } from "../errors.js";
import type { Configuration } from "../config/schema.js";
import type { ConfigurationStore } from "../config/store.js";
import { discoverOAuth } from "./discovery.js";
import type { OrganizationIdentity } from "./models.js";
import { listenForAuthorizationCallback } from "./loopback.js";
import { createPkce, createState, verifyState } from "./pkce.js";
import { TokenStore, type TokenStoreIdentity } from "./token-store.js";

export interface LoginDependencies {
  fetchFn?: typeof fetch;
  openBrowser?: (url: string) => Promise<unknown>;
  callbackFactory?: typeof listenForAuthorizationCallback;
}

function sdkMetadata(discovery: Awaited<ReturnType<typeof discoverOAuth>>): AuthorizationServerMetadata {
  return {
    issuer: discovery.issuer,
    authorization_endpoint: discovery.authorizationEndpoint,
    token_endpoint: discovery.tokenEndpoint,
    response_types_supported: ["code"],
    revocation_endpoint: discovery.revocationEndpoint,
    registration_endpoint: discovery.registrationEndpoint,
    scopes_supported: discovery.scopes,
    code_challenge_methods_supported: discovery.codeChallengeMethods,
  };
}

export class AuthenticationService {
  constructor(
    private readonly configStore: ConfigurationStore,
    private readonly tokenStore: TokenStore,
  ) {}

  async configureClient(configuration: Configuration, clientId: string, clientSecret?: string): Promise<Configuration> {
    const discovery = await discoverOAuth(configuration.serverUrl);
    const updated = { ...configuration, clientId, clientSecret: clientSecret || undefined };
    const identity = this.identity(updated, discovery.issuer, clientId);
    if (clientSecret) await this.tokenStore.saveClientSecret(identity, clientSecret);
    await this.configStore.save(updated);
    return updated;
  }

  async login(configuration: Configuration, dependencies: LoginDependencies = {}): Promise<OrganizationIdentity[]> {
    const fetchFn = dependencies.fetchFn ?? fetch;
    const discovery = await discoverOAuth(configuration.serverUrl, fetchFn);
    const configuredRedirectUri = configuration.redirectUri;
    let redirectUrl: string;
    let callbackResult: Promise<{ code: string; state: string | null }>;
    let closeCallback: () => Promise<void>;
    if (configuredRedirectUri) {
      const parsed = new URL(configuredRedirectUri);
      const port = parsed.port ? Number(parsed.port) : parsed.protocol === "https:" ? 443 : 80;
      const callback = await (dependencies.callbackFactory ?? (() => listenForAuthorizationCallback(180_000, port)))();
      redirectUrl = configuredRedirectUri;
      callbackResult = callback.result;
      closeCallback = callback.close;
    } else {
      const callback = await (dependencies.callbackFactory ?? listenForAuthorizationCallback)();
      redirectUrl = callback.redirectUrl;
      callbackResult = callback.result;
      closeCallback = callback.close;
    }
    try {
      const configuredId = process.env.MULTITRACKS_MCP_CLIENT_ID ?? configuration.clientId ?? DEFAULT_MULTITRACKS_CLIENT_ID;
      const clientMetadata: OAuthClientMetadata = {
        client_name: "StagePilot MultiTracks Cues",
        redirect_uris: [redirectUrl],
        grant_types: ["authorization_code", "refresh_token"],
        response_types: ["code"],
        token_endpoint_auth_method: await this.clientSecret(configuration, discovery.issuer, configuredId)
          ? "client_secret_basic"
          : "none",
        scope: DEFAULT_SCOPE,
      };
      let client: OAuthClientInformationMixed;
      if (configuredId) {
        client = {
          client_id: configuredId,
          client_secret: await this.clientSecret(configuration, discovery.issuer, configuredId),
        };
      } else if (discovery.registrationEndpoint) {
        client = await registerClient(discovery.issuer, {
          metadata: sdkMetadata(discovery),
          clientMetadata,
          scope: DEFAULT_SCOPE,
          fetchFn,
        });
      } else {
        throw new AuthenticationError(
          "MultiTracks requires a registered standalone OAuth client. Set MULTITRACKS_MCP_CLIENT_ID after MultiTracks issues one; dynamic registration is not advertised.",
          EXIT.AUTH,
        );
      }
      if (client.client_secret) {
        await this.tokenStore.saveClientSecret(
          this.identity(configuration, discovery.issuer, client.client_id),
          client.client_secret,
        );
      }
      const { verifier, challenge } = createPkce();
      const state = createState();
      const authorizationUrl = new URL(discovery.authorizationEndpoint);
      authorizationUrl.search = new URLSearchParams({
        response_type: "code",
        client_id: client.client_id,
        redirect_uri: redirectUrl,
        scope: DEFAULT_SCOPE,
        state,
        code_challenge: challenge,
        code_challenge_method: "S256",
        resource: discovery.protectedResource,
      }).toString();
      await (dependencies.openBrowser ?? ((url) => open(url)))(authorizationUrl.toString());
      const authorization = await callbackResult;
      verifyState(state, authorization.state);
      const tokens = await exchangeAuthorization(discovery.issuer, {
        metadata: sdkMetadata(discovery),
        clientInformation: client,
        authorizationCode: authorization.code,
        codeVerifier: verifier,
        redirectUri: redirectUrl,
        resource: new URL(discovery.protectedResource),
        fetchFn,
      });
      await this.tokenStore.save(this.identity(configuration, discovery.issuer, client.client_id), tokens);
      await this.configStore.save({ ...configuration, clientId: client.client_id });
      return this.discoverOrganizations(discovery.userInfoEndpoint, tokens.access_token, fetchFn);
    } finally {
      await closeCallback();
    }
  }

  async selectOrganization(configuration: Configuration, organization: OrganizationIdentity): Promise<Configuration> {
    const discovery = await discoverOAuth(configuration.serverUrl);
    const clientId = process.env.MULTITRACKS_MCP_CLIENT_ID ?? configuration.clientId;
    if (!clientId) throw new AuthenticationError("OAuth client registration is required.", EXIT.AUTH);
    const source = this.identity({ ...configuration, organization: undefined }, discovery.issuer, clientId);
    const tokens = await this.tokenStore.load(source);
    if (!tokens) throw new AuthenticationError("No unassigned login is available.", EXIT.AUTH);
    const selected = { ...configuration, clientId, organization };
    await this.tokenStore.save(this.identity(selected, discovery.issuer, clientId), tokens);
    await this.tokenStore.delete(source);
    await this.configStore.save(selected);
    return selected;
  }

  async tokens(configuration: Configuration, fetchFn: typeof fetch = fetch): Promise<OAuthTokens> {
    const discovery = await discoverOAuth(configuration.serverUrl, fetchFn);
    const clientId = process.env.MULTITRACKS_MCP_CLIENT_ID ?? configuration.clientId;
    if (!clientId) throw new AuthenticationError("OAuth client registration is required.", EXIT.AUTH);
    const identity = this.identity(configuration, discovery.issuer, clientId);
    const stored = await this.tokenStore.load(identity);
    if (!stored) throw new AuthenticationError("Not authenticated. Run 'stagepilot-cues auth login'.", EXIT.AUTH);
    if (!this.tokenStore.isExpired(stored)) return stored;
    if (!stored.refresh_token) {
      throw new AuthenticationError("The access token expired and no refresh token is available. Log in again.", EXIT.AUTH);
    }
    const refreshed = await refreshAuthorization(discovery.issuer, {
      metadata: sdkMetadata(discovery),
      clientInformation: {
        client_id: clientId,
        client_secret: await this.clientSecret(configuration, discovery.issuer, clientId),
      },
      refreshToken: stored.refresh_token,
      resource: new URL(discovery.protectedResource),
      fetchFn,
    });
    return this.tokenStore.save(identity, { ...refreshed, refresh_token: refreshed.refresh_token ?? stored.refresh_token });
  }

  async status(configuration: Configuration): Promise<{
    authenticated: boolean;
    expiresAt?: string;
    organization?: Configuration["organization"];
    serverUrl: string;
    registrationMode: "static" | "dynamic" | "missing";
  }> {
    const discovery = await discoverOAuth(configuration.serverUrl);
    const clientId = process.env.MULTITRACKS_MCP_CLIENT_ID ?? configuration.clientId;
    if (!clientId) {
      return {
        authenticated: false,
        organization: configuration.organization,
        serverUrl: configuration.serverUrl,
        registrationMode: discovery.registrationEndpoint ? "dynamic" : "missing",
      };
    }
    const stored = await this.tokenStore.load(this.identity(configuration, discovery.issuer, clientId));
    const expiry = stored && this.tokenStore.expiresAt(stored);
    return {
      authenticated: Boolean(stored),
      expiresAt: expiry ? new Date(expiry).toISOString() : undefined,
      organization: configuration.organization,
      serverUrl: configuration.serverUrl,
      registrationMode: "static",
    };
  }

  async logout(configuration: Configuration, fetchFn: typeof fetch = fetch): Promise<void> {
    const discovery = await discoverOAuth(configuration.serverUrl, fetchFn);
    const clientId = process.env.MULTITRACKS_MCP_CLIENT_ID ?? configuration.clientId;
    if (!clientId) return;
    const identity = this.identity(configuration, discovery.issuer, clientId);
    const stored = await this.tokenStore.load(identity);
    if (stored && discovery.revocationEndpoint) {
      for (const token of [stored.refresh_token, stored.access_token].filter(Boolean) as string[]) {
        const body = new URLSearchParams({ token, client_id: clientId });
        const secret = await this.clientSecret(configuration, discovery.issuer, clientId);
        if (secret) body.set("client_secret", secret);
        await fetchFn(discovery.revocationEndpoint, {
          method: "POST",
          headers: { "Content-Type": "application/x-www-form-urlencoded" },
          body,
        }).catch(() => undefined);
      }
    }
    await this.tokenStore.delete(identity);
    await this.configStore.save({ ...configuration, organization: undefined });
  }

  private identity(configuration: Configuration, issuer: string, clientId: string): TokenStoreIdentity {
    return {
      serverOrigin: configuration.serverUrl,
      issuer,
      clientId,
      organizationId: configuration.organization?.id,
    };
  }

  private async clientSecret(configuration: Configuration, issuer: string, clientId?: string): Promise<string | undefined> {
    if (!clientId) return process.env.MULTITRACKS_MCP_CLIENT_SECRET ?? configuration.clientSecret ?? DEFAULT_MULTITRACKS_CLIENT_SECRET;
    return process.env.MULTITRACKS_MCP_CLIENT_SECRET
      ?? configuration.clientSecret
      ?? DEFAULT_MULTITRACKS_CLIENT_SECRET
      ?? await this.tokenStore.loadClientSecret(this.identity(configuration, issuer, clientId));
  }

  private async discoverOrganizations(endpoint: string | undefined, accessToken: string, fetchFn: typeof fetch): Promise<OrganizationIdentity[]> {
    if (!endpoint) return [];
    const response = await fetchFn(endpoint, { headers: { Authorization: `Bearer ${accessToken}`, Accept: "application/json" } });
    if (!response.ok) return [];
    const value = await response.json() as Record<string, unknown>;
    const candidates = Array.isArray(value.organizations) ? value.organizations : value.organization ? [value.organization] : [];
    return candidates.flatMap((candidate) => {
      if (!candidate || typeof candidate !== "object") return [];
      const record = candidate as Record<string, unknown>;
      const id = record.id ?? record.organization_id;
      const name = record.name ?? record.organization_name;
      return (typeof id === "string" || typeof id === "number") && typeof name === "string"
        ? [{ id: String(id), name }]
        : [];
    });
  }
}

