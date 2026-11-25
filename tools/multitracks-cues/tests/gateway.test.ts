import { describe, expect, it, vi } from "vitest";
import { MultiTracksGateway } from "../src/multitracks/gateway.js";
import type { SafeMcpClient } from "../src/mcp/client.js";

function mockClient(): SafeMcpClient {
  return {
    schema: vi.fn(() => ({
      type: "object" as const,
      properties: {
        setlistId: { type: "string" },
        libraryEntryId: { type: "string" },
        bankId: { type: "string" },
        busId: { type: "string" },
        arrangementId: { type: "string" },
        cue: { type: "object" },
        from: { type: "string" },
        to: { type: "string" },
        limit: { type: "number" },
        position: {
          type: "object",
          properties: {
            measure: { type: "integer", minimum: 1 },
            beat: { type: "integer", minimum: 1, maximum: 4 },
            tick: { type: "integer", minimum: 0 },
          },
          required: ["measure", "beat", "tick"],
        },
      },
    })),
    call: vi.fn(async (name: string) => {
      if (name === "setlistsList") return { setlists: [{ id: "s1", name: "Sunday", targetDate: "2026-01-04" }] };
      if (name === "setlistGet") return { id: "s1", name: "Sunday", items: [] };
      if (name === "midiBusesList") return { buses: [{ id: "aux-1", name: "Aux 1" }] };
      if (name === "libraryMidiBanksList") return { banks: [{ id: "b1", name: "Default", isDefault: true }] };
      if (name === "libraryMidiEventsList") return { events: [] };
      if (name === "cloudArrangementMidiEventsList") return { events: [] };
      if (name === "libraryMidiEventCreate") return { success: true };
      if (name === "cloudArrangementMidiEventCreate") return { success: true };
      return {};
    }),
  } as unknown as SafeMcpClient;
}

describe("MultiTracksGateway", () => {
  it("lists setlists", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const setlists = await gateway.listSetlists();
    expect(setlists).toHaveLength(1);
    expect(setlists[0]!.id).toBe("s1");
  });

  it("gets a single setlist", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const setlist = await gateway.getSetlist("s1");
    expect(setlist.id).toBe("s1");
  });

  it("lists MIDI buses", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const buses = await gateway.listMidiBuses();
    expect(buses).toHaveLength(1);
    expect(buses[0]!.id).toBe("aux-1");
  });

  it("lists library banks", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const banks = await gateway.listLibraryBanks("entry-1");
    expect(banks).toHaveLength(1);
    expect(banks[0]!.id).toBe("b1");
  });

  it("lists library events", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const events = await gateway.listLibraryEvents("entry-1", "bank-1", "bus-1");
    expect(events).toEqual([]);
  });

  it("lists cloud events", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const events = await gateway.listCloudEvents("arr-1", "bus-1");
    expect(events).toEqual([]);
  });

  it("creates library event", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const result = await gateway.createLibraryEvent("bus-1", "entry-1", "bank-1", {
      channel: 1, note: 112, velocity: 1, songOrdinal: 1, position: { measure: 1, beat: 2, tick: 0 },
    });
    expect(result).toBeDefined();
  });

  it("creates cloud event", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const result = await gateway.createCloudEvent("bus-1", "arr-1", {
      channel: 1, note: 112, velocity: 1, songOrdinal: 1, position: { measure: 1, beat: 2, tick: 0 },
    });
    expect(result).toBeDefined();
  });

  it("resolves a cue for library target", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const cue = gateway.resolveCue("library", 3);
    expect(cue.velocity).toBe(3);
    expect(cue.note).toBe(112);
    expect(cue.channel).toBe(1);
  });

  it("resolves a cue for cloud target", async () => {
    const gateway = new MultiTracksGateway(mockClient());
    const cue = gateway.resolveCue("cloud", 5);
    expect(cue.velocity).toBe(5);
  });
});

