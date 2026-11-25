import type { OpenAIClient, OpenAIResponse, OpenAIModel } from "../ai/client.js";
import { blockingAsk, newConversation, shutdownBrowser, type AskResult } from "./chatgpt-client.js";

export interface ChatGPTWebClientOptions {
  timeoutMinutes?: number;
}

export function createChatGPTWebClient(options: ChatGPTWebClientOptions = {}): OpenAIClient {
  const timeoutMinutes = options.timeoutMinutes ?? 5;

  return {
    responses: {
      create: async (body: Record<string, unknown>): Promise<OpenAIResponse> => {
        const input = body.input as Array<{ type: string; role?: string; content?: string }>;
        const instructions = body.instructions as string | undefined;

        // Extract the user message from input
        const userMessage = extractUserMessage(input, instructions);
        if (!userMessage) {
          throw new Error("No user message found in input");
        }

        // Send to ChatGPT via Playwright
        const result: AskResult = await blockingAsk(userMessage, timeoutMinutes);

        if (result.error) {
          throw new Error(result.error);
        }

        // Convert to OpenAIResponse format
        return {
          id: `chatgpt-web-${Date.now()}`,
          output: [
            {
              id: `msg-${Date.now()}`,
              type: "message",
              role: "assistant",
              content: [{ type: "output_text", text: result.response }],
            },
          ],
        };
      },
    },
    models: {
      list: async (): Promise<{ data: OpenAIModel[] }> => {
        return {
          data: [
            { id: "chatgpt-web" },
          ],
        };
      },
    },
  };
}

function extractUserMessage(
  input: Array<{ type: string; role?: string; content?: string }>,
  instructions?: string,
): string | null {
  // Build a prompt that includes instructions if provided
  let prompt = "";

  // Add system instructions as context
  if (instructions) {
    prompt += `[System Instructions]\n${instructions}\n\n`;
  }

  // Find the last user message
  for (const item of input) {
    if (item.type === "message" && item.role === "user" && item.content) {
      prompt += item.content;
    }
  }

  return prompt || null;
}

export async function shutdownChatGPTWebClient(): Promise<void> {
  await shutdownBrowser();
}

export { newConversation };

