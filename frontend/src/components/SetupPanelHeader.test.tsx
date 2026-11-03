import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SetupPanelHeader } from "./SetupPanelHeader";
import { ConnectionMethod } from "./ui/ConnectionMethod";

const options = [
  { value: "api", label: "Playback API", role: "primary" as const },
  { value: "midi", label: "MIDI", role: "alternate" as const },
];
const header = (withMethod = true) => render(
  <SetupPanelHeader
    closeLabel="Close" onClose={() => undefined} status="connected" title="Playback"
    method={withMethod ? <ConnectionMethod onChange={() => undefined} options={options} placement="header" value="api" /> : undefined}
  />,
);

describe("SetupPanelHeader method slot", () => {
  it("puts the connection method in its own header slot, inside the header grid", () => {
    header();
    const root = document.querySelector("[data-setup-panel-header]")!;
    const slot = root.querySelector("[data-header-method]")!;
    expect(slot).not.toBeNull();
    expect(slot.querySelector("[data-connection-method]")).not.toBeNull();
    expect(slot.className).toContain("setup-header-method");
  });

  it("draws no slot without a method (Backend, Services keep their own layout)", () => {
    header(false);
    expect(document.querySelector("[data-header-method]")).toBeNull();
  });

  it("drops the visible kicker in header mode but keeps the accessible group name", () => {
    header();
    const group = screen.getByRole("radiogroup", { name: "Connection method" });
    expect(within(group).getAllByRole("radio")).toHaveLength(2);
    const legend = document.querySelector("legend")!;
    expect(legend).toHaveTextContent("Connection method");
    expect(legend.className).toContain("connection-method-kicker");
  });

  it("keeps the status pill directly before the close button", () => {
    header();
    const root = document.querySelector("[data-setup-panel-header]")!;
    const slot = root.querySelector("[data-status-slot]")!;
    expect(slot.nextElementSibling).toBe(screen.getByRole("button", { name: "Close" }));
  });

  it("the default ConnectionMethod (Backend, Lightkey) keeps its visible kicker", () => {
    render(<ConnectionMethod onChange={() => undefined} options={options} value="api" />);
    expect(document.querySelector("legend")!.className).not.toContain("connection-method-kicker");
  });
});

describe("header layout rule (jsdom has no container queries, so the stylesheet is checked)", () => {
  it("is a container query on the header itself, centring the method between title and status", async () => {
    const { readFileSync } = (await import("node:fs" as string)) as { readFileSync(file: string, enc: "utf8"): string };
    const css = readFileSync(`${(globalThis as unknown as { process: { cwd(): string } }).process.cwd()}/src/index.css`, "utf8");
    expect(css).toMatch(/\.setup-header\s*\{[^}]*container-type:\s*inline-size/);
    expect(css).toMatch(/@container \(min-width: 57rem\)\s*\{\s*\.setup-header-grid\[data-has-method\]\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\) auto minmax\(0, 1fr\)/);
    expect(css).toMatch(/\.setup-header-method \.connection-method-kicker/);
  });
});
