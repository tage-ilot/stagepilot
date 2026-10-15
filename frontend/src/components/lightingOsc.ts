import type { OscArgument, OscArgumentType, OscLightingCue, OscPreset } from "../types";

export interface OscAction {
  label: string;
  address: string;
  args: OscArgument[];
}

export interface OscPresetInfo {
  value: OscPreset;
  label: string;
  /** Default UDP port, or null where the app's own docs name none (the user types it). */
  port: number | null;
  /** Default host, filled only where the app normally runs on the same computer. */
  host: string | null;
  /** Shown as helper text under the address fields. */
  pattern: string;
  actions: OscAction[];
}

const int = (value: number): OscArgument => ({ type: "int", value });

/** Ports are verified against each app's own docs; sources live in docs/lights.md. */
export const oscPresets: OscPresetInfo[] = [
  {
    value: "lightkey",
    label: "Lightkey",
    port: 21600,
    host: "127.0.0.1",
    pattern: "In Lightkey, control-click a cue and choose External Control, then Copy OSC Address.",
    actions: [],
  },
  {
    value: "qlc_plus",
    label: "QLC+",
    port: 7700,
    host: null,
    pattern: "Pattern: /universe-1/dmx/channel-1, for example /0/dmx/11 with a float value.",
    actions: [],
  },
  {
    value: "chamsys",
    label: "Chamsys",
    port: 8000,
    host: null,
    pattern: "Pattern: /pb/1/go starts playback 1. /dbo is blackout.",
    actions: [
      { label: "Playback 1 go", address: "/pb/1/go", args: [int(1)] },
      { label: "Blackout on", address: "/dbo", args: [int(0)] },
      { label: "Blackout off", address: "/dbo", args: [int(1)] },
    ],
  },
  {
    value: "etc_eos",
    label: "ETC Eos",
    port: 8000,
    host: null,
    pattern: "Pattern: /eos/cue/list/number/fire, for example /eos/cue/1/1/fire.",
    actions: [{ label: "Fire cue 1 in list 1", address: "/eos/cue/1/1/fire", args: [] }],
  },
  {
    value: "grandma",
    label: "GrandMA",
    port: null,
    host: null,
    pattern: "Pattern: /Page1/Fader201 with an int value. Use the port set in the OSC menu.",
    actions: [{ label: "Executor 201 to full", address: "/Page1/Fader201", args: [int(100)] }],
  },
  {
    value: "resolume",
    label: "Resolume",
    port: null,
    host: null,
    pattern: "Pattern: /composition/layers/1/clips/1/connect with an int value of 1.",
    actions: [
      { label: "Connect layer 1 clip 1", address: "/composition/layers/1/clips/1/connect", args: [int(1)] },
    ],
  },
  {
    value: "custom",
    label: "Custom",
    port: null,
    host: null,
    pattern: "Enter the address your lighting app expects. It must start with / and have no spaces.",
    actions: [],
  },
];

export const oscPresetInfo = (preset: OscPreset): OscPresetInfo =>
  oscPresets.find((item) => item.value === preset) ?? oscPresets[oscPresets.length - 1]!;

export const defaultOscPort = (preset: OscPreset): string => {
  const port = oscPresetInfo(preset).port;
  return port === null ? "" : String(port);
};

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

export const formatElapsed = (seconds: number) =>
  `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;

export const parseElapsed = (value: string) => {
  const match = /^(\d{1,3}):([0-5]\d)$/.exec(value.trim());
  return match ? Number(match[1]) * 60 + Number(match[2]) : null;
};
