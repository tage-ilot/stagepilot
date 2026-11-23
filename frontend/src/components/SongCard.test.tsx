import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { SONG_CARD_LOOK, SongCardBody, SongCardShell } from "./SongCard";

/**
 * The colored bar must span the card in the Mac app. WebKit's own style for a `button` is `align-items: flex-start`, so a flex column inside
 * one does not stretch its children unless the class says so. jsdom has no layout, so these tests assert the class that does it.
 */
const sections = [{ name: "Verse 1", start: 0, end: 20 }, { name: "Chorus", start: 20, end: 50 }];
const body = <SongCardBody cover={null} facts={<span>C</span>} sections={sections} title="Holy Forever" />;

describe("SongCardShell", () => {
  it("press mode: one button that presses the song and stretches the bar across the card", async () => {
    const onActivate = vi.fn();
    render(<ul><SongCardShell label="Holy Forever" look={SONG_CARD_LOOK.plain} onActivate={onActivate}>{body}</SongCardShell></ul>);
    const button = screen.getByRole("button", { name: "Holy Forever" });
    await userEvent.click(button);
    expect(onActivate).toHaveBeenCalledTimes(1);
    expect(button).toHaveClass("flex-col", "items-stretch");
    expect(screen.getByTestId("library-bar").parentElement).toBe(button);
  });

  it("check mode: a checkbox that stretches the bar across the card", async () => {
    const onActivate = vi.fn();
    render(<ul><SongCardShell label="Holy Forever" look={SONG_CARD_LOOK.picked} mode="check" onActivate={onActivate} selected>{body}</SongCardShell></ul>);
    const box = screen.getByRole("checkbox", { name: "Holy Forever" });
    await userEvent.click(box);
    expect(onActivate).toHaveBeenCalledTimes(1);
    expect(box).toHaveAttribute("aria-checked", "true");
    expect(box).toHaveClass("flex-col", "items-stretch");
  });

  it("static mode: a div with the same layout classes and no button", () => {
    render(<ul><SongCardShell look={SONG_CARD_LOOK.empty} mode="static">{body}</SongCardShell></ul>);
    expect(screen.queryByRole("button")).toBeNull();
    const card = screen.getByTestId("library-bar").parentElement as HTMLElement;
    expect(card.tagName).toBe("DIV");
    expect(card).toHaveClass("flex-col", "items-stretch");
  });

  it("a button beside the card sits in its corner over the card, so the bar still runs the whole width", async () => {
    const onClear = vi.fn();
    render(<ul><SongCardShell after={<button aria-label="Clear" onClick={onClear} type="button">×</button>} label="Holy Forever" look={SONG_CARD_LOOK.plain}>{body}</SongCardShell></ul>);
    const button = screen.getByRole("button", { name: "Holy Forever" });
    const corner = screen.getByRole("button", { name: "Clear" }).parentElement as HTMLElement;
    expect(corner).toHaveClass("absolute", "right-0", "top-0");
    expect(button.parentElement).toContainElement(corner);
    expect(button).not.toContainElement(corner);
    expect(button).toHaveClass("flex-1", "[&>span:first-child]:pr-9");
    expect(screen.getByTestId("library-bar").parentElement).toBe(button);
    await userEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(onClear).toHaveBeenCalledTimes(1);
  });
});
