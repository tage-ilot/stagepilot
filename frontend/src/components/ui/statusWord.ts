import type { Tone } from "./types";

export type StatusWord = "Connected" | "Connecting" | "Not connected" | "Error";

export const statusTone: Record<StatusWord, Tone> = {
  Connected: "success",
  Connecting: "warning",
  "Not connected": "idle",
  Error: "danger",
};

/** Raw API enum (plus the panel `loading` placeholder) to the one word people read. The enum is never printed. */
export type ConnectionState = "connected" | "connecting" | "disconnected" | "error" | "loading";

export const connectionWord: Record<ConnectionState, StatusWord> = {
  connected: "Connected",
  connecting: "Connecting",
  disconnected: "Not connected",
  error: "Error",
  loading: "Not connected",
};
