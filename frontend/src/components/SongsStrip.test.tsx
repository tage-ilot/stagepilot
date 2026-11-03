import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { SongsStrip, type SongChip } from "./SongsStrip";

const chips: SongChip[] = [
  { key: "a", title: "Holy Forever", order: 1, looks: 12, handmade: 0, hasLoudness: true },
  { key: "b", title: "Goodness", order: 2, looks: 0, handmade: 0, hasLoudness: false },
  { key: "c", title: "Hand made only", order: 3, looks: 0, handmade: 2, hasLoudness: false },
];

const mount = (props: Partial<Parameters<typeof SongsStrip>[0]> = {}) => {
  const onSelect = vi.fn();
  const onClear = vi.fn();
  const onClearAll = vi.fn();
  render(<SongsStrip chips={chips} liveKey="a" noun="looks" onClear={onClear} onClearAll={onClearAll} onSelect={onSelect} selectedKey="a" {...props} />);
  return { onSelect, onClear, onClearAll };
};

describe("SongsStrip", () => {
  it("heads the strip with the captured count and a Clear all songs action", async () => {
    const { onClearAll } = mount();
    expect(screen.getByText("Songs in this service - 2 of 3 captured")).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "Clear all songs..." }));
    expect(onClearAll).toHaveBeenCalledTimes(1);
  });

  it("keeps plan order and shows each state", () => {
    mount();
    const list = screen.getByRole("list", { name: "Songs in this service" });
    const items = within(list).getAllByRole("listitem");
    expect(items.map((item) => item.getAttribute("data-song"))).toEqual(["a", "b", "c"]);
    expect(items[0]).toHaveAttribute("data-state", "captured");
    expect(items[0]).toHaveTextContent("Holy Forever");
    expect(items[0]).toHaveTextContent("12 looks");
    expect(within(items[0]!).getByTestId("song-chip-loudness")).toBeInTheDocument();
    expect(items[1]).toHaveAttribute("data-state", "empty");
    expect(items[1]).toHaveTextContent("not captured");
    expect(within(items[1]!).queryByRole("button", { name: /Clear/ })).toBeNull();
    expect(items[2]).toHaveAttribute("data-state", "captured");
    expect(items[2]).toHaveTextContent("2 cues");
  });

  it("marks the live song with a dot and the selected song as pressed", () => {
    mount({ liveKey: "a", selectedKey: "b" });
    expect(screen.getByRole("button", { name: /Holy Forever.*live/ })).toHaveAttribute("aria-pressed", "false");
    expect(within(screen.getByRole("button", { name: /Holy Forever.*live/ })).getByTestId("song-chip-live")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Goodness.*not captured/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getAllByTestId("song-chip-live")).toHaveLength(1);
  });

  it("clicking or pressing Enter on a chip selects that song, captured or not", async () => {
    const { onSelect } = mount();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /^Goodness/ }));
    expect(onSelect).toHaveBeenLastCalledWith("b");
    screen.getByRole("button", { name: /^Hand made only/ }).focus();
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenLastCalledWith("c");
  });

  it("the x beside the title clears only that song, and is its own named button", async () => {
    const { onClear, onSelect } = mount();
    await userEvent.setup().click(screen.getByRole("button", { name: "Clear cues for Holy Forever..." }));
    expect(onClear).toHaveBeenCalledWith("a");
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("has touch targets of at least 44 px even though the chip looks small", () => {
    mount();
    for (const name of [/^Holy Forever/, "Clear cues for Holy Forever..."]) {
      const button = screen.getByRole("button", { name });
      expect(button.className).toContain("min-h-[44px]");
    }
  });

  it("40 songs keep one fixed-height wrapping list that scrolls inside itself", () => {
    const many: SongChip[] = Array.from({ length: 40 }, (_, i) => ({ key: `s${i}`, title: `Song number ${i + 1}`, order: i + 1, looks: i % 2 ? 5 : 0, handmade: 0, hasLoudness: false }));
    mount({ chips: many, liveKey: "s3", selectedKey: "s3" });
    const list = screen.getByRole("list", { name: "Songs in this service" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(40);
    expect(list.className).toContain("flex-wrap");
    expect(list.className).toContain("max-h-");
    expect(list.className).toContain("overflow-y-auto");
    expect(screen.getByText("Songs in this service - 20 of 40 captured")).toBeInTheDocument();
  });

  it("renders nothing without a service plan", () => {
    const { container } = render(<SongsStrip chips={[]} liveKey="" noun="looks" onClear={vi.fn()} onClearAll={vi.fn()} onSelect={vi.fn()} selectedKey="" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("keeps the title whole and lets the count truncate first; a very long title truncates but stays named", () => {
    const long = "A very long song title that goes on and on and on past any sensible chip width";
    mount({ chips: [
      { key: "n", title: "Battle Belongs", order: 1, looks: 12, handmade: 0, hasLoudness: false },
      { key: "l", title: long, order: 2, looks: 3, handmade: 0, hasLoudness: false },
    ] });
    const normal = screen.getByText("Battle Belongs");
    expect(normal.className).toContain("shrink-0");
    expect(normal.className).not.toContain("min-w-0");
    expect(normal.className).toContain("max-w-[16rem]");
    expect(normal.className).toContain("truncate");
    const count = screen.getByText("· 12 looks");
    expect(count.className).toContain("min-w-0");
    expect(count.className).toContain("truncate");
    expect(count.className).not.toContain("shrink-0");
    expect(screen.getByText(long).className).toContain("truncate");
    expect(screen.getByRole("button", { name: new RegExp(`^${long}, 3 looks`) })).toBeInTheDocument();
    expect(screen.getByText(long).closest("li")?.className).toContain("max-w-[28rem]");
  });
});
