import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { usePanelPresence } from "./usePanelPresence";

type Id = "a" | "b";
const setup = (ms = 300) => renderHook(({ t }: { t: Id | null }) => usePanelPresence<Id>(t, ms), { initialProps: { t: null as Id | null } });

describe("usePanelPresence", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("goes closed -> opening -> open, and open -> closing -> closed with unmount only after the duration", () => {
    const h = setup();
    expect(h.result.current).toEqual({ shown: null, phase: "closed" });
    h.rerender({ t: "a" });
    expect(h.result.current).toEqual({ shown: "a", phase: "opening" });
    act(() => { vi.advanceTimersByTime(299); });
    expect(h.result.current.phase).toBe("opening");
    act(() => { vi.advanceTimersByTime(1); });
    expect(h.result.current.phase).toBe("open");
    h.rerender({ t: null });
    expect(h.result.current).toEqual({ shown: "a", phase: "closing" });
    act(() => { vi.advanceTimersByTime(299); });
    expect(h.result.current.shown).toBe("a");
    act(() => { vi.advanceTimersByTime(1); });
    expect(h.result.current).toEqual({ shown: null, phase: "closed" });
  });

  it("turns round when reopened during the close", () => {
    const h = setup();
    h.rerender({ t: "a" });
    act(() => { vi.advanceTimersByTime(300); });
    h.rerender({ t: null });
    act(() => { vi.advanceTimersByTime(100); });
    h.rerender({ t: "a" });
    expect(h.result.current).toEqual({ shown: "a", phase: "opening" });
    act(() => { vi.advanceTimersByTime(300); });
    expect(h.result.current).toEqual({ shown: "a", phase: "open" });
  });

  it("closing during the open settles closed with no stale timer", () => {
    const h = setup();
    h.rerender({ t: "a" });
    act(() => { vi.advanceTimersByTime(100); });
    h.rerender({ t: null });
    act(() => { vi.advanceTimersByTime(300); });
    expect(h.result.current).toEqual({ shown: null, phase: "closed" });
    act(() => { vi.advanceTimersByTime(1000); });
    expect(h.result.current).toEqual({ shown: null, phase: "closed" });
  });

  it("switching panels closes the old one, then opens the new one", () => {
    const h = setup();
    h.rerender({ t: "a" });
    act(() => { vi.advanceTimersByTime(300); });
    h.rerender({ t: "b" });
    expect(h.result.current).toEqual({ shown: "a", phase: "closing" });
    act(() => { vi.advanceTimersByTime(300); });
    expect(h.result.current).toEqual({ shown: "b", phase: "opening" });
    act(() => { vi.advanceTimersByTime(300); });
    expect(h.result.current).toEqual({ shown: "b", phase: "open" });
  });

  it("a switch back to the shown panel mid-close cancels the switch", () => {
    const h = setup();
    h.rerender({ t: "a" });
    act(() => { vi.advanceTimersByTime(300); });
    h.rerender({ t: "b" });
    act(() => { vi.advanceTimersByTime(100); });
    h.rerender({ t: "a" });
    expect(h.result.current).toEqual({ shown: "a", phase: "opening" });
    act(() => { vi.advanceTimersByTime(1000); });
    expect(h.result.current).toEqual({ shown: "a", phase: "open" });
  });

  it("uses the (short) reduced-motion duration it is given", () => {
    const h = setup(100);
    h.rerender({ t: "a" });
    act(() => { vi.advanceTimersByTime(100); });
    expect(h.result.current.phase).toBe("open");
    h.rerender({ t: null });
    act(() => { vi.advanceTimersByTime(100); });
    expect(h.result.current.shown).toBeNull();
  });

  it("clears its timer on unmount", () => {
    const h = setup();
    h.rerender({ t: "a" });
    h.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
