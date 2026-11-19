import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Button } from "../ui/Button";
import { Panel } from "../ui/Panel";
import { SectionNotice, type NoticeTone } from "./SectionNotice";

describe("SectionNotice: a box", () => {
  it.each<[NoticeTone, "alert" | "status"]>([["warning", "alert"], ["danger", "alert"], ["info", "status"], ["success", "status"]])("%s is role %s, as InlineAlert is everywhere", (tone, role) => {
    render(<SectionNotice tone={tone}>Words</SectionNotice>);
    const notice = screen.getByRole(role);
    expect(notice).toHaveTextContent("Words");
    expect(notice).toHaveAttribute("data-tone", tone);
  });

  it("can be a polite live region as well (a result that appears after an action)", () => {
    render(<SectionNotice live tone="success">Saved</SectionNotice>);
    expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
  });

  it("is a bold first line, the words, then the buttons", async () => {
    const onClick = vi.fn();
    render(<SectionNotice actions={<Button onClick={onClick}>Reconnect</Button>} testId="n" title="Your connection has expired" tone="warning">Sign in again.</SectionNotice>);
    const notice = screen.getByTestId("n");
    expect(Array.from(notice.children).map((child) => child.tagName)).toEqual(["P", "P", "DIV"]);
    expect(notice.children[0]).toHaveClass("font-semibold");
    expect(notice.children[0]).toHaveTextContent("Your connection has expired");
    await userEvent.setup().click(screen.getByRole("button", { name: "Reconnect" }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("wears the connection's colour for info and success, never for warning and danger, unless it is neutral", () => {
    render(
      <Panel tone="services">
        <SectionNotice tone="info">Info</SectionNotice>
        <SectionNotice neutral tone="info">Neutral</SectionNotice>
        <SectionNotice tone="danger">Danger</SectionNotice>
      </Panel>,
    );
    expect(screen.getByText("Info")).toHaveClass("text-integration-services-text");
    expect(screen.getByText("Neutral")).toHaveClass("text-info-text");
    expect(screen.getByText("Danger")).toHaveClass("text-danger-text");
  });
});

describe("SectionNotice: a line", () => {
  it("is a quiet sentence with no role unless the page asks", () => {
    render(<SectionNotice id="blocked" tone="info" variant="line">Enter a host before saving.</SectionNotice>);
    const line = screen.getByText("Enter a host before saving.");
    expect(line.tagName).toBe("P");
    expect(line).not.toHaveAttribute("role");
    expect(line).toHaveAttribute("id", "blocked");
    expect(line).toHaveClass("text-sm", "text-ink-body");
  });

  it("takes the role the page asks for, and the tone's colour", () => {
    render(<>
      <SectionNotice role="alert" tone="danger" variant="line">Bad</SectionNotice>
      <SectionNotice role="status" size="xs" tone="warning" variant="line">Restart</SectionNotice>
      <SectionNotice quiet tone="info" variant="line">Hint</SectionNotice>
    </>);
    expect(screen.getByRole("alert")).toHaveClass("text-danger-text");
    expect(screen.getByRole("status")).toHaveClass("text-xs", "text-warning-text");
    expect(screen.getByText("Hint")).toHaveClass("text-ink-muted");
  });

  it("a success line wears the connection's colour inside its panel", () => {
    render(<Panel tone="backend"><SectionNotice tone="success" variant="line">Done</SectionNotice></Panel>);
    expect(screen.getByText("Done")).toHaveClass("text-integration-backend-text");
  });
  it("a line can have its button beside the words: one row that carries the role and the test id, the words first", () => {
    render(<SectionNotice actions={<button type="button">Use the OSC host</button>} role="status" testId="host-notice" tone="warning" variant="line">The API is set to another host.</SectionNotice>);
    const row = screen.getByTestId("host-notice");
    expect(row).toHaveAttribute("role", "status");
    expect(row).toHaveClass("flex", "flex-wrap", "items-center", "gap-2");
    expect(row.firstElementChild).toHaveTextContent("The API is set to another host.");
    expect(row.firstElementChild).toHaveClass("text-warning-text");
    expect(row.lastElementChild).toBe(screen.getByRole("button", { name: "Use the OSC host" }));
  });
});
