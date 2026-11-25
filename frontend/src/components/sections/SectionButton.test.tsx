import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Panel } from "../ui/Panel";
import { SectionButton } from "./SectionButton";

describe("SectionButton", () => {
  it("is one wide link in a new window, named by its title and its line", () => {
    render(<SectionButton description="Verse slides from your notes" href="http://127.0.0.1:8765/versevision/" logo={<svg data-testid="logo" />} testId="b" title="VerseVision" />);
    const link = screen.getByRole("link", { name: "VerseVision Verse slides from your notes (opens in a new window)" });
    expect(link).toHaveAttribute("href", "http://127.0.0.1:8765/versevision/");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
    expect(link).toHaveClass("w-full", "min-h-[4.25rem]", "rounded-ds-lg");
    expect(screen.getByTestId("b")).toBe(link);
    // The logo tile, the words, then the Open pill (decorative: the link's name is the title and its line).
    expect(Array.from(link.children).map((child) => child.getAttribute("data-part"))).toEqual(["logo-tile", "words", "open"]);
    expect(link.querySelector('[data-part="logo-tile"]')).toContainElement(screen.getByTestId("logo"));
    expect(link.querySelector('[data-part="open"]')).toHaveAttribute("aria-hidden", "true");
    expect(link.querySelector('[data-part="title"]')).toHaveClass("text-lg", "font-bold", "text-ink");
  });

  it("needs no description", () => {
    render(<SectionButton href="/x/" title="Other page" />);
    expect(screen.getByRole("link", { name: "Other page (opens in a new window)" })).toBeInTheDocument();
  });

  it("is a flat card, not the feature card of the band beside it, with the Open pill in the connection's colour", () => {
    const { rerender } = render(<SectionButton href="/x/" title="Page" />);
    const link = screen.getByRole("link");
    expect(link).toHaveClass("border", "border-integration-presentation-soft/45");
    expect(link).not.toHaveClass("border-l-[5px]");
    expect(link.querySelector('[data-part="logo-tile"]')).toBeNull();
    expect(link.querySelector('[data-part="glow"]')).toBeNull();
    expect(link.querySelector('[data-part="open"]')).toHaveClass("text-integration-presentation-text");
    rerender(<Panel tone="playback"><SectionButton href="/x/" logo={<svg />} title="Page" /></Panel>);
    expect(screen.getByRole("link").querySelector('[data-part="open"]')).toHaveClass("text-integration-playback-text");
    // The outline and the glow behind the logo wear the connection's colour too.
    expect(screen.getByRole("link")).toHaveClass("border-integration-playback-soft/45");
    expect(screen.getByRole("link").querySelector('[data-part="glow"]')).toHaveClass("bg-integration-playback-soft", "blur-lg");
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
