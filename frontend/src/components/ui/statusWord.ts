import type { Tone } from "./types";

export type StatusWord = "Connected" | "Connecting" | "Disconnected" | "Error";

export const statusTone: Record<StatusWord, Tone> = {
  Connected: "success",
  Connecting: "warning",
  "Disconnected": "idle",
  Error: "danger",
};

/** Raw API enum (plus the panel `loading` placeholder) to the one word people read. The enum is never printed. */
export type ConnectionState = "connected" | "connecting" | "disconnected" | "error" | "loading";

export const connectionWord: Record<ConnectionState, StatusWord> = {
  connected: "Connected",
  connecting: "Connecting",
  disconnected: "Disconnected",
  error: "Error",
  loading: "Disconnected",
};
