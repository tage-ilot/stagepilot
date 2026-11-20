import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { OptionalSection } from "./OptionalSection";

const toggle = (name: RegExp | string) => screen.getByRole("button", { name });
const region = (container: HTMLElement) => container.querySelector("[data-part=region]") as HTMLElement;

beforeEach(() => window.localStorage.clear());

describe("OptionalSection", () => {
  it("is one closed row (the Disclosure the app has) with aria-expanded and aria-controls, closed by default", () => {
    const { container } = render(<OptionalSection summary="Port 8765" title="Advanced" tone="backend"><input aria-label="Port" /></OptionalSection>);
    const button = toggle(/Advanced/);
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(button).toHaveAttribute("aria-controls", region(container).id);
    expect(container.firstElementChild).toHaveAttribute("data-variant", "advanced");
    // The closed body is out of the tab order and the accessibility tree.
    expect(region(container).firstElementChild).toHaveAttribute("inert");
    expect(screen.getByText("Port 8765")).toBeInTheDocument();
  });

  it("opens and closes with the button and with the keyboard", async () => {
    const user = userEvent.setup();
    render(<OptionalSection title="Advanced" tone="backend"><input aria-label="Port" /></OptionalSection>);
    toggle(/Advanced/).focus();
    await user.keyboard("{Enter}");
    expect(toggle(/Advanced/)).toHaveAttribute("aria-expanded", "true");
    await user.keyboard(" ");
    expect(toggle(/Advanced/)).toHaveAttribute("aria-expanded", "false");
  });

  it("a persisted section starts as its default, remembers the person's choice and starts that way next time", async () => {
    const user = userEvent.setup();
    const first = render(<OptionalSection defaultOpen persistKey="test.optional" title="Remote access" variant="activity"><p>Inside</p></OptionalSection>);
    expect(toggle(/Remote access/)).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle(/Remote access/));
    expect(window.localStorage.getItem("test.optional")).toBe("false");
    first.unmount();
    render(<OptionalSection defaultOpen persistKey="test.optional" title="Remote access" variant="activity"><p>Inside</p></OptionalSection>);
    expect(toggle(/Remote access/)).toHaveAttribute("aria-expanded", "false");
  });

  it("falls back to the default when storage holds something else", () => {
    window.localStorage.setItem("test.optional", "maybe");
    render(<OptionalSection defaultOpen persistKey="test.optional" title="Remote access" variant="activity"><p>Inside</p></OptionalSection>);
    expect(toggle(/Remote access/)).toHaveAttribute("aria-expanded", "true");
  });

  it("forceOpen opens it while something needs the person, without rewriting what they chose", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<OptionalSection persistKey="test.optional" title="Remote access" variant="activity"><p>Inside</p></OptionalSection>);
    expect(toggle(/Remote access/)).toHaveAttribute("aria-expanded", "false");
    rerender(<OptionalSection forceOpen persistKey="test.optional" title="Remote access" variant="activity"><p>Inside</p></OptionalSection>);
    expect(toggle(/Remote access/)).toHaveAttribute("aria-expanded", "true");
    expect(window.localStorage.getItem("test.optional")).toBeNull();
    rerender(<OptionalSection persistKey="test.optional" title="Remote access" variant="activity"><p>Inside</p></OptionalSection>);
    expect(toggle(/Remote access/)).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle(/Remote access/));
    expect(window.localStorage.getItem("test.optional")).toBe("true");
  });

  it("a page that holds the state itself controls it (opening on an invalid field) and is told of every change", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const { rerender } = render(<OptionalSection onOpenChange={onOpenChange} open={false} title="Advanced" tone="backend"><p>Inside</p></OptionalSection>);
    await user.click(toggle(/Advanced/));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    expect(toggle(/Advanced/)).toHaveAttribute("aria-expanded", "false");
    rerender(<OptionalSection onOpenChange={onOpenChange} open title="Advanced" tone="backend"><p>Inside</p></OptionalSection>);
    expect(toggle(/Advanced/)).toHaveAttribute("aria-expanded", "true");
  });

  it("defaultOpen opens a plain one at first, and it is then the person's", async () => {
    const user = userEvent.setup();
    render(<OptionalSection defaultOpen title="Cue board" tone="lighting"><p>Inside</p></OptionalSection>);
    expect(toggle(/Cue board/)).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle(/Cue board/));
    expect(toggle(/Cue board/)).toHaveAttribute("aria-expanded", "false");
  });

  it("is the three kinds the app has: advanced, activity (with its count) and feature (with its detail and tag)", () => {
    const { container, rerender } = render(<OptionalSection title="Recent activity" variant="activity" count={12}><p>Inside</p></OptionalSection>);
    expect(container.firstElementChild).toHaveAttribute("data-variant", "activity");
    expect(screen.getByText("12 events")).toBeInTheDocument();
    rerender(<OptionalSection aside={<span>OSC</span>} detail="16 cues" title="Song lighting timeline" tone="lighting" variant="feature"><p>Inside</p></OptionalSection>);
    expect(container.firstElementChild).toHaveAttribute("data-variant", "feature");
    expect(screen.getByText("16 cues")).toBeInTheDocument();
    expect(screen.getByText("OSC")).toBeInTheDocument();
  });

  it("passes a test id and class to the row", () => {
    render(<OptionalSection className="mt-4" testId="opt" title="Advanced" tone="backend"><p>Inside</p></OptionalSection>);
    expect(screen.getByTestId("opt")).toHaveClass("mt-4");
  });
});
