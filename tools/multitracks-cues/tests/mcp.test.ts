import { describe, expect, it, vi } from "vitest";
import { SafeMcpClient } from "../src/mcp/client.js";
import type { ToolDefinition, ToolTransport } from "../src/mcp/types.js";

class MockTransport implements ToolTransport {
  calls: Array<{ name: string; args: Record<string, unknown> }> = [];
  failures: Error[] = [];
  constructor(readonly tools: ToolDefinition[]) {}
  async connect(): Promise<void> {}
  async close(): Promise<void> {}
  async listTools(): Promise<ToolDefinition[]> { return this.tools; }
  async callTool(name: string, args: Record<string, unknown>): Promise<unknown> {
    this.calls.push({ name, args });
    const failure = this.failures.shift();
    if (failure) throw failure;
    return { structuredContent: { ok: true } };
  }
}

const tool = (name: string, required: string[] = []): ToolDefinition => ({ name, inputSchema: { type: "object", properties: Object.fromEntries(required.map((key) => [key, { type: "string" }])), required, additionalProperties: false } });

describe("safe MCP client", () => {
  it("connects, lists tools, and validates arguments", async () => {
    const transport = new MockTransport([tool("setlistGet", ["id"])]);
    const client = new SafeMcpClient(transport);
    await client.connect();
    await expect(client.call("setlistGet", { id: "123" }, "read")).resolves.toEqual({ ok: true });
    await expect(client.call("setlistGet", {}, "read")).rejects.toThrow(/advertised schema/);
  });

  it("blocks writes in dry-run and permanently blocks destructive tools", async () => {
    const transport = new MockTransport([tool("libraryMidiEventCreate")]);
    const client = new SafeMcpClient(transport);
    await client.connect();
    await expect(client.call("libraryMidiEventCreate", {}, "read")).rejects.toThrow(/blocked outside explicit apply/);
    await expect(client.call("libraryMidiEventDelete", {}, "apply")).rejects.toThrow(/permanently blocked/);
    expect(transport.calls).toHaveLength(0);
  });

  it("reports missing capabilities", async () => {
    const client = new SafeMcpClient(new MockTransport([tool("setlistsList")]));
    await client.connect();
    expect(client.validateCapabilities().missing).toContain("midiBusesList");
  });

  it("retries transient reads with bounded attempts but never retries writes", async () => {
    vi.useFakeTimers();
    const readTransport = new MockTransport([tool("setlistsList")]);
    readTransport.failures.push(new Error("HTTP 503 temporary"));
    const readClient = new SafeMcpClient(readTransport, () => 0);
    await readClient.connect();
    const result = readClient.call("setlistsList", {}, "read");
    await vi.runAllTimersAsync();
    await expect(result).resolves.toEqual({ ok: true });
    expect(readTransport.calls).toHaveLength(2);
    vi.useRealTimers();

    const writeTransport = new MockTransport([tool("libraryMidiEventCreate")]);
    writeTransport.failures.push(new Error("HTTP 503 temporary"));
    const writeClient = new SafeMcpClient(writeTransport);
    await writeClient.connect();
    await expect(writeClient.call("libraryMidiEventCreate", {}, "apply")).rejects.toThrow(/503/);
    expect(writeTransport.calls).toHaveLength(1);
  });

  it("surfaces tool error results without leaking raw structures", async () => {
    const transport = new MockTransport([tool("setlistsList")]);
    transport.callTool = async () => ({ isError: true, content: [{ type: "text", text: "Unauthorized" }] });
    const client = new SafeMcpClient(transport);
    await client.connect();
    await expect(client.call("setlistsList", {}, "read")).rejects.toThrow("Unauthorized");
  });
});

