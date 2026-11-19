import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PanelBody, PanelLayout } from "./PanelLayout";

const node = (id: string) => <p data-testid={id}>{id}</p>;
const all = { connection: node("connection"), settings: node("settings"), optional: node("optional"), actions: node("actions"), content: node("content"), notices: node("notices"), saveBar: node("saveBar"), summary: node("summary") };
const order = (ids: string[]) => ids.map((id) => screen.getByTestId(id)).every((n, i, list) => i === 0 || Boolean(list[i - 1]!.compareDocumentPosition(n) & Node.DOCUMENT_POSITION_FOLLOWING));

describe("PanelLayout: the order every configuration page follows", () => {
  it("is header, alert lines, connection, settings, actions, optional sections, the page's content, notices, Save bar, summary", () => {
    render(<PanelLayout alerts={node("alerts")} header={node("header")} id="p" tone="presentation" {...all} />);
    expect(order(["header", "alerts", "connection", "settings", "actions", "optional", "content", "notices", "saveBar", "summary"])).toBe(true);
  });

  it("is the Panel in the connection's colour, with its id and the setup look", () => {
    render(<PanelLayout header={node("header")} id="propresenter-configuration" tone="presentation" {...all} />);
    const panel = document.getElementById("propresenter-configuration")!;
    expect(panel).toHaveAttribute("data-variant", "setup");
    expect(panel).toHaveClass("mt-5", "border-t-integration-presentation-soft");
    expect(panel).toHaveAttribute("data-panel-layout");
    expect(panel.firstElementChild).toBe(screen.getByTestId("header"));
  });

  it("passes aria-labelledby and aria-busy to the panel, and says nothing when the page does not", () => {
    const { rerender } = render(<PanelLayout aria-busy aria-labelledby="h" header={node("header")} id="p" tone="services" />);
    expect(document.getElementById("p")).toHaveAttribute("aria-busy", "true");
    expect(document.getElementById("p")).toHaveAttribute("aria-labelledby", "h");
    rerender(<PanelLayout header={node("header")} id="p" tone="services" />);
    expect(document.getElementById("p")).not.toHaveAttribute("aria-busy");
  });

  it("a page leaves a slot empty to skip it: nothing is drawn for it and nothing else moves", () => {
    render(<PanelLayout header={node("header")} id="p" settings={node("settings")} saveBar={node("saveBar")} tone="midi" />);
    const body = document.querySelector("[data-panel-body]")!;
    expect(Array.from(body.children).map((child) => child.getAttribute("data-testid"))).toEqual(["settings", "saveBar"]);
    expect(document.querySelector("[data-slot=alerts]")).toBeNull();
  });

  it("the body is one column with one gap (space-y-4) under the header, the same on every page", () => {
    render(<PanelLayout header={node("header")} id="p" tone="lighting" {...all} />);
    expect(document.querySelector("[data-panel-body]")).toHaveClass("mt-6", "space-y-4");
  });

  it("the alert lines sit under the header and hide themselves when they hold nothing", () => {
    const { rerender } = render(<PanelLayout alerts={<>{null}</>} header={node("header")} id="p" tone="lighting" />);
    const slot = document.querySelector("[data-slot=alerts]")!;
    expect(slot).toHaveClass("mt-4", "space-y-3", "empty:hidden");
    expect(slot).toBeEmptyDOMElement();
    rerender(<PanelLayout alerts={node("alert")} header={node("header")} id="p" tone="lighting" />);
    expect(screen.getByTestId("alert").parentElement).toBe(document.querySelector("[data-slot=alerts]"));
  });

  it("the summary can open the page instead of closing it (Backend's Status cards)", () => {
    render(<PanelLayout header={node("header")} id="p" tone="backend" {...all} summaryAt="top" />);
    expect(order(["header", "summary", "settings", "saveBar"])).toBe(true);
    expect(order(["saveBar", "summary"])).toBe(false);
  });

  it("a body the page built itself replaces the slots, and `below` follows it (the Save bar of the MIDI / Playback page)", () => {
    render(<PanelLayout below={node("below")} header={node("header")} id="p" tone="midi" {...all}><PanelBody settings={node("own")} /></PanelLayout>);
    expect(screen.queryByTestId("connection")).toBeNull();
    expect(order(["header", "own", "below"])).toBe(true);
  });
});
