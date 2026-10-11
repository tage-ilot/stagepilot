import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SetupPanelHeader } from "./SetupPanelHeader";
import { StatusCard } from "./StatusCard";
import { connectionWord } from "./ui";
import type { ConnectionStatus } from "../types";

const words: Array<[ConnectionStatus | "loading", string]> = [
  ["connected", "Connected"],
  ["connecting", "Connecting"],
  ["disconnected", "Not connected"],
  ["error", "Error"],
  ["loading", "Not connected"],
];

describe("status words", () => {
  it.each(words)("maps %s to %s", (status, word) => {
    expect(connectionWord[status]).toBe(word);
  });

  it.each(words)("SetupPanelHeader badge shows the word for %s, never the raw enum, and keeps data-status", (status, word) => {
    render(<SetupPanelHeader closeLabel="Close" description="d" status={status} title="Lights" />);
    const badge = document.querySelector("[data-status-badge]")!;
    expect(badge).toHaveTextContent(new RegExp(`^${word}$`));
    expect(badge).toHaveAttribute("data-status", status);
    // Sentence case in the DOM; the badge's uppercase class does the capitals.
    expect(badge.className).toContain("uppercase");
  });

  it("SetupPanelHeader shows the kicker, the connection name as title and the sentence", () => {
    render(<SetupPanelHeader closeLabel="Close" description="One sentence." headingId="h" status="connected" title="Presentation" />);
    expect(screen.getByText("Connection configuration")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Presentation" })).toHaveAttribute("id", "h");
    expect(screen.getByText("One sentence.")).toBeInTheDocument();
    expect(document.querySelector("[data-setup-panel-header]")!.className).toContain("grid-cols-[minmax(0,1fr)_auto]");
  });

  it.each(words.filter(([status]) => status !== "loading") as Array<[ConnectionStatus, string]>)(
    "StatusCard shows %s as %s in sentence case without a CSS uppercase class and keeps data-status",
    (status, word) => {
      render(
        <StatusCard accessibleTitle="Lights" active={false} controls="x" detail="Detail" icon={null} onClick={() => undefined} status={status} title="Lights" />,
      );
      const card = screen.getByRole("button", { name: `Lights ${word}. Detail` });
      expect(card).toHaveAttribute("data-status", status);
      const label = card.querySelector(".status-card-status-word")!;
      expect(label).toHaveTextContent(new RegExp(`^${word}$`));
      expect(label.className).not.toContain("uppercase");
      expect(card).not.toHaveTextContent(/disconnected/);
    },
  );
});
