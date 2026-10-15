import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi, type Mock } from "vitest";

import type {
  ApplicationState,
  LightingCaptureResponse,
  LightingPad,
  LightsStatusResponse,
  SettingsResponse,
  Song,
} from "../types";
import { LightingSetupPanel } from "./LightingSetupPanel";
import { LightingTimeline } from "./LightingTimeline";
import type { TimelineMarker } from "./lightingTimeline";

const song: Song = { id: "item-1", source_song_id: "song-1", title: "Holy Forever", duration_seconds: 200, order: 1, is_generic: false };
const base = { osc_preset: "lightkey" as const, osc_host: "127.0.0.1", osc_port: 21600, enabled: true, output_name: "Out", channel: 1, pulse_ms: 100 };
const lightsFor = (method: "osc" | "midi"): LightsStatusResponse => ({
  ...base, method, connection_status: "connected", detail: null, outputs: [], last_cue: null, last_osc_cue: null, last_cue_at: null,
});
const oscPad = (id: string, label: string): LightingPad => ({ id, label, color: "teal", method: "osc", address: `/cue/${label}`, args: [], note: null, velocity: 127 });
const midiPad = (id: string, label: string): LightingPad => ({ id, label, color: null, method: "midi", address: null, args: [], note: 40, velocity: 100 });

const settingsFor = (method: "osc" | "midi", pads: LightingPad[], osc_cues: unknown[] = [], cues: unknown[] = []) => ({
  planning_center_secret_saved: true, warning: null, restart_required: false,
  settings: { lights: { ...base, method, pads, cue_maps: osc_cues.length || cues.length ? { "song-1": { song_key: "song-1", song_title: "Holy Forever", cues, osc_cues } } : {} } },
}) as unknown as SettingsResponse;

const runningState = (secondsAgo: number) => ({
  plan: { id: "p", title: "Sunday", songs: [song] },
  current_song: song,
  playing_song: song,
  timer: { status: "running", duration_seconds: 200, started_at: new Date(Date.now() - secondsAgo * 1000).toISOString(), position_seconds: 0 },
}) as unknown as ApplicationState;
const idleState = { plan: { id: "p", title: "Sunday", songs: [song] }, current_song: song, timer: { status: "idle", duration_seconds: null, started_at: null } } as unknown as ApplicationState;

const response = (over: Partial<LightingCaptureResponse> = {}): LightingCaptureResponse => ({
  accepted: true, message: "Sent", lights: lightsFor("osc"), cue_id: "cue-1", at_seconds: 72, ...over,
});

const mount = (props: {
  method?: "osc" | "midi"; pads?: LightingPad[]; state?: ApplicationState;
  onPressPad?: Mock; onUndoCapture?: Mock; onSavePads?: Mock;
  osc_cues?: unknown[]; cues?: unknown[]; onSaveCues?: Mock;
}) => {
  const method = props.method ?? "osc";
  const pads = props.pads ?? [oscPad("p1", "Chorus"), oscPad("p2", "Blackout")];
  const onPressPad = props.onPressPad ?? vi.fn().mockResolvedValue(response());
  const onUndoCapture = props.onUndoCapture ?? vi.fn().mockResolvedValue(true);
  const onSavePads = props.onSavePads ?? vi.fn().mockResolvedValue(true);
  const onSaveCues = props.onSaveCues ?? vi.fn();
  render(
    <LightingSetupPanel
      error={null}
      lights={lightsFor(method)}
      message={null}
      onPressPad={onPressPad}
      onRefresh={vi.fn()}
      onSaveCues={onSaveCues}
      onSavePads={onSavePads}
      onSaveSettings={vi.fn()}
      onTest={vi.fn()}
      onTestOsc={vi.fn()}
      onUndoCapture={onUndoCapture}
      pendingOperation={null}
      settings={settingsFor(method, pads, props.osc_cues, props.cues)}
      state={props.state ?? idleState}
    />,
  );
  return { onPressPad, onUndoCapture, onSavePads, onSaveCues };
};

describe("cue board and live capture", () => {
  it("captures at the elapsed time rounded to 0.1 s while the song runs", async () => {
    const { onPressPad } = mount({ state: runningState(72.04) });
    await userEvent.click(screen.getByRole("button", { name: /Chorus/ }));
    await waitFor(() => expect(onPressPad).toHaveBeenCalledTimes(1));
    const [padId, capture] = onPressPad.mock.calls[0]!;
    expect(padId).toBe("p1");
    expect(capture.song_key).toBe("song-1");
    expect(capture.song_title).toBe("Holy Forever");
    expect(capture.at_seconds).toBeGreaterThanOrEqual(72);
    expect(capture.at_seconds).toBeLessThan(73);
    expect(Number.isInteger(Math.round(capture.at_seconds * 10))).toBe(true);
    expect(Math.abs(capture.at_seconds * 10 - Math.round(capture.at_seconds * 10))).toBeLessThan(1e-9); // whole tenths
    expect(await screen.findByTestId("capture-status")).toHaveTextContent(/Added at 01:12/);
  });

  it("Preview only fires without writing to the song", async () => {
    const onPressPad = vi.fn().mockResolvedValue(response({ cue_id: null, at_seconds: null }));
    mount({ state: runningState(30), onPressPad });
    await userEvent.click(screen.getByRole("checkbox", { name: /Preview only/ }));
    await userEvent.click(screen.getByRole("button", { name: /Chorus/ }));
    await waitFor(() => expect(onPressPad).toHaveBeenCalledTimes(1));
    expect(onPressPad.mock.calls[0]![1]).toBeUndefined();
    expect(await screen.findByTestId("capture-status")).toHaveTextContent(/Nothing was added/);
  });

  it("with no song running a press only fires and says so", async () => {
    const onPressPad = vi.fn().mockResolvedValue(response({ cue_id: null, at_seconds: null }));
    mount({ state: idleState, onPressPad });
    await userEvent.click(screen.getByRole("button", { name: /Blackout/ }));
    await waitFor(() => expect(onPressPad).toHaveBeenCalledTimes(1));
    expect(onPressPad.mock.calls[0]![1]).toBeUndefined();
    expect(await screen.findByTestId("capture-status")).toHaveTextContent(/No song is running/);
  });

  it("undoes the last capture and then disables Undo", async () => {
    const { onUndoCapture } = mount({ state: runningState(10) });
    const undo = screen.getByRole("button", { name: "Undo last capture" });
    expect(undo).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: /Chorus/ }));
    await waitFor(() => expect(undo).toBeEnabled());
    await userEvent.click(undo);
    await waitFor(() => expect(onUndoCapture).toHaveBeenCalledWith("song-1", "cue-1"));
    await waitFor(() => expect(undo).toBeDisabled());
  });

  it("keyboard shortcuts 1-9 press the Nth pad when the board has focus", async () => {
    const { onPressPad } = mount({ state: idleState });
    const first = screen.getByRole("button", { name: /Chorus/ });
    first.focus();
    await userEvent.keyboard("2");
    await waitFor(() => expect(onPressPad).toHaveBeenCalledWith("p2", undefined));
  });

  it("dims pads of the other method, keeps them, and cannot fire them", async () => {
    const { onPressPad } = mount({ method: "midi", pads: [oscPad("p1", "Chorus"), midiPad("p3", "Strobe")], state: idleState });
    const dimmed = screen.getByTestId("lighting-pad-dimmed");
    expect(dimmed).toBeDisabled();
    expect(dimmed).toHaveTextContent("Chorus");
    expect(within(screen.getByTestId("lighting-pads")).getAllByTestId("lighting-pad")).toHaveLength(1);
    await userEvent.click(dimmed);
    expect(onPressPad).not.toHaveBeenCalled();
  });

  it("creates, edits, reorders and deletes pads through onSavePads", async () => {
    const onSavePads = vi.fn().mockResolvedValue(true);
    mount({ onSavePads });
    await userEvent.click(screen.getByRole("button", { name: "Edit pads" }));

    await userEvent.click(screen.getByRole("button", { name: "Add pad" }));
    await userEvent.type(screen.getByLabelText("Pad name"), "Verse");
    await userEvent.type(within(screen.getByRole("form", { name: "New pad" })).getByLabelText("OSC address"), "bad");
    await userEvent.click(within(screen.getByRole("form", { name: "New pad" })).getByRole("button", { name: "Add pad" }));
    expect(onSavePads).not.toHaveBeenCalled(); // invalid address is refused
    await userEvent.clear(within(screen.getByRole("form", { name: "New pad" })).getByLabelText("OSC address"));
    await userEvent.type(within(screen.getByRole("form", { name: "New pad" })).getByLabelText("OSC address"), "/cue/9");
    await userEvent.click(within(screen.getByRole("form", { name: "New pad" })).getByRole("button", { name: "Add pad" }));
    await waitFor(() => expect(onSavePads).toHaveBeenCalledTimes(1));
    const created = onSavePads.mock.calls[0]![0] as LightingPad[];
    expect(created.map((item) => item.label)).toEqual(["Chorus", "Blackout", "Verse"]);
    expect(created[2]).toMatchObject({ method: "osc", address: "/cue/9", note: null });

    await userEvent.click(screen.getByRole("button", { name: "Edit Chorus" }));
    const nameField = screen.getByLabelText("Pad name");
    await userEvent.clear(nameField);
    await userEvent.type(nameField, "Big chorus");
    await userEvent.click(within(screen.getByRole("form", { name: "Edit pad" })).getByRole("button", { name: "Save pad" }));
    await waitFor(() => expect(onSavePads).toHaveBeenCalledTimes(2));
    expect((onSavePads.mock.calls[1]![0] as LightingPad[])[0]).toMatchObject({ id: "p1", label: "Big chorus" });

    await userEvent.click(screen.getByRole("button", { name: "Move Chorus later" }));
    expect((onSavePads.mock.calls[2]![0] as LightingPad[]).map((item) => item.id)).toEqual(["p2", "p1"]);

    await userEvent.click(screen.getByRole("button", { name: "Delete Blackout" }));
    expect((onSavePads.mock.calls[3]![0] as LightingPad[]).map((item) => item.id)).toEqual(["p1"]);
  });
});

describe("timeline in the panel", () => {
  const cue = (id: string, at: number, label: string) => ({ id, at_seconds: at, address: `/${label}`, args: [], label, color: "teal" });

  it("shows markers for the method on screen only", () => {
    mount({
      method: "midi",
      osc_cues: [cue("o1", 20, "osc-only")],
      cues: [{ id: "m1", at_seconds: 40, note: 60, velocity: 100, label: "midi-only" }],
    });
    const markers = screen.getAllByTestId("lighting-marker");
    expect(markers).toHaveLength(1);
    expect(markers[0]).toHaveAccessibleName("midi-only at 00:40");
  });

  it("a keyboard nudge saves the moved cue through the same save as the cue rows", async () => {
    const { onSaveCues } = mount({ osc_cues: [cue("o1", 20, "wash")] });
    const marker = screen.getByRole("button", { name: "wash at 00:20" });
    marker.focus();
    await userEvent.keyboard("{ArrowRight}");
    await waitFor(() => expect(onSaveCues).toHaveBeenCalledTimes(1));
    expect(onSaveCues.mock.calls[0]![1][0]).toMatchObject({ id: "o1", at_seconds: 20.1 });
    expect(onSaveCues.mock.calls[0]![2]).toBe("osc");
  });

  it("draws the playhead only while the song runs", () => {
    mount({ state: runningState(50) });
    expect(screen.getByTestId("lighting-playhead")).toBeInTheDocument();
  });
});

describe("LightingTimeline", () => {
  const markers: TimelineMarker[] = [{ id: "a", at: 10, label: "Wash", color: "teal" }];
  const stubTrack = () => {
    const track = screen.getByTestId("lighting-timeline-track");
    track.getBoundingClientRect = () => ({ left: 0, top: 0, right: 1000, bottom: 56, width: 1000, height: 56, x: 0, y: 0, toJSON: () => ({}) });
  };
  const Harness = ({ onMove }: { onMove: (id: string, at: number) => void }) => {
    const [list, setList] = useState(markers);
    return <LightingTimeline markers={list} onMove={(id, at) => { onMove(id, at); setList((current) => current.map((m) => m.id === id ? { ...m, at } : m)); }} onRemove={vi.fn()} playhead={null} songLength={200} />;
  };

  it("dragging snaps to 0.1 s and moves on release", () => {
    const onMove = vi.fn();
    render(<Harness onMove={onMove} />);
    stubTrack();
    const marker = screen.getByTestId("lighting-marker");
    fireEvent.pointerDown(marker, { button: 0, pointerId: 1, clientX: 50 });
    fireEvent.pointerMove(marker, { pointerId: 1, clientX: 333 });
    fireEvent.pointerUp(marker, { pointerId: 1, clientX: 333 });
    expect(onMove).toHaveBeenCalledWith("a", 66.6);
  });

  it("dragging past either end clamps to the song", () => {
    const onMove = vi.fn();
    render(<Harness onMove={onMove} />);
    stubTrack();
    const marker = screen.getByTestId("lighting-marker");
    fireEvent.pointerDown(marker, { button: 0, pointerId: 1, clientX: 50 });
    fireEvent.pointerUp(marker, { pointerId: 1, clientX: 5000 });
    expect(onMove).toHaveBeenLastCalledWith("a", 200);
    fireEvent.pointerDown(screen.getByTestId("lighting-marker"), { button: 0, pointerId: 1, clientX: 900 });
    fireEvent.pointerUp(screen.getByTestId("lighting-marker"), { pointerId: 1, clientX: -300 });
    expect(onMove).toHaveBeenLastCalledWith("a", 0);
  });

  it("arrow keys nudge 0.1 s and Shift 1 s, Delete removes, selection opens the popover", async () => {
    const onMove = vi.fn();
    const onRemove = vi.fn();
    render(<LightingTimeline markers={markers} onMove={onMove} onRemove={onRemove} playhead={null} songLength={200} />);
    const marker = screen.getByRole("button", { name: "Wash at 00:10" });
    marker.focus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(onMove).toHaveBeenLastCalledWith("a", 9.9);
    await userEvent.keyboard("{Shift>}{ArrowRight}{/Shift}");
    expect(onMove).toHaveBeenLastCalledWith("a", 11);
    await userEvent.click(marker);
    expect(screen.getByTestId("lighting-marker-popover")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Delete cue at 00:10" }));
    expect(onRemove).toHaveBeenCalledWith("a");
  });

  it("stays scrollable on narrow widths instead of clipping", () => {
    render(<LightingTimeline markers={markers} onMove={vi.fn()} onRemove={vi.fn()} playhead={null} songLength={200} />);
    const scroll = screen.getByTestId("lighting-timeline-scroll");
    expect(scroll.className).toContain("overflow-x-auto");
  });
});
