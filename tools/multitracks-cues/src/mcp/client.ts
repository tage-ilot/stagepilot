import AjvModule from "ajv/dist/2020.js";
import type { ErrorObject, ValidateFunction } from "ajv";
import {
  ALLOWED_TOOLS,
  ALLOWED_WRITE_TOOLS,
  EXIT,
  FORBIDDEN_TOOLS,
  REQUIRED_READ_TOOLS,
  WRITE_TOOLS,
} from "../constants.js";
import { CapabilityError, SchemaError } from "../errors.js";
import { redact } from "../security/redact.js";
import type { InvocationMode, ToolDefinition, ToolTransport } from "./types.js";

const transient = /\b(?:429|502|503|504)\b|rate.?limit|temporar|ECONNRESET|ETIMEDOUT|fetch failed/i;

interface AjvInstance {
  compile(schema: Record<string, unknown>): ValidateFunction;
  errorsText(errors?: ErrorObject[] | null): string;
}

const Ajv = AjvModule as unknown as new (options: Record<string, unknown>) => AjvInstance;

export function normalizeToolResult(result: unknown): unknown {
  if (!result || typeof result !== "object") return result;
  const record = result as Record<string, unknown>;
  if (record.isError) throw new Error(extractText(record.content) ?? "MultiTracks tool returned an error.");
  if (record.structuredContent !== undefined) return record.structuredContent;
  const text = extractText(record.content);
  if (text === undefined) return result;
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return text;
  }
}

function extractText(content: unknown): string | undefined {
  if (!Array.isArray(content)) return undefined;
  return content
    .filter((item): item is { type: "text"; text: string } =>
      Boolean(item && typeof item === "object" && (item as { type?: unknown }).type === "text"),
    )
    .map((item) => item.text)
    .join("\n");
}

const wait = (milliseconds: number): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, milliseconds));

export class SafeMcpClient {
  private readonly ajv = new Ajv({ allErrors: true, strict: false });
  private readonly tools = new Map<string, ToolDefinition>();
  private readonly validators = new Map<string, ValidateFunction>();

  constructor(
    private readonly transport: ToolTransport,
    private readonly random: () => number = Math.random,
  ) {}

  async connect(): Promise<void> {
    await this.transport.connect();
    await this.refreshTools();
  }

  async close(): Promise<void> {
    await this.transport.close();
  }

  async refreshTools(): Promise<ToolDefinition[]> {
    const listed = await this.transport.listTools();
    this.tools.clear();
    this.validators.clear();
    for (const tool of listed) {
      this.tools.set(tool.name, tool);
      this.validators.set(tool.name, this.ajv.compile(tool.inputSchema));
    }
    return listed;
  }

  listTools(): ToolDefinition[] {
    return [...this.tools.values()];
  }

  sanitizedSchemas(): unknown {
    return redact(this.listTools().map(({ name, description, inputSchema }) => ({ name, description, inputSchema })));
  }

  validateCapabilities(): { present: string[]; missing: string[] } {
    const required = [...REQUIRED_READ_TOOLS, ...ALLOWED_WRITE_TOOLS];
    const present = required.filter((name) => this.tools.has(name));
    const missing = required.filter((name) => !this.tools.has(name));
    return { present: [...present], missing: [...missing] };
  }

  schema(name: string): Record<string, unknown> {
    const tool = this.tools.get(name);
    if (!tool) throw new CapabilityError(`Required MultiTracks tool '${name}' is unavailable.`, EXIT.CAPABILITY);
    return tool.inputSchema;
  }

  async call(name: string, args: Record<string, unknown>, mode: InvocationMode): Promise<unknown> {
    if (FORBIDDEN_TOOLS.includes(name as (typeof FORBIDDEN_TOOLS)[number])) {
      throw new CapabilityError(`Destructive tool '${name}' is permanently blocked.`, EXIT.CAPABILITY);
    }
    if (!ALLOWED_TOOLS.has(name)) {
      throw new CapabilityError(`Tool '${name}' is outside the stagepilot-cues allowlist.`, EXIT.CAPABILITY);
    }
    if (WRITE_TOOLS.has(name) && mode !== "apply") {
      throw new CapabilityError(`Write tool '${name}' is blocked outside explicit apply mode.`, EXIT.CAPABILITY);
    }
    const validator = this.validators.get(name);
    if (!validator) throw new CapabilityError(`Tool '${name}' was not advertised by the server.`, EXIT.CAPABILITY);
    if (!validator(args)) {
      throw new SchemaError(
        `Arguments for '${name}' do not match the advertised schema: ${this.ajv.errorsText(validator.errors)}.`,
        EXIT.SCHEMA,
      );
    }
    const attempts = WRITE_TOOLS.has(name) ? 1 : 4;
    let lastError: unknown;
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      try {
        return normalizeToolResult(await this.transport.callTool(name, args));
      } catch (error) {
        lastError = error;
        if (!transient.test(error instanceof Error ? error.message : String(error)) || attempt + 1 >= attempts) {
          throw error;
        }
        await wait(Math.min(4_000, 250 * 2 ** attempt) + Math.floor(this.random() * 100));
      }
    }
    throw lastError;
  }
}

