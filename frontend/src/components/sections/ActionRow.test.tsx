import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ActionButton, ActionRow } from "./ActionRow";

describe("ActionRow", () => {
  it("is one wrapping row of buttons, then its one note", () => {
    render(
      <ActionRow note="Test connection and Refresh use the saved settings." noteId="saved-note" testId="row">
        <ActionButton label="Refresh timers" onClick={() => undefined} />
        <ActionButton label="Test" onClick={() => undefined} />
      </ActionRow>,
    );
    const row = screen.getByTestId("row");
    expect(row).toHaveAttribute("data-section", "actions");
    expect(Array.from(row.children).map((child) => child.getAttribute("data-part"))).toEqual(["buttons", "note"]);
    expect(row.querySelector("[data-part=buttons]")).toHaveClass("flex", "flex-wrap", "gap-2");
    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual(["Refresh timers", "Test"]);
    expect(screen.getByText(/use the saved settings/)).toHaveAttribute("id", "saved-note");
  });

  it("draws no note line and no spacing for it when there is no note", () => {
    render(<ActionRow testId="row"><ActionButton label="Reload" onClick={() => undefined} /></ActionRow>);
    const row = screen.getByTestId("row");
    expect(row.querySelector("[data-part=note]")).toBeNull();
    expect(row).not.toHaveClass("space-y-4");
  });

  it("names the group for a screen reader only when asked", () => {
    const { rerender } = render(<ActionRow><ActionButton label="Reload" onClick={() => undefined} /></ActionRow>);
    expect(screen.queryByRole("group")).toBeNull();
    rerender(<ActionRow aria-label="Plan actions"><ActionButton label="Reload" onClick={() => undefined} /></ActionRow>);
    expect(screen.getByRole("group", { name: "Plan actions" })).toBeInTheDocument();
  });

  it("a button shows its busy words while its own request runs, and is a tinted button in the connection's tone", () => {
    const { rerender } = render(<ActionRow><ActionButton label="Refresh timers and Looks" busyLabel="Refreshing…" onClick={() => undefined} tone="presentation" /></ActionRow>);
    const button = screen.getByRole("button", { name: "Refresh timers and Looks" });
    expect(button).toHaveClass("text-integration-presentation-text");
    rerender(<ActionRow><ActionButton busy label="Refresh timers and Looks" busyLabel="Refreshing…" onClick={() => undefined} tone="presentation" /></ActionRow>);
    expect(screen.getByRole("button", { name: "Refreshing…" })).toBeInTheDocument();
  });

  it("keeps the label when the button has no busy words", () => {
    render(<ActionRow><ActionButton busy label="Reload" onClick={() => undefined} /></ActionRow>);
    expect(screen.getByRole("button", { name: "Reload" })).toBeInTheDocument();
  });

  it("a disabled button does nothing; an enabled one calls back once", async () => {
    const onClick = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(<ActionRow><ActionButton disabled label="Test" onClick={onClick} /></ActionRow>);
    await user.click(screen.getByRole("button", { name: "Test" }));
    expect(onClick).not.toHaveBeenCalled();
    rerender(<ActionRow><ActionButton label="Test" onClick={onClick} /></ActionRow>);
    await user.click(screen.getByRole("button", { name: "Test" }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("the tab order is the button order", async () => {
    const user = userEvent.setup();
    render(<ActionRow><ActionButton label="One" onClick={() => undefined} /><ActionButton label="Two" onClick={() => undefined} /></ActionRow>);
    await user.tab();
    expect(document.activeElement).toHaveTextContent("One");
    await user.tab();
    expect(document.activeElement).toHaveTextContent("Two");
  });

  it("a primary action is the filled one, and takes no tone", () => {
    render(<ActionRow><ActionButton label="Connect" onClick={() => undefined} tone="presentation" variant="primary" /></ActionRow>);
    expect(screen.getByRole("button", { name: "Connect" })).toHaveClass("bg-primary");
  });
});
