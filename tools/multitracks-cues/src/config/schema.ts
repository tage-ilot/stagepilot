import { z } from "zod";

export const configurationSchema = z.object({
  serverUrl: z.url().default("https://mcp.multitracks.com/mcp"),
  clientId: z.string().min(1).optional(),
  clientSecret: z.string().min(1).optional(),
  redirectUri: z.string().min(1).optional(),
  organization: z
    .object({ id: z.string().min(1), name: z.string().min(1) })
    .optional(),
  midiBus: z
    .object({ id: z.string().min(1), name: z.string().optional(), type: z.string().optional() })
    .optional(),
  channel: z.number().int().min(1).max(16).default(1),
  note: z.number().int().min(0).max(127).default(112),
  reportDirectory: z.string().min(1).default("reports"),
  color: z.boolean().default(true),
  setlistNameFilter: z.string().min(1).optional(),
  defaultDateWindowDays: z.number().int().min(1).max(365).default(30),
  insecureDevelopmentTokenStore: z.boolean().default(false),
});

export type Configuration = z.infer<typeof configurationSchema>;
export const defaultConfiguration = configurationSchema.parse({});

