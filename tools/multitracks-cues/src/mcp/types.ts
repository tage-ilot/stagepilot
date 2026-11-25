export interface ToolDefinition {
  name: string;
  description?: string;
  inputSchema: Record<string, unknown>;
}

export interface ToolTransport {
  connect(): Promise<void>;
  close(): Promise<void>;
  listTools(): Promise<ToolDefinition[]>;
  callTool(name: string, args: Record<string, unknown>): Promise<unknown>;
}

export type InvocationMode = "read" | "apply";

