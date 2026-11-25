import path from "node:path";
import { createOpenAIOAuthTransport } from "@openai-oauth/core";
import { openaiCredentials } from "@openai-oauth/local";
import { codexHome } from "../codex/executable.js";

export interface OpenAIResponse {
  id: string;
  output: OpenAIResponseOutputItem[];
}

export type OpenAIResponseOutputItem =
  | OpenAIResponseMessage
  | OpenAIResponseFunctionCall;

export interface OpenAIResponseMessage {
  id: string;
  type: "message";
  role: "assistant";
  content: Array<{ type: "output_text"; text: string }>;
}

export interface OpenAIResponseFunctionCall {
  id: string;
  type: "function_call";
  call_id: string;
  name: string;
  arguments: string;
}

export interface OpenAITool {
  type: "function";
  name: string;
  description: string;
  parameters: Record<string, unknown>;
  strict: boolean;
}

export interface OpenAIModel {
  id: string;
}

export interface OpenAIClient {
  responses: {
    create(body: Record<string, unknown>): Promise<OpenAIResponse>;
  };
  models: {
    list(): Promise<{ data: OpenAIModel[] }>;
  };
}

export function createOpenAIClient(options: { authFilePath?: string; baseURL?: string } = {}): OpenAIClient {
  const authFilePath = options.authFilePath ?? path.join(codexHome(), "auth.json");
  const credentials = openaiCredentials({ authFilePath });

  const transport = createOpenAIOAuthTransport({
    auth: () => credentials.getSession(),
  });

  const baseURL = options.baseURL ?? "http://127.0.0.1:10531/v1";

  async function request<T>(endpoint: string, body: Record<string, unknown>): Promise<T> {
    const res = await transport.fetch(`${baseURL}${endpoint}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      const text = await res.text();
      throw new Error(`API error ${res.status}: ${text}`);
    }
    return res.json() as Promise<T>;
  }

  return {
    responses: {
      create: (body) => request("/responses", body),
    },
    models: {
      list: () => request("/models", {}),
    },
  };
}

