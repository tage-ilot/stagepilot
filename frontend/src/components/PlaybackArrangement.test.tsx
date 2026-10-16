import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import type { PlaybackController } from "../hooks/usePlaybackInput";
import { playbackStatus } from "../test/playbackFixtures";
import type { PlaybackStatusResponse } from "../types";
import { PlaybackApiPanel } from "./PlaybackApiPanel";
import { PlaybackSaveBar, usePlaybackDraft } from "./PlaybackSaveFooter";
import { usePlaybackSave, type PlaybackMidiStep } from "./usePlaybackSave";

const noMidi: PlaybackMidiStep = { pending: false, count: 0, valid: true, snapshot: "", apply: async () => true, revert: () => undefined };
// The owner's example: five plan songs and five Playback songs. Plan song 4 (acoustic) has no track, and
// Playback song 5 is background music that is not in the service.
const TITLES = ["One Thing Remains", "So Much", "Easy To Worship", "I Sing Praises", "Great Are You Lord"];
const PLAN = TITLES.map((title, i) => ({ id: `p${i + 1}`, title, duration_seconds: [443, 309, 539, 150, 295][i]! }));
const base = (extra: Partial<PlaybackStatusResponse> = {}) => playbackStatus({
  song_order: [900, 17, 42, 55, 70], song_count: 5, plan_song_count: 5,
  song_lengths: [438, 312, 535, 298, 606], plan_songs: PLAN, skipped_song_ids: [], skipped_plan_song_ids: [], ...extra,
});

function Host({ initial, saveArrangement }: { initial?: Partial<PlaybackStatusResponse>; saveArrangement?: PlaybackController["saveArrangement"] }) {
  const [status, setStatus] = useState(base(initial));
  const playback: PlaybackController = {
    status, events: [], error: null, message: null, pending: null,
    save: vi.fn(async () => true), selectSource: vi.fn(async () => true), scan: vi.fn(), cancelScan: vi.fn(), discover: vi.fn(),
    saveArrangement: async (playbackSkips, planSkips) => {
      const ok = saveArrangement ? await saveArrangement(playbackSkips, planSkips) : true;
      if (ok) setStatus((current) => ({ ...current, skipped_song_ids: playbackSkips, skipped_plan_song_ids: planSkips }));
      return ok;
    },
  };
  const draft = usePlaybackDraft(playback);
  const bar = usePlaybackSave(draft, playback, noMidi);
  return <><PlaybackApiPanel playback={playback} draft={draft} /><PlaybackSaveBar save={bar} /></>;
}

const pairs = () => within(screen.getByRole("list", { name: "Song pairs" })).getAllByRole("listitem");
const skip = (name: string) => screen.getByRole("switch", { name });
const summary = () => screen.getByTestId("arrangement-summary").textContent;

describe("Song arrangement", () => {
  it("shows every song once, in order, as linked pairs with how far apart the lengths are", () => {
    render(<Host />);
    expect(pairs()).toHaveLength(5);
    expect(pairs()[0]).toHaveTextContent("Song 1 in plan7:23One Thing Remains");
    expect(pairs()[0]).toHaveTextContent("Song 1 in Playback7:18Playback song 1");
    expect(pairs()[0]).toHaveTextContent("5 s apart");
    expect(screen.getByTestId("arrangement-badge")).toHaveTextContent("2 to check");
    expect(summary()).toContain("5 of 5 service songs are linked");
  });

  it("flags a pair whose lengths are far apart as probably a different song, never silently fixing it", () => {
    render(<Host />);
    expect(pairs()[3]).toHaveTextContent("I Sing Praises");
    expect(pairs()[3]).toHaveTextContent("Probably a different song");
    expect(pairs()[4]).toHaveTextContent("Probably a different song");
    expect(screen.getByTestId("arrangement-badge")).toHaveTextContent("2 to check");
  });

  it("the owner's example: skip plan song 4 and Playback song 5 and everything lines up", async () => {
    const user = userEvent.setup();
    render(<Host />);
    await user.click(skip("Skip I Sing Praises"));
    await user.click(skip("Skip Playback song 5"));
    expect(pairs()).toHaveLength(6);
    expect(summary()).toContain("4 of 5 service songs are linked");
    expect(summary()).toContain("1 service song has no track");
    expect(summary()).toContain("1 Playback song is not in this service");
    // The fifth plan song now pairs with the fourth Playback song.
    expect(pairs()[4]).toHaveTextContent("Song 5 in plan4:55Great Are You Lord");
    expect(pairs()[4]).toHaveTextContent("Song 4 in Playback");
    expect(pairs()[4]).toHaveTextContent("3 s apart");
    expect(screen.getByTestId("arrangement-badge")).toHaveTextContent("Lines up");
  });

  it("marks a skipped song with the reason, dashed, and keeps its switch", async () => {
    const user = userEvent.setup();
    render(<Host />);
    await user.click(skip("Skip I Sing Praises"));
    await user.click(skip("Skip Playback song 5"));
    expect(within(pairs()[3]!).getByText("No track")).toBeVisible();
    expect(within(pairs()[5]!).getByText("Not in this service")).toBeVisible();
    expect(pairs()[3]!.querySelector("[data-skipped]")).toHaveClass("border-dashed");
    expect(skip("Skip I Sing Praises")).toBeChecked();
  });

  it("each switch is just the word Skip, in the bottom right of its box", () => {
    render(<Host />);
    const box = pairs()[0]!.querySelector(".arr-plan")!;
    const row = within(box as HTMLElement).getByRole("switch").closest("label")!;
    expect(row).toHaveTextContent(/^Skip$/);
    expect(row).toHaveClass("self-end", "mt-auto");
    expect(screen.getAllByRole("switch")).toHaveLength(10);
  });

  it("holds the choice until Save configuration, then saves both lists and settles", async () => {
    const user = userEvent.setup();
    const saveArrangement = vi.fn(async () => true);
    render(<Host saveArrangement={saveArrangement} />);
    expect(screen.getByRole("button", { name: "Save configuration" })).toHaveAttribute("aria-disabled", "true");
    await user.click(skip("Skip I Sing Praises"));
    await user.click(skip("Skip Playback song 5"));
    expect(saveArrangement).not.toHaveBeenCalled();
    expect(document.querySelector("[data-held-marker]")).toHaveTextContent("Not saved yet");
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "held");
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    expect(saveArrangement).toHaveBeenCalledTimes(1);
    expect(saveArrangement).toHaveBeenCalledWith([70], ["p4"]);
    await act(async () => undefined);
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "clean");
    expect(skip("Skip I Sing Praises")).toBeChecked();
  });

  it("Discard puts the saved arrangement back", async () => {
    const user = userEvent.setup();
    render(<Host initial={{ skipped_plan_song_ids: ["p4"], skipped_song_ids: [70] }} />);
    expect(skip("Skip I Sing Praises")).toBeChecked();
    await user.click(skip("Skip I Sing Praises"));
    await user.click(skip("Skip Playback song 5"));
    expect(skip("Skip I Sing Praises")).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: /discard/i }));
    expect(skip("Skip I Sing Praises")).toBeChecked();
    expect(skip("Skip Playback song 5")).toBeChecked();
  });

  it("a failed save keeps the choices and says so, without hiding the bar", async () => {
    const user = userEvent.setup();
    render(<Host saveArrangement={async () => false} />);
    await user.click(skip("Skip I Sing Praises"));
    await user.click(screen.getByRole("button", { name: "Save configuration" }));
    await act(async () => undefined);
    expect(skip("Skip I Sing Praises")).toBeChecked();
    expect(screen.getByRole("alert")).toHaveTextContent(/song arrangement was not saved/);
  });

  it("warns when a skipped plan song is gone from the plan, and Forget it clears the skip as a held change", async () => {
    const user = userEvent.setup();
    render(<Host initial={{ skipped_plan_song_ids: ["p9"] }} />);
    expect(screen.getByTestId("arrangement-stale")).toHaveTextContent("no longer in the service plan");
    await user.click(screen.getByRole("button", { name: "Forget it" }));
    expect(screen.queryByTestId("arrangement-stale")).not.toBeInTheDocument();
    expect(document.querySelector("[data-save-bar]")).toHaveAttribute("data-state", "held");
  });

  it("says a song is left alone when nothing is left to pair with it", async () => {
    const user = userEvent.setup();
    render(<Host />);
    await user.click(skip("Skip One Thing Remains"));
    expect(pairs().at(-1)).toHaveTextContent("No service song left to pair with.");
    expect(screen.getByTestId("arrangement-badge")).toHaveTextContent("Does not line up");
  });

  it("shows nothing without a loaded service plan, since there is nothing to arrange", () => {
    render(<Host initial={{ plan_songs: [] }} />);
    expect(screen.queryByRole("group", { name: "Song arrangement" })).not.toBeInTheDocument();
  });

  it("shows nothing before a scan, or when the saved order is out of date", () => {
    const { rerender } = render(<Host initial={{ song_order: [] }} />);
    expect(screen.queryByRole("group", { name: "Song arrangement" })).not.toBeInTheDocument();
    rerender(<Host initial={{ stale: true }} key="stale" />);
    expect(screen.queryByRole("group", { name: "Song arrangement" })).not.toBeInTheDocument();
  });

  it("stacks by its own width, keeps plan boxes left and Playback boxes right, and never overflows", () => {
    render(<Host />);
    expect(screen.getByRole("group", { name: "Song arrangement" })).toHaveClass("song-arrangement", "min-w-0");
    expect(pairs()[0]!.querySelector(".arr-plan")).toBeInTheDocument();
    expect(pairs()[0]!.querySelector(".arr-pb")).toBeInTheDocument();
    for (const li of pairs()) expect(li).toHaveClass("arr-line");
  });
});
