import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Badge } from "./Badge";
import { Button } from "./Button";
import { Dialog } from "./Dialog";
import { InlineAlert } from "./InlineAlert";
import { Panel, type PanelIdentity } from "./Panel";
import { SaveBar } from "./SaveBar";
import { StatusDot } from "./StatusDot";

const tones: PanelIdentity[] = ["services", "presentation", "playback", "lighting", "backend"];
const classes = (el: Element) => el.className.split(/\s+/);

// DESIGN.md "Accent inside a configuration panel": the connection's colour is the only accent inside its panel.
describe("accent inside a connection panel", () => {
  it.each(tones)("%s: primary Button is filled with the connection colour, not red", (tone) => {
    render(<Panel tone={tone} variant="setup"><Button variant="primary">Save configuration</Button></Panel>);
    const c = classes(screen.getByRole("button"));
    expect(c).toContain(`bg-integration-${tone}`);
    expect(c).not.toContain("bg-primary");
  });

  it.each(["services", "presentation"] as const)("%s: the fill is light, so its label is dark", (tone) => {
    render(<Panel tone={tone}><Button variant="primary">Scan</Button></Panel>);
    expect(classes(screen.getByRole("button"))).toContain("text-on-light");
  });

  it.each(["playback", "lighting", "backend"] as const)("%s: the fill is dark enough for white", (tone) => {
    render(<Panel tone={tone}><Button variant="primary">Scan</Button></Panel>);
    expect(classes(screen.getByRole("button"))).toContain("text-on-primary");
  });

  it.each(tones)("%s: the default tinted Button and a success Badge/Dot/Alert wear the connection colour, not teal or green", (tone) => {
    render(
      <Panel tone={tone}>
        <Button variant="tinted">Refresh</Button>
        <Badge tone="success" data-testid="badge">Connected</Badge>
        <StatusDot tone="success" data-testid="dot" />
        <InlineAlert tone="success">Changes saved</InlineAlert>
        <InlineAlert tone="info" data-testid="info">Note</InlineAlert>
      </Panel>,
    );
    const html = document.body.innerHTML;
    expect(html).toContain(`integration-${tone}`);
    expect(html).not.toMatch(/\b(?:bg|text|border)-(?:success|success-soft|success-text|info|info-text)\b/);
  });

  it("an explicit tone and the danger and warning looks are never swapped", () => {
    render(
      <Panel tone="services">
        <Button variant="tinted" tone="warning">Careful</Button>
        <Button variant="destructive">Remove</Button>
        <InlineAlert tone="danger">Failed</InlineAlert>
        <InlineAlert tone="warning">Heads up</InlineAlert>
      </Panel>,
    );
    expect(classes(screen.getByRole("button", { name: "Careful" }))).toContain("text-warning-text");
    expect(classes(screen.getByRole("button", { name: "Remove" }))).toContain("bg-danger-solid");
    expect(classes(screen.getAllByRole("alert")[0]!)).toContain("text-danger-text");
    expect(classes(screen.getAllByRole("alert")[1]!)).toContain("text-warning-text");
  });

  it.each(tones)("%s: the Save bar is a deep tint of the connection colour, not the dark blue well", (tone) => {
    const { container } = render(<Panel tone={tone}><SaveBar state={{ kind: "auto", at: "10:02" }} /></Panel>);
    const bar = container.querySelector("[data-save-bar]")!;
    expect(classes(bar)).toContain(`bg-integration-${tone}-soft/[0.22]`);
    expect(classes(bar)).not.toContain("bg-surface-900");
    expect(bar.querySelector("[data-save-status]")!.parentElement!.className).toContain(`text-integration-${tone}-text`);
  });

  it("an unsaved or failed Save bar keeps its warning and error outline inside a panel", () => {
    const { container, rerender } = render(<Panel tone="services"><SaveBar state={{ kind: "held", count: 2 }} /></Panel>);
    expect(classes(container.querySelector("[data-save-bar]")!)).toContain("border-warning/60");
    rerender(<Panel tone="services"><SaveBar state={{ kind: "error", message: "Nope" }} /></Panel>);
    expect(classes(container.querySelector("[data-save-bar]")!)).toContain("border-danger/60");
  });

  it("the setup wash comes from the connection colour", () => {
    const { container } = render(<Panel tone="lighting" variant="setup">x</Panel>);
    expect(classes(container.firstElementChild!)).toContain("from-integration-lighting/[0.13]");
    expect(classes(container.firstElementChild!)).not.toContain("from-primary/[0.13]");
  });

  it("outside a panel nothing changes: primary is red and success is the deep green", () => {
    render(<><Button variant="primary">Go</Button><Badge tone="success">Ready</Badge></>);
    expect(classes(screen.getByRole("button"))).toContain("bg-primary");
    expect(classes(screen.getByText("Ready"))).toContain("text-success-text");
  });

  it("a dialog opened from a panel keeps the app colours", () => {
    render(
      <Panel tone="services">
        <Dialog open title="Sure?" actions={<Button variant="primary">Confirm</Button>} />
      </Panel>,
    );
    expect(classes(screen.getByRole("button", { name: "Confirm" }))).toContain("bg-primary");
  });
});
