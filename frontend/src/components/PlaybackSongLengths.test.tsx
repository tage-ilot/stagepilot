import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { playbackStatus } from "../test/playbackFixtures";
import type { PlaybackStatusResponse } from "../types";
import { PlaybackSongLengths } from "./PlaybackSongLengths";

const song = (title: string, duration_seconds: number | null) => ({ title, duration_seconds });
const show = (extra: Partial<PlaybackStatusResponse> = {}) => render(<PlaybackSongLengths status={playbackStatus({
  song_lengths: [180, 240], plan_songs: [song("First song", 180), song("Second song", 240)], ...extra,
})} />);
const rows = () => within(screen.getByRole("list", { name: "Song length comparisons" })).getAllByRole("listitem");

describe("Playback song lengths", () => {
  it("compares equal lengths by position with plan titles and no difference note", () => {
    show();
    expect(rows()).toHaveLength(2);
    expect(rows()[0]).toHaveTextContent("1. First song Plan: 3:00Playback: 3:00");
    expect(rows()[1]).toHaveTextContent("2. Second song Plan: 4:00Playback: 4:00");
    expect(screen.queryByText(/differs by/)).not.toBeInTheDocument();
    expect(screen.getByText("Playback lengths are read from where each song ends (the start of its last measure).")).toBeVisible();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("keeps extra plan positions when the plan is longer", () => {
    show({ song_lengths: [180], plan_songs: [song("First", 180), song("Extra plan song", 65)] });
    expect(rows()).toHaveLength(2);
    expect(rows()[1]).toHaveTextContent("2. Extra plan song Plan: 1:05");
    expect(within(rows()[1]!).queryByText(/^Playback:/)).not.toBeInTheDocument();
    expect(screen.queryByText(/differs by/)).not.toBeInTheDocument();
  });

  it("keeps extra Playback positions when Playback is longer", () => {
    show({ plan_songs: [song("First", 180)], song_lengths: [180, 245] });
    expect(rows()).toHaveLength(2);
    expect(rows()[1]).toHaveTextContent("2. No service plan songPlayback: 4:05");
    expect(within(rows()[1]!).queryByText(/^Plan:/)).not.toBeInTheDocument();
    expect(screen.queryByText(/differs by/)).not.toBeInTheDocument();
  });

  it.each([
    [180, 170, "differs by 10 s"], [180, 190, "differs by 10 s"], [180, 180, null],
    [180, 158, "differs by 22 s"], [180, 202, "differs by 22 s"],
    [180, 169.5, "differs by 10 s"],
  ])("notes every unequal whole-second value (%s/%s)", (plan, playback, note) => {
    show({ plan_songs: [song("First", plan)], song_lengths: [playback] });
    if (note) expect(screen.getByText(note)).toBeVisible();
    else expect(screen.queryByText(/differs by/)).not.toBeInTheDocument();
  });

  it("null means unknown, not zero or a difference", () => {
    show({ song_lengths: [null, 0], plan_songs: [song("Unknown Playback", 180), song("Unknown plan", null)] });
    expect(rows()[0]).toHaveTextContent("Playback: Unknown");
    expect(rows()[1]).toHaveTextContent("Plan: UnknownPlayback: 0:00");
    expect(screen.queryByText(/differs by/)).not.toBeInTheDocument();
  });

  it.each([
    { song_lengths: [] }, { song_lengths: undefined }, { stale: true }, { song_order: [] },
  ])("does not show an empty or stale comparison %o", (extra) => {
    show(extra);
    expect(screen.queryByRole("group", { name: "Song lengths" })).not.toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });

  it("uses wrapping single-column phone rows and only adds desktop columns at sm", () => {
    show({ plan_songs: [song("Averylongunbrokentitle".repeat(12), 180)] });
    expect(screen.getByRole("group", { name: "Song lengths" })).toHaveClass("min-w-0");
    for (const row of rows()) {
      expect(row).toHaveClass("min-w-0", "grid-cols-1", "sm:grid-cols-[minmax(0,1fr)_auto]");
      expect(row.firstElementChild).toHaveClass("min-w-0", "break-words");
      expect(within(row).getByText(/^Playback:/)).toHaveClass("min-w-0", "break-words");
      expect(row.className).not.toMatch(/whitespace-nowrap|min-w-[1-9]/);
    }
  });
});
