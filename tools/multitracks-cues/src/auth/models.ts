import { z } from "zod";

export const storedTokensSchema = z.object({
  access_token: z.string().min(1),
  token_type: z.string().default("Bearer"),
  expires_in: z.number().positive().optional(),
  refresh_token: z.string().min(1).optional(),
  scope: z.string().optional(),
  id_token: z.string().optional(),
  obtainedAt: z.number().int().positive(),
});

export type StoredTokens = z.infer<typeof storedTokensSchema>;

export interface OAuthDiscovery {
  issuer: string;
  authorizationEndpoint: string;
  tokenEndpoint: string;
  revocationEndpoint?: string;
  userInfoEndpoint?: string;
  registrationEndpoint?: string;
  scopes: string[];
  codeChallengeMethods: string[];
  protectedResource: string;
  authorizationServers: string[];
}

export interface OrganizationIdentity {
  id: string;
  name: string;
}

