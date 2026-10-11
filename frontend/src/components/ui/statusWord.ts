import type { Tone } from "./types";

export type StatusWord = "Connected" | "Connecting" | "Not connected" | "Error";

export const statusTone: Record<StatusWord, Tone> = {
  Connected: "success",
  Connecting: "warning",
  "Not connected": "idle",
  Error: "danger",
};
