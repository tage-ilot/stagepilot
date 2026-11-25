import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { CreateMessageRequestSchema, ElicitRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import type { OAuthTokens } from "@modelcontextprotocol/sdk/shared/auth.js";
import { APP_NAME, APP_VERSION } from "../constants.js";
import type { ToolDefinition, ToolTransport } from "./types.js";

export class SdkToolTransport implements ToolTransport {
  private readonly client: Client;
  private transport?: StreamableHTTPClientTransport;

  constructor(
    private readonly serverUrl: string,
    private readonly tokens: OAuthTokens,
    private readonly fetchFn: typeof fetch = fetch,
  ) {
    this.client = new Client({ name: APP_NAME, version: APP_VERSION }, { capabilities: { elicitation: {} } });
    this.client.setRequestHandler(CreateMessageRequestSchema, async () => {
      throw new Error("Sampling is not supported: stagepilot-cues has no LLM or AI runtime.");
    });
    this.client.setRequestHandler(ElicitRequestSchema, async () => ({ action: "decline" as const }));
  }

  async connect(): Promise<void> {
    this.transport = new StreamableHTTPClientTransport(new URL(this.serverUrl), {
      requestInit: { headers: { Authorization: `Bearer ${this.tokens.access_token}` } },
      fetch: this.fetchFn,
      reconnectionOptions: {
        initialReconnectionDelay: 500,
        maxReconnectionDelay: 5_000,
        reconnectionDelayGrowFactor: 2,
        maxRetries: 3,
      },
    });
    await this.client.connect(this.transport);
  }

  async close(): Promise<void> {
    if (this.transport) {
      await this.transport.terminateSession().catch(() => undefined);
    }
    await this.client.close();
  }

  async listTools(): Promise<ToolDefinition[]> {
    const result = await this.client.listTools();
    return result.tools.map((tool) => ({
      name: tool.name,
      description: tool.description,
      inputSchema: tool.inputSchema as Record<string, unknown>,
    }));
  }

  async callTool(name: string, args: Record<string, unknown>): Promise<unknown> {
    return this.client.callTool({ name, arguments: args });
  }
}

