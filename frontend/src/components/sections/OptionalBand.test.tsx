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
});
