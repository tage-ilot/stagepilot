import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SectionLabel } from "./SectionLabel";
import { Panel } from "./Panel";
import { accentClasses } from "./accent";
import { SetupPanelHeader } from "../SetupPanelHeader";

describe("SectionLabel (panels read like the style guide)", () => {
  it("is a real heading drawn as the small uppercase kicker, not a bold sentence", () => {
    render(<SectionLabel>General configuration</SectionLabel>);
    const h = screen.getByRole("heading", { level: 3, name: "General configuration" });
    expect(h).toHaveClass("font-type-kicker", "text-type-kicker", "uppercase", "text-ink-muted");
    expect(h.className).not.toMatch(/font-semibold|font-bold/);
  });

  it("appends className last", () => {
    render(<SectionLabel className="mt-2">People</SectionLabel>);
    expect(screen.getByRole("heading", { name: "People" }).className.endsWith("mt-2")).toBe(true);
  });
});

describe("setup panel header title", () => {
  const header = (tone?: Parameters<typeof Panel>[0]["tone"]) =>
    render(
      <Panel tone={tone} variant="setup">
        <SetupPanelHeader closeLabel="Close" description="d" onClose={() => {}} status="connected" title="Playback" />
      </Panel>,
    );

  it("wears the connection colour, larger and bold, inside a panel", () => {
    header("playback");
    const title = screen.getByRole("heading", { level: 2, name: "Playback" });
    expect(title).toHaveClass("text-xl", "font-bold", accentClasses.playback.title);
  });

  it("the title colour is the connection colour for four connections and the pale tint for Backend", () => {
    expect(accentClasses.services.title).toBe("text-integration-services-soft");
    expect(accentClasses.presentation.title).toBe("text-integration-presentation-soft");
    expect(accentClasses.playback.title).toBe("text-integration-playback-soft");
    expect(accentClasses.lighting.title).toBe("text-integration-lighting-soft");
    // Backend's colour IS the primary red (3.2:1 on the panel), so its title uses the readable tint.
    expect(accentClasses.backend.title).toBe("text-integration-backend-text");
  });

  it("falls back to plain ink outside a panel and keeps the eyebrow as the kicker", () => {
    render(<SetupPanelHeader closeLabel="Close" description="d" status="connected" title="Alone" />);
    expect(screen.getByRole("heading", { name: "Alone" })).toHaveClass("text-ink");
    expect(screen.getByText("Connection configuration")).toHaveClass("font-type-kicker", "uppercase");
  });
});
