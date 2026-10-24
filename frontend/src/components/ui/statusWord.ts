import type { Tone } from "./types";

export type StatusWord = "Connected" | "Connecting" | "Disconnected" | "Error" | "Ready";

export const statusTone: Record<StatusWord, Tone> = {
  Connected: "success",
  Connecting: "warning",
  "Disconnected": "idle",
  Error: "danger",
  // Output open, delivery not proven (UDP/OSC before the first cue): not green, not an alarm.
  Ready: "info",
};

/** Raw API enum (plus the panel `loading` placeholder) to the one word people read. The enum is never printed. */
export type ConnectionState = "connected" | "connecting" | "disconnected" | "error" | "ready" | "loading";

export const connectionWord: Record<ConnectionState, StatusWord> = {
  connected: "Connected",
  connecting: "Connecting",
  disconnected: "Disconnected",
  error: "Error",
  ready: "Ready",
  loading: "Disconnected",
};
