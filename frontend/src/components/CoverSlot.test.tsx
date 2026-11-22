import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { CoverSlot } from "./CoverSlot";
import { SongCardBody } from "./SongCard";

const SRC = "http://127.0.0.1:8765/api/v1/playback-library/songs/7/artwork?v=abc";
const placeholder = (root: ParentNode = document) => root.querySelector("[data-part=\"cover-placeholder\"]");

describe("CoverSlot: a song card always has a cover slot, the picture or the placeholder", () => {
  it("draws the 28 px picture when there is an address", () => {
    const { container } = render(<CoverSlot src={SRC} />);
    const img = container.querySelector("img")!;
    expect(img).toHaveAttribute("src", SRC);
    expect(img).toHaveAttribute("alt", "");
    expect(img).toHaveAttribute("width", "28");
    expect(img).toHaveClass("h-7", "w-7", "shrink-0", "rounded-ds-md", "bg-surface-900");
    expect(placeholder(container)).toBeNull();
  });

  it("draws a same-size tile with a music note, hidden from screen readers, when there is no picture", () => {
    const { container } = render(<CoverSlot src={null} />);
    const tile = placeholder(container)!;
    expect(tile).toHaveAttribute("aria-hidden", "true");
    expect(tile).toHaveClass("h-7", "w-7", "shrink-0", "rounded-ds-md", "bg-surface-900");
    expect(tile.querySelector("svg")).not.toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("swaps to the tile when the picture does not load (blocked, 404, 503): a state, not a hidden box", () => {
    const { container } = render(<CoverSlot src={SRC} />);
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector("img")).toBeNull();
    expect(placeholder(container)).not.toBeNull();
    expect(container.innerHTML).not.toContain("visibility");
  });

  it("tries a changed picture (another etag in the address) again after a failure, and never loops on the one that failed", () => {
    const { container, rerender } = render(<CoverSlot src={SRC} />);
    fireEvent.error(container.querySelector("img")!);
    rerender(<CoverSlot src={SRC} />);
    expect(placeholder(container)).not.toBeNull();
    rerender(<CoverSlot src={`${SRC.replace("abc", "def")}`} />);
    expect(container.querySelector("img")).toHaveAttribute("src", SRC.replace("abc", "def"));
    expect(placeholder(container)).toBeNull();
  });

  it("is in every song card: the title sits after a 28 px slot with or without a cover", () => {
    const sections = [{ name: "Verse 1", start: 0, end: 20 }];
    const { container, rerender } = render(<SongCardBody cover={null} sections={sections} title="Holy Forever" />);
    expect(placeholder(container)).not.toBeNull();
    rerender(<SongCardBody cover={SRC} sections={sections} title="Holy Forever" />);
    expect(container.querySelector("img")).toHaveAttribute("src", SRC);
    expect(placeholder(container)).toBeNull();
    fireEvent.error(container.querySelector("img")!);
    expect(placeholder(container)).not.toBeNull();
    expect(screen.getByText("Holy Forever")).toBeInTheDocument();
  });
});
