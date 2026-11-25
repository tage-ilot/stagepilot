import type { OpenAIClient } from "./client.js";

export interface ModelInfo {
  id: string;
  isDefault?: boolean;
}

export async function listModels(client: OpenAIClient): Promise<ModelInfo[]> {
  const response = await client.models.list();
  return response.data.map((m) => ({ id: m.id }));
}

