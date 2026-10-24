import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SetupPanelHeader } from "./SetupPanelHeader";
import { StatusCard } from "./StatusCard";
import { connectionWord } from "./ui";
import type { ConnectionStatus } from "../types";

const words: Array<[ConnectionStatus | "loading", string]> = [
  ["connected", "Connected"],
  ["connecting", "Connecting"],
  ["disconnected", "Disconnected"],
  ["error", "Error"],
  ["ready", "Ready"],
  ["loading", "Disconnected"],
];

describe("status words", () => {
  it.each(words)("maps %s to %s", (status, word) => {
    expect(connectionWord[status]).toBe(word);
  });

  it.each(words)("SetupPanelHeader badge shows the word for %s, never the raw enum, and keeps data-status", (status, word) => {
    render(<SetupPanelHeader closeLabel="Close" description="d" status={status} title="Lighting" />);
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

  it("keeps the status pill in one fixed slot beside the close button, centred on its height, with or without a description or extra badge", () => {
    const { rerender } = render(<SetupPanelHeader closeLabel="Close" headingId="h" status="connected" title="Lighting" onClose={() => undefined} />);
    const slot = () => document.querySelector("[data-status-slot]")!;
    const base = slot().className;
    for (const klass of ["min-[480px]:col-start-2", "min-[480px]:row-start-1", "min-[480px]:h-ds-touch-target", "min-[480px]:justify-self-end"]) {
      expect(base).toContain(klass);
    }
    expect(slot().querySelector("[data-status-badge]")).toHaveTextContent("Connected");
    rerender(<SetupPanelHeader closeLabel="Close" description="A long sentence about this page." headingId="h" status="disconnected" title="Lighting" onClose={() => undefined} />);
    expect(slot().className).toBe(base);
    rerender(<SetupPanelHeader closeLabel="Close" headingId="h" status="connected" statusDetail="MIDI · Alternate" title="Playback" onClose={() => undefined} />);
    expect(slot().className).toBe(base);
    // The extra badge sits before the pill, so the pill stays next to the close button.
    expect([...slot().children].map((child) => child.textContent)).toEqual(["MIDI · Alternate", "Connected"]);
  });

  it.each(words.filter(([status]) => status !== "loading") as Array<[ConnectionStatus, string]>)(
    "StatusCard shows %s as %s in sentence case without a CSS uppercase class and keeps data-status",
    (status, word) => {
      render(
        <StatusCard accessibleTitle="Lighting" active={false} controls="x" detail="Detail" icon={null} onClick={() => undefined} status={status} title="Lighting" />,
      );
      const card = screen.getByRole("button", { name: `Lighting ${word}. Detail` });
      expect(card).toHaveAttribute("data-status", status);
      const label = card.querySelector(".status-card-status-word")!;
      expect(label).toHaveTextContent(new RegExp(`^${word}$`));
      expect(label.className).not.toContain("uppercase");
      expect(card).not.toHaveTextContent(/disconnected/);
    },
  );
});
