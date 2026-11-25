import type { OpenAIClient, OpenAITool } from "./client.js";
import type { StagePilotAgentTools } from "../agent/tools.js";
import type { DynamicToolSpec } from "../agent/tools.js";
import { STAGEPILOT_AGENT_INSTRUCTIONS } from "../agent/instructions.js";

const MAX_AGENT_STEPS = 10;

export class AgentService {
  readonly history: Array<{ role: string; content: string }> = [];
  private readonly tools: OpenAITool[];

  constructor(
    private client: OpenAIClient,
    private model: string,
    private agentTools: StagePilotAgentTools,
  ) {
    this.tools = toOpenAITools(agentTools.definitions());
  }

  loadHistory(history: Array<{ role: string; content: string }>): void {
    this.history.length = 0;
    this.history.push(...history);
  }

  getHistory(): Array<{ role: string; content: string }> {
    return [...this.history];
  }

  async turn(userMessage: string, onDelta?: (delta: string) => void): Promise<string> {
    this.history.push({ role: "user", content: userMessage });

    const input = this.history.map((m) => ({
      type: "message" as const,
      role: m.role as "user" | "assistant",
      content: m.content,
    }));

    let lastResponseId: string | undefined;
    let finalAnswer = "";

    for (let step = 0; step < MAX_AGENT_STEPS; step++) {
      const body: Record<string, unknown> = {
        model: this.model,
        instructions: STAGEPILOT_AGENT_INSTRUCTIONS,
        tools: this.tools.length > 0 ? this.tools : undefined,
      };

      if (lastResponseId) {
        body.previous_response_id = lastResponseId;
        body.input = []; // empty — continue from previous
      } else {
        body.input = input;
      }

      const response = await this.client.responses.create(body);
      lastResponseId = response.id;

      const output = response.output ?? [];
      let assistantText = "";
      const toolCalls: Array<{ callId: string; name: string; args: string }> = [];

      for (const item of output) {
        if (item.type === "message" && item.role === "assistant") {
          for (const content of item.content ?? []) {
            if (content.type === "output_text") {
              assistantText += content.text;
            }
          }
        }
        if (item.type === "function_call") {
          toolCalls.push({ callId: item.call_id, name: item.name, args: item.arguments });
        }
      }

      if (toolCalls.length === 0) {
        this.history.push({ role: "assistant", content: assistantText });
        finalAnswer = assistantText;
        if (onDelta) onDelta(finalAnswer);
        break;
      }

      // Execute each tool call and submit results
      for (const tc of toolCalls) {
        let result: unknown;
        try {
          const args = JSON.parse(tc.args);
          result = await this.agentTools.call(tc.name, args);
        } catch (error) {
          result = { error: error instanceof Error ? error.message : String(error) };
        }
        const toolResponse = await this.client.responses.create({
          model: this.model,
          previous_response_id: lastResponseId,
          tools: this.tools,
          input: [{ type: "function_call_output", call_id: tc.callId, output: JSON.stringify(result) }],
        });
        lastResponseId = toolResponse.id;
      }
    }

    return finalAnswer;
  }
}

function toOpenAITools(specs: DynamicToolSpec[]): OpenAITool[] {
  return specs.map((spec) => ({
    type: "function" as const,
    name: spec.name,
    description: spec.description,
    parameters: spec.inputSchema,
    strict: false,
  }));
}

