import type { OpenAIClient } from "../ai/client.js";
import { randomUUID } from "node:crypto";
import { AgentService } from "../ai/agent-service.js";
import { StagePilotAgentTools } from "./tools.js";
import { AGENT_PROMPT_VERSION } from "./instructions.js";
import { SessionStore } from "./sessions.js";
import { listModels } from "../ai/models.js";

export class AgentThreadService {
  readonly sessions = new SessionStore();
  private readonly services = new Map<string, AgentService>();
  private readonly defaultModel: string;

  constructor(
    private client: OpenAIClient,
    defaultModel: string | undefined,
    readonly tools = new StagePilotAgentTools(),
  ) {
    this.defaultModel = defaultModel ?? "gpt-5.6-luna";
  }

  async models(): Promise<Array<{ id: string; isDefault?: boolean }>> {
    const models = await listModels(this.client);
    return models.map((m) => ({ id: m.id, isDefault: m.id === this.defaultModel }));
  }

  async start(model?: string): Promise<string> {
    if (model) {
      const models = await this.models();
      if (!models.some((entry) => entry.id === model)) throw new Error(`Model '${model}' is unavailable for this account.`);
    }
    const threadId = randomUUID();
    const service = new AgentService(this.client, model ?? this.defaultModel, this.tools);
    this.services.set(threadId, service);
    await this.sessions.remember(threadId);
    return threadId;
  }

  async resume(threadId: string, model?: string): Promise<string> {
    const existing = await this.sessions.get(threadId);
    if (!existing) throw new Error(`Session '${threadId}' not found.`);
    if (existing.promptVersion !== AGENT_PROMPT_VERSION) {
      process.stdout.write(`Warning: Thread was created with prompt v${existing.promptVersion}; current is v${AGENT_PROMPT_VERSION}. Consider starting a new session.\n`);
    }
    const service = new AgentService(this.client, model ?? this.defaultModel, this.tools);
    if (existing.history && existing.history.length > 0) {
      service.loadHistory(existing.history);
    }
    this.services.set(threadId, service);
    return threadId;
  }

  async delete(threadId: string): Promise<void> {
    this.services.delete(threadId);
    await this.sessions.remove(threadId);
  }

  async turn(threadId: string, text: string, onDelta?: (delta: string) => void): Promise<string> {
    const service = this.services.get(threadId);
    if (!service) throw new Error(`Session '${threadId}' is not active. Resume it first.`);
    const answer = await service.turn(text, onDelta);
    await this.sessions.remember(threadId, undefined, service.getHistory());
    return answer;
  }

  async interrupt(): Promise<void> {
    // No-op: OpenAI API calls are not interruptible in the same way as Codex App Server
  }
}

