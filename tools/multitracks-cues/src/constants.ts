export const APP_NAME = "StagePilot MultiTracks Cues";
export const APP_VERSION = "0.1.0";
export const MCP_SERVER_URL = "https://mcp.multitracks.com/mcp";
export const DEFAULT_SCOPE = "mcp offline_access openid profile";
export const DEFAULT_MULTITRACKS_CLIENT_ID = "mcp-chatgpt";
export const DEFAULT_MULTITRACKS_CLIENT_SECRET = "multitracksMCPChatGPT";

export const REQUIRED_READ_TOOLS = [
  "setlistsList",
  "setlistGet",
  "midiBusesList",
  "libraryMidiBanksList",
  "libraryMidiEventsList",
  "cloudArrangementMidiEventsList",
] as const;

export const ALLOWED_WRITE_TOOLS = [
  "libraryMidiEventCreate",
  "cloudArrangementMidiEventCreate",
] as const;

export const FORBIDDEN_TOOLS = [
  "libraryMidiBankCopyBusCues",
  "libraryMidiBankImportProductionCues",
  "libraryMidiBankUploadMidiFile",
  "libraryMidiBankDelete",
  "libraryMidiEventDelete",
  "cloudArrangementMidiEventDelete",
] as const;

export const ALLOWED_TOOLS = new Set<string>([
  ...REQUIRED_READ_TOOLS,
  ...ALLOWED_WRITE_TOOLS,
]);

export const WRITE_TOOLS = new Set<string>(ALLOWED_WRITE_TOOLS);

export const EXIT = {
  SUCCESS: 0,
  INVALID: 2,
  AUTH: 3,
  CAPABILITY: 4,
  AMBIGUOUS: 5,
  PARTIAL_FAILURE: 6,
  VERIFICATION: 7,
  SCHEMA: 8,
  CREDENTIAL_STORE: 9,
} as const;

export const DEFAULT_CUE = {
  kind: "note-on" as const,
  channel: 1,
  note: 112,
  musicalNote: "E7",
  velocity: 100,
  position: { kind: "song-start" as const },
  bankName: "StagePilot",
};

export const SETLIST_ORDINAL_TEST_PROFILE = "setlist-ordinal-test" as const;

