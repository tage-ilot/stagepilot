import type { OscArgument, OscArgumentType, OscLightingCue } from "../types";

/** Returns an error message, or null when the address is a valid OSC address. */
export const oscAddressError = (address: string): string | null => {
  const value = address.trim();
  if (!value) return "Enter an OSC address.";
  if (!value.startsWith("/")) return "Start the address with /.";
  if (/\s/.test(value)) return "Remove spaces from the address.";
  if (value.length > 256) return "Use 256 characters or fewer.";
  return null;
};

export const oscPortError = (port: string): string | null => {
  const value = Number(port);
  return port.trim() !== "" && Number.isInteger(value) && value >= 1 && value <= 65535
    ? null
    : "Enter a port from 1 to 65535.";
};

export const oscHostError = (host: string): string | null =>
  host.trim() && !/\s/.test(host.trim()) ? null : "Enter a host name or IP address without spaces.";

/** One argument as typed: the type plus the raw text of its value. */
export interface OscArgumentDraft {
  type: OscArgumentType;
  text: string;
}

export const argumentToDraft = (argument: OscArgument): OscArgumentDraft => ({
  type: argument.type,
  text: String(argument.value),
});

/** Parses a draft, or returns an error message. A float keeps its type even when written as 1. */
export const parseOscArgument = (draft: OscArgumentDraft): OscArgument | string => {
  const text = draft.text.trim();
  switch (draft.type) {
    case "int": {
      if (!/^-?\d+$/.test(text)) return "Enter a whole number.";
      const value = Number(text);
      return Math.abs(value) < 2 ** 31 ? { type: "int", value } : "Enter a whole number below 2147483648.";
    }
    case "float": {
      if (text === "" || !Number.isFinite(Number(text))) return "Enter a number.";
      return { type: "float", value: Number(text) };
    }
    case "bool": {
      if (text === "true" || text === "false") return { type: "bool", value: text === "true" };
      return "Choose true or false.";
    }
    default:
      return draft.text.length <= 512 ? { type: "string", value: draft.text } : "Use 512 characters or fewer.";
  }
};

/** Parses all drafts; the first error wins. */
export const parseOscArguments = (drafts: OscArgumentDraft[]): { args: OscArgument[]; error: string | null } => {
  const args: OscArgument[] = [];
  for (const draft of drafts) {
    const parsed = parseOscArgument(draft);
    if (typeof parsed === "string") return { args: [], error: parsed };
    args.push(parsed);
  }
  return { args, error: null };
};

export const describeOscArguments = (args: OscArgument[]): string =>
  args.map((argument) => `${argument.value}`).join(" ");

export const newOscCue = (id: string): OscLightingCue => ({ id, at_seconds: 0, address: "", args: [], label: "" });

/** mm:ss, or mm:ss.t when the time is not a whole second (cue times are kept to 0.1 s). */
export const formatElapsed = (seconds: number) => {
  const tenths = Math.round(Math.max(0, seconds) * 10);
  const whole = Math.floor(tenths / 10);
  const mmss = `${String(Math.floor(whole / 60)).padStart(2, "0")}:${String(whole % 60).padStart(2, "0")}`;
  return tenths % 10 === 0 ? mmss : `${mmss}.${tenths % 10}`;
};

/** Accepts mm:ss and mm:ss.t. Returns seconds, or null when the text is not a time. */
export const parseElapsed = (value: string) => {
  const match = /^(\d{1,3}):([0-5]\d)(?:\.(\d))?$/.exec(value.trim());
  if (!match) return null;
  return Math.round((Number(match[1]) * 60 + Number(match[2]) + Number(match[3] ?? 0) / 10) * 10) / 10;
};
