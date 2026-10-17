import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { playbackStatus } from "../test/playbackFixtures";
import type { LengthPreview, PlaybackStatusResponse } from "../types";
import { PlanningCenterLengths } from "./PlanningCenterLengths";
vi.mock("../api", () => ({ getLengthCategories: vi.fn(), chooseLengthCategory: vi.fn(), previewPlanLengths: vi.fn(), confirmPlanLengths: vi.fn(), restorePlanLengths: vi.fn() }));
const ready = (extra: Partial<PlaybackStatusResponse> = {}) => playbackStatus({ planning_center_connected: true, captured_at: "2026-10-07T12:00:00Z", song_order: [1, 2], song_lengths: [268, null], discovery: "done", planning_center_update_service_type_id: "type", ...extra });
const preview: LengthPreview = { token: "preview", category: "Sunday Service", plan_title: "Sunday", plan_date: "2026-10-11", message: "", items: [{ item_id: "one", title: "Worship", old_length: 245, new_length: 268, status: "updated", reason: null }, { item_id: "two", title: "Second", old_length: 240, new_length: null, status: "skipped", reason: "No measured Playback length." }] };
beforeEach(() => { vi.clearAllMocks(); vi.mocked(api.previewPlanLengths).mockResolvedValue(preview); vi.mocked(api.getLengthCategories).mockResolvedValue([{ id: "type", name: "Sunday Service" }, { id: "youth", name: "Youth" }]); });
const update = () => screen.getByRole("button", { name: "Update Services" });
const show = (extra: Partial<PlaybackStatusResponse> = {}) => render(<PlanningCenterLengths status={ready(extra)} />);
describe("explicit Planning Center split button", () => {
  it.each([
    [{ captured_at: null, song_order: [] }, "Scan songs first to read the lengths from Playback."],
    [{ stale: true }, "Your Playback setlist changed. Scan the setlist again before updating Planning Center."],
    [{ plan_changed: true }, "Your Services plan changed since the last scan. Scan the setlist again before updating Planning Center."],
    [{ discovery: "failed" }, "The song scan failed or was cancelled. Scan songs again."],
    [{ song_lengths: [null] }, "The scan has no song lengths. Scan songs again."],
    [{ planning_center_connected: false }, "Connect Services first."],
    [{ playing: true }, "Stop Playback first."],
    [{ discovery: "running" }, "Wait for the song scan to finish."],
  ] as [Partial<PlaybackStatusResponse>, string][])("unavailable with reason %o", (extra, reason) => {
    show(extra); expect(update()).toBeDisabled(); expect(update()).toHaveAttribute("aria-disabled", "true"); expect(screen.getByText(reason)).toBeVisible();
    fireEvent.click(update()); expect(api.previewPlanLengths).not.toHaveBeenCalled(); expect(api.confirmPlanLengths).not.toHaveBeenCalled();
  });
  it("no button press makes no request; preview is explicit, Cancel writes nothing", async () => {
    show(); expect(update()).toBeEnabled(); expect(api.previewPlanLengths).not.toHaveBeenCalled();
    fireEvent.click(update()); const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/Sunday Service · Sunday · 2026-10-11/)).toBeVisible();
    expect(within(dialog).getByText(/4:05 → 4:28/)).toBeVisible(); expect(within(dialog).getByText(/No change/)).toBeVisible();
    const confirm = within(dialog).getByRole("button", { name: "Update Services" }); expect(confirm).toBeEnabled();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" })); expect(api.confirmPlanLengths).not.toHaveBeenCalled();
    // Focus returns from Dialog's cleanup and from the component's busy/returnFocus effect after the dialog unmounts; wait for it instead of reading it synchronously.
    await waitFor(() => expect(update()).toHaveFocus());
  });
  it.each(["Cancel", "Escape"])("%s hands focus back to Update Services only after the dialog has gone", async (action) => {
    const user = userEvent.setup();
    show(); const trigger = update(); await user.click(trigger);
    const dialog = await screen.findByRole("dialog");
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    // Every call to the button's focus(), and whether the dialog was still on screen when it happened.
    const dialogShownAtFocus: boolean[] = [];
    const spy = vi.spyOn(trigger, "focus").mockImplementation(() => {
      dialogShownAtFocus.push(screen.queryByRole("dialog") !== null);
      HTMLElement.prototype.focus.call(trigger);
    });
    try {
      if (action === "Cancel") await user.click(cancel); else await user.keyboard("{Escape}");
      await waitFor(() => expect(trigger).toHaveFocus());
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(dialogShownAtFocus.length).toBeGreaterThan(0);
      expect(dialogShownAtFocus.every((shown) => !shown)).toBe(true);
      expect(api.confirmPlanLengths).not.toHaveBeenCalled();
    } finally { spy.mockRestore(); }
  });
  it("one confirmation covers every measured length; result and Restore appear and restore updates result", async () => {
    vi.mocked(api.previewPlanLengths).mockResolvedValue({ ...preview, items: [{ ...preview.items[0]! }] });
    vi.mocked(api.confirmPlanLengths).mockResolvedValue(ready({ planning_center_undo_available: true, planning_center_lengths: { status: "done", message: "Updated 1 songs in Planning Center: Worship 4:05 -> 4:28. StagePilot reloaded the plan and verified the song times.", items: [], reload: "verified" } }));
    vi.mocked(api.restorePlanLengths).mockResolvedValue(ready({ planning_center_undo_available: false, planning_center_lengths: { status: "done", message: "Restored 1 songs in Planning Center.", items: [], reload: "verified" } }));
    show(); fireEvent.click(update()); const dialog = await screen.findByRole("dialog"); expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Update Services" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument()); expect(api.confirmPlanLengths).toHaveBeenCalledWith("preview");
    expect(screen.getByText(/Updated 1 songs/)).toBeVisible(); fireEvent.click(screen.getByRole("button", { name: "Restore Services times" }));
    await screen.findByText(/Restored 1 songs/); expect(screen.queryByRole("button", { name: "Restore Services times" })).not.toBeInTheDocument();
  });
  it("already matches offers no write action", async () => {
    vi.mocked(api.previewPlanLengths).mockResolvedValue({ ...preview, message: "Planning Center already matches Playback to the second.", items: [{ ...preview.items[0]!, status: "skipped", reason: "Already the same." }] });
    show(); fireEvent.click(update()); const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Planning Center already matches Playback to the second.")).toBeVisible(); expect(within(dialog).queryByRole("button", { name: "Update Services" })).not.toBeInTheDocument();
  });
  it("menu keyboard arrows, Enter persists immediately, Escape returns focus, remembers selected category", async () => {
    const user = userEvent.setup(); const onStatus = vi.fn();
    vi.mocked(api.chooseLengthCategory).mockResolvedValue(ready({ planning_center_update_service_type_id: "youth" }));
    render(<PlanningCenterLengths status={ready()} onStatus={onStatus} />);
    const arrow = screen.getByRole("button", { name: "Choose service plan category" }); await user.click(arrow);
    const menu = await screen.findByRole("menu"); await waitFor(() => expect(within(menu).getAllByRole("menuitemradio")[0]).toHaveFocus());
    await user.keyboard("{ArrowDown}{Enter}"); await waitFor(() => expect(api.chooseLengthCategory).toHaveBeenCalledWith("youth")); await waitFor(() => expect(arrow).toHaveFocus()); expect(onStatus).toHaveBeenCalled();
    await user.click(arrow); await screen.findByRole("menu"); expect(screen.getByRole("menuitemradio", { name: "✓ Youth" })).toHaveAttribute("aria-checked", "true");
    await user.keyboard("{Escape}"); expect(screen.queryByRole("menu")).not.toBeInTheDocument(); expect(arrow).toHaveFocus();
  });
  it("playing or setlist change invalidates an open preview", async () => {
    const view = show(); fireEvent.click(update()); await screen.findByRole("dialog"); view.rerender(<PlanningCenterLengths status={ready({ stale: true })} />); expect(screen.queryByRole("dialog")).not.toBeInTheDocument(); expect(update()).toBeDisabled();
  });
  it.each(["Updated 2 songs in Planning Center.", "Worship: Someone changed this item; their edit was left alone.", "Planning Center did not allow this change. The account or token used by StagePilot can't edit this plan."])("reports result %s", message => { show({ planning_center_lengths: { status: "partial", message, items: [], reload: "not_needed" } }); expect(screen.getByText(message)).toBeVisible(); });
  it("preview errors are visible, no write occurs, and retry works", async () => {
    vi.mocked(api.previewPlanLengths).mockRejectedValueOnce(new Error("No upcoming plan in this category.")); show(); fireEvent.click(update()); await screen.findByRole("alert"); expect(api.confirmPlanLengths).not.toHaveBeenCalled();
    fireEvent.click(update()); await screen.findByRole("dialog");
  });
});

describe("outdated data closes an open preview and never writes", () => {
  it("plan change while the confirm dialog is open closes it; the update action is disabled", async () => {
    const view = show(); fireEvent.click(update()); await screen.findByRole("dialog");
    view.rerender(<PlanningCenterLengths status={ready({ plan_changed: true, song_lengths: [] })} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(); expect(update()).toBeDisabled();
    expect(api.confirmPlanLengths).not.toHaveBeenCalled();
  });
  it("a new scan makes the action available again", async () => {
    const view = show({ plan_changed: true }); expect(update()).toBeDisabled();
    view.rerender(<PlanningCenterLengths status={ready({ plan_changed: false, captured_at: "2026-10-07T13:00:00Z" })} />);
    expect(update()).toBeEnabled();
  });
});

describe("after a successful write the update is not offered again", () => {
  it("lengths_applied disables Update, keeps Restore working, and a new scan brings it back", () => {
    const view = render(<PlanningCenterLengths status={ready({ lengths_applied: true, planning_center_undo_available: true })} />);
    expect(update()).toBeDisabled(); expect(screen.getByText(/already has these lengths/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Restore Services times" })).toBeEnabled();
    view.rerender(<PlanningCenterLengths status={ready({ lengths_applied: false, planning_center_undo_available: true, captured_at: "2026-10-08T12:00:00Z" })} />);
    expect(update()).toBeEnabled();
  });
});
