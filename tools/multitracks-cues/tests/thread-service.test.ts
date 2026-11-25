import { describe, expect, it, vi } from "vitest";
import { AgentThreadService } from "../src/agent/thread-service.js";

function mockOpenAI() {
  return {
    responses: {
      create: vi.fn(async () => ({
        id: "resp-1",
        output: [
          { type: "message", role: "assistant", content: [{ type: "output_text", text: "Hello!" }] },
        ],
      })),
    },
    models: {
      list: vi.fn(async () => ({
        data: [{ id: "gpt-4o" }, { id: "gpt-4o-mini" }],
      })),
    },
  } as never;
}

describe("AgentThreadService", () => {
  it("lists available models", async () => {
    const openai = mockOpenAI();
    const service = new AgentThreadService(openai, "gpt-4o");
    const models = await service.models();
    expect(models).toHaveLength(2);
    expect(models[0]!.id).toBe("gpt-4o");
  });

  it("starts a new thread", async () => {
    const openai = mockOpenAI();
    const service = new AgentThreadService(openai, "gpt-4o");
    const threadId = await service.start();
    expect(threadId).toBeDefined();
    expect(typeof threadId).toBe("string");
  });

  it("starts with a specific model", async () => {
    const openai = mockOpenAI();
    const service = new AgentThreadService(openai, "gpt-4o");
    const threadId = await service.start("gpt-4o-mini");
    expect(threadId).toBeDefined();
  });

  it("rejects unavailable model", async () => {
    const openai = mockOpenAI();
    const service = new AgentThreadService(openai, "gpt-4o");
    await expect(service.start("nonexistent-model")).rejects.toThrow("unavailable");
  });

  it("sends a turn and receives a response", async () => {
    const openai = mockOpenAI();
    const service = new AgentThreadService(openai, "gpt-4o");
    const threadId = await service.start();
    const answer = await service.turn(threadId, "Hello");
    expect(answer).toBe("Hello!");
  });

  it("rejects turn for unknown thread", async () => {
    const openai = mockOpenAI();
    const service = new AgentThreadService(openai, "gpt-4o");
    await expect(service.turn("unknown", "Hello")).rejects.toThrow("not active");
  });

  it("deletes a thread", async () => {
    const openai = mockOpenAI();
    const service = new AgentThreadService(openai, "gpt-4o");
    const threadId = await service.start();
    await service.delete(threadId);
    await expect(service.turn(threadId, "Hello")).rejects.toThrow("not active");
  });

  it("interrupt is a no-op", async () => {
    const openai = mockOpenAI();
    const service = new AgentThreadService(openai, "gpt-4o");
    await expect(service.interrupt()).resolves.toBeUndefined();
  });
});

