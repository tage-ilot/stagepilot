import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import { OptionalBand } from "./OptionalBand";

beforeEach(() => window.localStorage.clear());
const band = (extra: Partial<Parameters<typeof OptionalBand>[0]> = {}) => (
  <OptionalBand actions={<button type="button">Scan</button>} headingId="h" persistKey="test.band" summary="12 songs" summaryTestId="sum" title="Song order" {...extra}>
    <p>Inside</p>
  </OptionalBand>
);

describe("OptionalBand", () => {
  it("is a named section: the toggle (chevron, heading, summary) and the actions on one header row, then the body", () => {
    const { container } = render(band());
    const section = screen.getByRole("region", { name: "Song order" });
    expect(section).toBe(container.firstElementChild);
    const header = section.firstElementChild as HTMLElement;
    expect(header).toHaveAttribute("data-part", "band-header");
    // The actions are beside the toggle, never inside it (a button cannot hold a button).
    const toggle = screen.getByRole("button", { name: /Song order/ });
    expect(toggle).not.toContainElement(screen.getByRole("button", { name: "Scan" }));
    expect(Array.from(header.children).map((child) => child.tagName)).toEqual(["BUTTON", "BUTTON"]);
    expect(screen.getByRole("heading", { name: "Song order" })).toHaveAttribute("id", "h");
    expect(screen.getByTestId("sum")).toHaveTextContent("12 songs");
  });

  it("starts open, folds the body away (inert) and remembers the choice", async () => {
    const user = userEvent.setup();
    const { container, unmount } = render(band());
    const toggle = () => screen.getByRole("button", { name: /Song order/ });
    const body = () => container.querySelector("[data-part=region]") as HTMLElement;
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(toggle()).toHaveAttribute("aria-controls", body().id);
    expect(body()).toHaveAttribute("data-state", "open");
    await user.click(toggle());
    expect(body()).toHaveAttribute("data-state", "closed");
    expect(body().firstElementChild).toHaveAttribute("inert");
    expect(window.localStorage.getItem("test.band")).toBe("false");
    unmount();
    render(band());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
  });

  it("the tab order is the toggle, then the actions", async () => {
    const user = userEvent.setup();
    render(band());
    await user.tab();
    expect(document.activeElement).toHaveTextContent("Song order");
    await user.tab();
    expect(document.activeElement).toHaveTextContent("Scan");
  });

  it("can start folded (defaultOpen={false}) and a saved choice wins over the default", () => {
    const { unmount } = render(band({ defaultOpen: false }));
    expect(screen.getByRole("button", { name: /Song order/ })).toHaveAttribute("aria-expanded", "false");
    unmount();
    window.localStorage.setItem("test.band", "true");
    render(band({ defaultOpen: false }));
    expect(screen.getByRole("button", { name: /Song order/ })).toHaveAttribute("aria-expanded", "true");
  });

  it("forceOpen opens a folded band without touching what the person chose, and tells the page what the toggle asked for", async () => {
    const user = userEvent.setup();
    const asked: boolean[] = [];
    const { container, rerender } = render(band({ defaultOpen: false, forceOpen: true, onOpenChange: (next) => asked.push(next) }));
    const toggle = () => screen.getByRole("button", { name: /Song order/ });
    const body = () => container.querySelector("[data-part=region]") as HTMLElement;
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(body().firstElementChild).not.toHaveAttribute("inert");
    expect(window.localStorage.getItem("test.band")).toBeNull();
    // Pressed while forced open: the page is told it was a request to close; the band stays open while it is forced.
    await user.click(toggle());
    expect(asked).toEqual([false]);
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    // Not forced any more: the person's own choice (the toggle's last press: closed) shows, nothing closed by itself before.
    rerender(band({ defaultOpen: false, forceOpen: false, onOpenChange: (next) => asked.push(next) }));
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
  });

  it("wears the feature card in the Presentation accent: a rail and a wash, like the Song lighting timeline", () => {
    const { container } = render(band({ tone: "presentation" }));
    const card = container.firstElementChild as HTMLElement;
    expect(card.className).toContain("border-l-integration-presentation-soft");
    expect(card.className).toContain("from-integration-presentation-soft");
    expect(card).toHaveAttribute("data-tone", "presentation");
  });

  it("wears the Playback accent by default", () => {
    const { container } = render(band());
    expect((container.firstElementChild as HTMLElement).className).toContain("border-l-integration-playback-soft");
  });
});
