import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import { playbackStatus } from "../test/playbackFixtures";
import { PlaybackApiPanel } from "./PlaybackApiPanel";
import { PlaybackSaveBar, usePlaybackDraft } from "./PlaybackSaveFooter";
import { usePlaybackSave, type PlaybackMidiStep } from "./usePlaybackSave";

const noMidi: PlaybackMidiStep = { pending: false, count: 0, valid: true, snapshot: "", apply: async () => true, revert: () => undefined };
const controller = (overrides: Partial<PlaybackController> = {}): PlaybackController => ({
  status: playbackStatus(), events: [], error: null, message: null, pending: null, saveArrangement: vi.fn(async () => true),
  save: vi.fn().mockResolvedValue(true), selectSource: vi.fn().mockResolvedValue(true), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(), ...overrides,
});
const deferred = <T,>() => { let resolve!: (v: T) => void; const promise = new Promise<T>((r) => { resolve = r; }); return { promise, resolve }; };

function Host({ playback, midi = noMidi }: { playback: PlaybackController; midi?: PlaybackMidiStep }) {
  const draft = usePlaybackDraft(playback, "real");
  const bar = usePlaybackSave(draft, playback, midi);
  return (
    <>
      <PlaybackApiPanel playback={playback} draft={draft} />
      <button onClick={() => { draft.setMethod("playback_api"); }} type="button">edit</button>
      <PlaybackSaveBar save={bar} />
    </>
  );
}
const saveButton = () => screen.getByRole("button", { name: "Save configuration" });

describe("Playback Save configuration is single-flight", () => {
  it("user double-click while the first step is pending: one selectSource, nothing else", async () => {
    const gate = deferred<boolean>();
    const playback = controller({ selectSource: vi.fn(() => gate.promise), save: vi.fn().mockResolvedValue(true) });
    const user = userEvent.setup();
    render(<Host playback={playback} />);
    await user.click(screen.getByRole("button", { name: "edit" }));
    await user.dblClick(saveButton());
    await user.click(saveButton());
    expect(playback.selectSource).toHaveBeenCalledTimes(1);
    await act(async () => { gate.resolve(true); await Promise.resolve(); });
    expect(playback.selectSource).toHaveBeenCalledTimes(1);
    // Connecting automatically is always on, so there is no separate settings save any more.
    expect(playback.save).not.toHaveBeenCalled();
  });

  it("two rapid fireEvent.click in one tick: still one selectSource", async () => {
    const gate = deferred<boolean>();
    const playback = controller({ selectSource: vi.fn(() => gate.promise) });
    render(<Host playback={playback} />);
    fireEvent.click(screen.getByRole("button", { name: "edit" }));
    fireEvent.click(saveButton());
    fireEvent.click(saveButton());
    expect(playback.selectSource).toHaveBeenCalledTimes(1);
    await act(async () => { gate.resolve(true); await Promise.resolve(); });
  });

  it("order method -> MIDI; stops on first failure; Try again does not repeat the finished step", async () => {
    const order: string[] = [];
    let midiOk = false;
    const midiApply = vi.fn(async () => { order.push("midi"); return midiOk; });
    const midi: PlaybackMidiStep = { ...noMidi, pending: true, count: 1, snapshot: "m1", apply: midiApply };
    const playback = controller({ selectSource: vi.fn(async () => { order.push("method"); return true; }) });
    const user = userEvent.setup();
    render(<Host playback={playback} midi={midi} />);
    await user.click(screen.getByRole("button", { name: "edit" }));
    await user.click(saveButton());
    expect(order).toEqual(["method", "midi"]);
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
    midiOk = true;
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(order).toEqual(["method", "midi", "midi"]);
    expect(playback.selectSource).toHaveBeenCalledTimes(1);
    expect(playback.save).not.toHaveBeenCalled();
  });
});

describe("Scan Network double click", () => {
  it("the second click of a double click starts nothing and does not cancel", async () => {
    const playback = controller();
    const user = userEvent.setup();
    const view = render(<PlaybackApiPanel playback={playback} />);
    await user.click(screen.getByRole("button", { name: "Scan Network" }));
    expect(playback.scan).toHaveBeenCalledTimes(1);
    // The hook flips pending to "scan" and the button becomes Cancel; a click within the guard is ignored.
    view.rerender(<PlaybackApiPanel playback={{ ...playback, pending: "scan" }} />);
    await user.click(screen.getByRole("button", { name: /Scanning/ }));
    expect(playback.cancelScan).not.toHaveBeenCalled();
  });

  it("Cancel works after the guard has passed", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const playback = controller();
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      const view = render(<PlaybackApiPanel playback={playback} />);
      await user.click(screen.getByRole("button", { name: "Scan Network" }));
      view.rerender(<PlaybackApiPanel playback={{ ...playback, pending: "scan" }} />);
      await act(async () => { vi.advanceTimersByTime(1000); });
      await user.click(screen.getByRole("button", { name: /Scanning/ }));
      expect(playback.cancelScan).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });
});
