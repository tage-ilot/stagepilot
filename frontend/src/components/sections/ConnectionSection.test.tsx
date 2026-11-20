import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { ConnectionSection, type ConnectionSectionProps } from "./ConnectionSection";
import { LiveLinkPill } from "./LiveLinkPill";

/** The DOM contract of the Connection section: what every Live Link block (Lighting, Playback, Presentation) is built from. */
const base = (over: Partial<ConnectionSectionProps> = {}): ConnectionSectionProps => ({
  title: "Test Live Link",
  pills: <LiveLinkPill name="" testId="pill-a" text="A: Connected" tone="success" />,
  connection: <div data-testid="row"><input aria-label="Host" /><input aria-label="Port" /><button type="button">Connect</button></div>,
  status: [{ text: "A: Connected.", testId: "line-a", tone: "success" }, { text: "B: Not installed.", testId: "line-b", tone: "idle" }],
  details: <div data-testid="details-content"><button type="button">Inside</button></div>,
  detailsLabel: "Link setup",
  testId: "section",
  ...over,
});
const drawn = (over: Partial<ConnectionSectionProps> = {}) => render(<ConnectionSection {...base(over)} />);
const follows = (a: Element, b: Element) => Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
const toggle = () => screen.getByRole("button", { name: /Link setup/ });

describe("ConnectionSection: the order never changes", () => {
  it("is title and pills, the connection row, ONE status line, the Details button, then the details", () => {
    drawn();
    const section = screen.getByTestId("section");
    const heading = within(section).getByRole("heading", { name: "Test Live Link" });
    const pill = screen.getByTestId("pill-a");
    const row = screen.getByTestId("row");
    const line = section.querySelector("[data-part=status]") as HTMLElement;
    const button = toggle();
    const details = screen.getByTestId("live-link-details");
    expect([heading, pill, row, line, button, details].every((node, index, all) => index === 0 || follows(all[index - 1] as Element, node))).toBe(true);
    // The title is an h3 with the pill in the same header row; everything else is a direct child of the section, in that order.
    expect(heading.tagName).toBe("H3");
    expect(heading.parentElement).toContainElement(pill);
    expect(Array.from(section.children).map((child) => child.getAttribute("data-part") ?? "header")).toEqual(["header", "connection", "status-row", "details"]);
  });

  it("puts quick actions beside the Details button (before it), their boxes under the status row, and the footer last", () => {
    drawn({ actions: <button type="button">Release</button>, panels: <p data-testid="panel">box</p>, footer: <p data-testid="footer">saving</p> });
    const actions = screen.getByTestId("live-link-actions");
    expect(within(actions).getAllByRole("button").map((button) => button.textContent?.replace(/[\u25B8\u25BE]/g, ""))).toEqual(["Release", "Link setup"]);
    const order = [screen.getByTestId("live-link-sub"), screen.getByTestId("panel"), screen.getByTestId("live-link-details"), screen.getByTestId("footer")];
    expect(order.every((node, index) => index === 0 || follows(order[index - 1] as Element, node))).toBe(true);
  });

  it("the tab order follows the DOM: connection controls, then the quick actions, then the Details button, then what is inside", async () => {
    drawn({ actions: <button type="button">Release</button> });
    const user = userEvent.setup();
    await user.click(toggle());
    (document.activeElement as HTMLElement).blur();
    const seen: string[] = [];
    for (let i = 0; i < 6; i += 1) {
      await user.tab();
      seen.push((document.activeElement as HTMLElement).getAttribute("aria-label") ?? document.activeElement?.textContent?.replace(/[\u25B8\u25BE]/g, "") ?? "");
    }
    expect(seen.slice(0, 6)).toEqual(["Host", "Port", "Connect", "Release", "Link setup", "Inside"]);
  });

  it("is a bordered, labelled block with the shared look (a design change is made once, here)", () => {
    drawn({ className: "!mt-0" });
    const section = screen.getByRole("region", { name: "Test Live Link" });
    expect(section.tagName).toBe("SECTION");
    expect(section).toHaveClass("live-link", "rounded-ds-xl", "border", "bg-surface-panel-strong", "!mt-0");
  });
});

describe("ConnectionSection: the Details", () => {
  it("are collapsed by default and the button says so (aria-expanded, aria-controls, the arrow)", () => {
    drawn();
    const details = screen.getByTestId("live-link-details");
    expect(details).toHaveAttribute("hidden");
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(toggle()).toHaveAttribute("aria-controls", details.id);
    expect(toggle().querySelector("[data-part=arrow]")).toHaveTextContent("\u25B8");
    expect(screen.queryByRole("button", { name: "Inside" })).toBeNull();
  });

  it("open and close with the button, which shows the arrow down while open", async () => {
    drawn();
    await userEvent.setup().click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(toggle().querySelector("[data-part=arrow]")).toHaveTextContent("\u25BE");
    expect(screen.getByTestId("live-link-details")).toBeVisible();
    expect(screen.getByRole("button", { name: "Inside" })).toBeVisible();
    await userEvent.setup().click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
  });

  it("open by themselves only when attention is true: at the start, and again whenever it turns true", async () => {
    const { rerender } = drawn({ attention: true });
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    // The person closes them: that is kept while attention stays true...
    await userEvent.setup().click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    rerender(<ConnectionSection {...base({ attention: true })} />);
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    // ...and attention going away and coming back opens them again.
    rerender(<ConnectionSection {...base({ attention: false })} />);
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    rerender(<ConnectionSection {...base({ attention: true })} />);
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
  });

  it("close by themselves when attention goes away, unless the person opened them", async () => {
    const { rerender } = drawn({ attention: true });
    rerender(<ConnectionSection {...base({ attention: false })} />);
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    await userEvent.setup().click(toggle());
    rerender(<ConnectionSection {...base({ attention: true })} />);
    rerender(<ConnectionSection {...base({ attention: false })} />);
    // Attention turned true again, which gave the choice back to attention; now it is gone, so closed.
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
  });

  it("tell a function child whether they are open and whether the person opened them", async () => {
    const seen: string[] = [];
    drawn({ details: ({ open, byChoice }) => { seen.push(`${open}/${byChoice}`); return <p>d</p>; }, attention: true });
    expect(seen.at(-1)).toBe("true/false");
    await userEvent.setup().click(toggle());
    expect(seen.at(-1)).toBe("false/false");
    await userEvent.setup().click(toggle());
    expect(seen.at(-1)).toBe("true/true");
  });

  it("draw no button without a label, and none (and no area) without details", () => {
    const { unmount } = drawn({ detailsLabel: null });
    expect(screen.queryByRole("button", { name: /setup/i })).toBeNull();
    expect(screen.getByTestId("live-link-details")).toBeInTheDocument();
    unmount();
    drawn({ details: undefined });
    expect(screen.queryByTestId("live-link-details")).toBeNull();
    expect(screen.queryByRole("button", { name: /Link setup/ })).toBeNull();
  });
});

describe("ConnectionSection: the one status line", () => {
  it("is a polite live region that joins each connection's words with a middle dot", () => {
    drawn();
    const line = screen.getByTestId("live-link-sub").querySelector("[data-part=status]") as HTMLElement;
    expect(line.tagName).toBe("P");
    expect(line).toHaveAttribute("role", "status");
    expect(line).toHaveAttribute("aria-live", "polite");
    expect(line).toHaveTextContent("A: Connected. \u00B7 B: Not installed.");
    expect(Array.from(line.querySelectorAll("[aria-hidden=true]")).map((dot) => dot.textContent)).toEqual([" \u00B7 "]);
  });

  it("keeps each connection's own test id and tone, stacked on a narrow block and inline on a wide one", () => {
    drawn();
    expect(screen.getByTestId("line-a")).toHaveAttribute("data-tone", "success");
    expect(screen.getByTestId("line-b")).toHaveAttribute("data-tone", "idle");
    for (const id of ["line-a", "line-b"]) expect(screen.getByTestId(id)).toHaveClass("block", "sm:inline");
    expect(screen.getByTestId("live-link-sub").querySelector("[data-part=status] [aria-hidden=true]")).toHaveClass("hidden", "sm:inline");
  });

  it("drops empty lines, so no stray dot", () => {
    drawn({ status: [{ text: "A: Connected." }, { text: null }, { text: "" }] });
    expect(screen.getByTestId("live-link-sub").querySelector("[data-part=status] [aria-hidden=true]")).toBeNull();
  });

  it("can carry the test id on the line itself (the Lighting headline)", () => {
    drawn({ status: [{ text: "Auto: replaying." }], statusTestId: "headline" });
    expect(screen.getByTestId("headline").tagName).toBe("P");
    expect(screen.getByTestId("headline")).toHaveClass("flex-[1_1_16rem]");
  });

  it("wraps on a narrow block: the row, the line (basis 16rem) and the group of actions may all wrap", () => {
    drawn({ actions: <button type="button">Release</button> });
    expect(screen.getByTestId("live-link-sub")).toHaveClass("flex", "flex-wrap");
    expect(screen.getByTestId("live-link-sub").querySelector("[data-part=status]")).toHaveClass("min-w-0", "flex-[1_1_16rem]");
    expect(screen.getByTestId("live-link-actions")).toHaveClass("flex", "flex-wrap");
  });

  it("is not drawn at all when there is nothing to say and nothing to press", () => {
    drawn({ status: [], details: undefined });
    expect(screen.queryByTestId("live-link-sub")).toBeNull();
  });
});

describe("ConnectionSection: test ids", () => {
  it("take the prefix, so a page keeps the ids its tests know", () => {
    drawn({ testIdPrefix: "propresenter-live-link", actions: <button type="button">x</button> });
    for (const id of ["propresenter-live-link-sub", "propresenter-live-link-actions", "propresenter-live-link-details"]) expect(screen.getByTestId(id)).toBeInTheDocument();
    expect(screen.queryByTestId("live-link-sub")).toBeNull();
  });

  it("put the connection's tone on the block (the accent comes from the page, never new colours)", () => {
    drawn({ tone: "playback" });
    expect(screen.getByTestId("section")).toHaveAttribute("data-tone", "playback");
  });
});
