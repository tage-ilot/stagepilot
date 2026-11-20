import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError, pinServicePlan, unpinServicePlan } from "../api";
import { PLANS, stateFor } from "../test/servicePlanFixtures";
import type { PlanSelectionResponse } from "../types";
import { useServicePlanChoice } from "./useServicePlanChoice";

vi.mock("../api", async () => {
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return { ...actual, pinServicePlan: vi.fn(), unpinServicePlan: vi.fn() };
});

const pin = vi.mocked(pinServicePlan);
const unpin = vi.mocked(unpinServicePlan);

const answer = (index: number, pinned: boolean): PlanSelectionResponse => ({
  accepted: true,
  message: "ok",
  state: stateFor(PLANS[index]!, pinned),
});

/** A request that finishes when the test says so. */
function slow<T>() {
  let finish!: (value: T) => void;
  let fail!: (cause: unknown) => void;
  const promise = new Promise<T>((resolve, reject) => { finish = resolve; fail = reject; });
  return { promise, finish, fail };
}

beforeEach(() => {
  pin.mockReset();
  unpin.mockReset();
});

describe("useServicePlanChoice", () => {
  it("loads the picked service, applies the state and announces it politely", async () => {
    pin.mockResolvedValue(answer(1, true));
    const apply = vi.fn();
    const { result } = renderHook(() => useServicePlanChoice(true, apply));
    await act(async () => { result.current.pick("p-nov-29"); });
    expect(pin).toHaveBeenCalledWith("p-nov-29");
    expect(apply).toHaveBeenCalledWith(expect.objectContaining({ selection: expect.objectContaining({ pinned: true }) }));
    expect(result.current.announcement).toBe("Loaded Advent Sunday, 29 Nov");
    expect(result.current.pending).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("goes back to the upcoming service and announces what loaded", async () => {
    unpin.mockResolvedValue(answer(0, false));
    const { result } = renderHook(() => useServicePlanChoice(true, vi.fn()));
    await act(async () => { result.current.back(); });
    expect(unpin).toHaveBeenCalledTimes(1);
    expect(result.current.announcement).toBe("Loaded Sunday Service, 22 Nov");
  });

  it("is pending while it loads and never has two requests going", async () => {
    const first = slow<PlanSelectionResponse>();
    pin.mockReturnValueOnce(first.promise);
    const { result } = renderHook(() => useServicePlanChoice(true, vi.fn()));
    act(() => { result.current.pick("p-nov-29"); });
    expect(result.current.pending).toBe(true);
    // A double click: the same choice again does not start a second request.
    act(() => { result.current.pick("p-nov-29"); });
    expect(pin).toHaveBeenCalledTimes(1);
    pin.mockResolvedValue(answer(1, true));
    await act(async () => { first.finish(answer(1, true)); });
    expect(result.current.pending).toBe(false);
    expect(pin).toHaveBeenCalledTimes(1);
  });

  it("does not load again what was just asked for again after another choice was waiting", async () => {
    const first = slow<PlanSelectionResponse>();
    pin.mockReturnValueOnce(first.promise);
    const { result } = renderHook(() => useServicePlanChoice(true, vi.fn()));
    act(() => { result.current.pick("p-nov-29"); });
    act(() => { result.current.pick("p-dec-06"); });
    // Changing their mind back to what is loading cancels the waiting choice.
    act(() => { result.current.pick("p-nov-29"); });
    await act(async () => { first.finish(answer(1, true)); });
    expect(pin.mock.calls.map((c) => c[0])).toEqual(["p-nov-29"]);
  });

  it("lets the last choice win: fast clicks make one request now and only the newest one after", async () => {
    const first = slow<PlanSelectionResponse>();
    pin.mockReturnValueOnce(first.promise).mockResolvedValue(answer(3, true));
    const apply = vi.fn();
    const { result } = renderHook(() => useServicePlanChoice(true, apply));
    act(() => { result.current.pick("p-nov-29"); });
    act(() => { result.current.pick("p-dec-06"); });
    act(() => { result.current.pick("p-dec-13"); });
    expect(pin).toHaveBeenCalledTimes(1);
    await act(async () => { first.finish(answer(1, true)); });
    expect(pin.mock.calls.map((c) => c[0])).toEqual(["p-nov-29", "p-dec-13"]);
    expect(apply).toHaveBeenLastCalledWith(expect.objectContaining({ plan: expect.objectContaining({ id: "p-dec-13" }) }));
    expect(result.current.announcement).toBe("Loaded Family Sunday, 13 Dec");
    expect(result.current.pending).toBe(false);
  });

  it("lets Back to upcoming be the last choice too", async () => {
    const first = slow<PlanSelectionResponse>();
    pin.mockReturnValueOnce(first.promise);
    unpin.mockResolvedValue(answer(0, false));
    const { result } = renderHook(() => useServicePlanChoice(true, vi.fn()));
    act(() => { result.current.pick("p-dec-06"); });
    act(() => { result.current.back(); });
    await act(async () => { first.finish(answer(2, true)); });
    expect(unpin).toHaveBeenCalledTimes(1);
  });

  it("says why a pick failed, in the server's words, and recovers on the next try", async () => {
    pin.mockRejectedValueOnce(new ApiError("Planning Center did not answer in time.", 503));
    const apply = vi.fn();
    const { result } = renderHook(() => useServicePlanChoice(true, apply));
    await act(async () => { result.current.pick("p-nov-29"); });
    expect(result.current.error).toBe("Planning Center did not answer in time.");
    expect(result.current.announcement).toBe("");
    expect(apply).not.toHaveBeenCalled();
    expect(result.current.pending).toBe(false);
    pin.mockResolvedValue(answer(1, true));
    await act(async () => { result.current.pick("p-nov-29"); });
    expect(result.current.error).toBeNull();
  });

  it("never asks for a session that can only look", async () => {
    const { result } = renderHook(() => useServicePlanChoice(false, vi.fn()));
    await act(async () => { result.current.pick("p-nov-29"); result.current.back(); });
    expect(pin).not.toHaveBeenCalled();
    expect(unpin).not.toHaveBeenCalled();
  });
});
