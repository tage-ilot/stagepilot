import type { DynamicToolSpec } from "../agent/tools.js";

export const MULTITRACKS_MCP_TOOLS: DynamicToolSpec[] = [
  // Setlist Management
  {
    type: "function",
    name: "multitracks_setlists_list",
    description: "List your MultiTracks setlists",
    inputSchema: {
      type: "object",
      properties: {},
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_setlist_get",
    description: "Get details for a specific setlist",
    inputSchema: {
      type: "object",
      properties: {
        name: { type: "string", description: "Setlist name or 'this Sunday'" },
      },
      required: ["name"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_setlist_create",
    description: "Create a new setlist",
    inputSchema: {
      type: "object",
      properties: {
        name: { type: "string", description: "Setlist name" },
        date: { type: "string", description: "Target date (YYYY-MM-DD)" },
      },
      required: ["name", "date"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_setlist_duplicate",
    description: "Duplicate an existing setlist for a new date",
    inputSchema: {
      type: "object",
      properties: {
        source: { type: "string", description: "Source setlist name" },
        date: { type: "string", description: "New target date (YYYY-MM-DD)" },
      },
      required: ["source", "date"],
      additionalProperties: false,
    },
  },

  // Song Management
  {
    type: "function",
    name: "multitracks_song_add",
    description: "Add a song to a setlist",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        setlist: { type: "string", description: "Setlist name (default: this Sunday)" },
      },
      required: ["song"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_song_remove",
    description: "Remove a song from a setlist",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_song_set_key",
    description: "Set the key for a song in a setlist",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        key: { type: "string", description: "Musical key (e.g., G, D, A minor)" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "key"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_song_set_tempo",
    description: "Set the tempo for a song in a setlist",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        tempo: { type: "number", description: "BPM (beats per minute)" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "tempo"],
      additionalProperties: false,
    },
  },

  // Automation Cues (Primary Focus)
  {
    type: "function",
    name: "multitracks_cue_list",
    description: "List all automation cues for a song in a setlist",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_cue_targets",
    description: "List automatable targets (tracks, buses, click, guide, pads) for a song",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_cue_create",
    description: "Create a new automation cue at a specific section of a song",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        section: { type: "string", description: "Song section (e.g., chorus, bridge, verse)" },
        name: { type: "string", description: "Optional cue name" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "section"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_cue_delete",
    description: "Delete an automation cue by name",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        name: { type: "string", description: "Cue name" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "name"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_cue_rename",
    description: "Rename an automation cue",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        oldName: { type: "string", description: "Current cue name" },
        newName: { type: "string", description: "New cue name" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "oldName", "newName"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_cue_clear",
    description: "Clear all automation cues for a song in a setlist",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song"],
      additionalProperties: false,
    },
  },

  // Track Automation
  {
    type: "function",
    name: "multitracks_track_mute",
    description: "Mute a track at a specific section of a song",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        track: { type: "string", description: "Track name (e.g., electric guitar, drums)" },
        section: { type: "string", description: "Song section (e.g., chorus, bridge)" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "track", "section"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_track_volume",
    description: "Set track volume at a specific section",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        track: { type: "string", description: "Track name" },
        volume: { type: "number", description: "Volume percentage (0-100)" },
        section: { type: "string", description: "Song section" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "track", "volume", "section"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_track_ramp",
    description: "Ramp track volume between two trigger points",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        track: { type: "string", description: "Track name" },
        startVolume: { type: "number", description: "Start volume percentage" },
        endVolume: { type: "number", description: "End volume percentage" },
        section: { type: "string", description: "Song section" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "track", "startVolume", "endVolume", "section"],
      additionalProperties: false,
    },
  },

  // Bus Automation
  {
    type: "function",
    name: "multitracks_bus_mute",
    description: "Mute a bus at a specific section",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        bus: { type: "string", description: "Bus name (e.g., click, guide)" },
        section: { type: "string", description: "Song section" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "bus", "section"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_bus_volume",
    description: "Set bus volume at a specific section",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        bus: { type: "string", description: "Bus name" },
        volume: { type: "number", description: "Volume percentage" },
        section: { type: "string", description: "Song section" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "bus", "volume", "section"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_bus_ramp",
    description: "Ramp bus volume between two trigger points",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        bus: { type: "string", description: "Bus name" },
        startVolume: { type: "number", description: "Start volume percentage" },
        endVolume: { type: "number", description: "End volume percentage" },
        section: { type: "string", description: "Song section" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "bus", "startVolume", "endVolume", "section"],
      additionalProperties: false,
    },
  },

  // Click and Guide
  {
    type: "function",
    name: "multitracks_click_mute",
    description: "Mute the click track at a specific section",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        section: { type: "string", description: "Song section" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "section"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_guide_mute",
    description: "Mute the guide vocal at a specific section",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        section: { type: "string", description: "Song section" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "section"],
      additionalProperties: false,
    },
  },

  // MIDI and Pad
  {
    type: "function",
    name: "multitracks_midi_mute",
    description: "Mute MIDI output at a specific section",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        section: { type: "string", description: "Song section" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "section"],
      additionalProperties: false,
    },
  },
  {
    type: "function",
    name: "multitracks_pad_mute",
    description: "Mute the pad player at a specific section",
    inputSchema: {
      type: "object",
      properties: {
        song: { type: "string", description: "Song name" },
        section: { type: "string", description: "Song section" },
        setlist: { type: "string", description: "Setlist name" },
      },
      required: ["song", "section"],
      additionalProperties: false,
    },
  },
];

export class MultiTracksMCPTools {
  private readonly tools = MULTITRACKS_MCP_TOOLS;

  definitions(): DynamicToolSpec[] {
    return structuredClone(this.tools);
  }

  async call(name: string, input: unknown): Promise<unknown> {
    // All tools are handled by sending prompts to ChatGPT with @Multitracks
    // This is a placeholder that returns the tool spec for the agent to use
    const tool = this.tools.find((t) => t.name === name);
    if (!tool) {
      throw new Error(`Unknown MultiTracks MCP tool: ${name}`);
    }
    return { tool: tool.name, input, description: tool.description };
  }
}

