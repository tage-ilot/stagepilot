import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Panel } from "../ui/Panel";
import { SectionButton } from "./SectionButton";

describe("SectionButton", () => {
  it("is one wide link in a new window, named by its title and its line", () => {
    render(<SectionButton description="Verse slides from your notes" href="http://127.0.0.1:8765/versevision/" testId="b" title="VerseVision" />);
    const link = screen.getByRole("link", { name: "VerseVision Verse slides from your notes (opens in a new window)" });
    expect(link).toHaveAttribute("href", "http://127.0.0.1:8765/versevision/");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
    expect(link).toHaveClass("w-full", "min-h-ds-touch-target", "rounded-ds-xl");
    expect(screen.getByTestId("b")).toBe(link);
    expect(Array.from(link.children).map((child) => child.tagName.toLowerCase())).toEqual(["span", "svg"]);
    expect(link.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });

  it("needs no description", () => {
    render(<SectionButton href="/x/" title="Other page" />);
    expect(screen.getByRole("link", { name: "Other page (opens in a new window)" })).toBeInTheDocument();
  });

  it("wears the connection's colour inside its panel and the Presentation colour outside one", () => {
    const { rerender } = render(<SectionButton href="/x/" title="Page" />);
    expect(screen.getByRole("link")).toHaveClass("text-integration-presentation-text");
    rerender(<Panel tone="playback"><SectionButton href="/x/" title="Page" /></Panel>);
    expect(screen.getByRole("link")).toHaveClass("text-integration-playback-text");
  });

  it("leaves the click to the browser unless the page hands it over", async () => {
    const onOpen = vi.fn();
    const { rerender } = render(<SectionButton href="/x/" title="Page" />);
    expect(screen.getByRole("link").onclick).toBeNull();
    rerender(<SectionButton href="/x/" onOpen={onOpen} title="Page" />);
    const click = await userEvent.setup().click(screen.getByRole("link"));
    expect(click).toBeUndefined();
    expect(onOpen).toHaveBeenCalledExactlyOnceWith("/x/");
  });
});
